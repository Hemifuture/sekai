"""Fit monthly wet-day fraction (>0.1 mm, CRU CL 2.0 rd0) against monthly mean precipitation (mm/day).

Usage: python -I fit_wetday.py <cru_cl2_dir> <out_json>
Data: New, Lister, Hulme & Makin (2002) CRU CL 2.0, 10' grids grid_10min_pre.dat.gz, grid_10min_rd0.dat.gz.
"""
import gzip
import json
import sys

import numpy as np
from scipy.optimize import least_squares

NDAYS = np.array([31, 28.25, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31])


def load(path, ncol):
    with gzip.open(path, "rt") as fh:
        a = np.loadtxt(fh, dtype=np.float64)
    assert a.shape[1] == ncol, a.shape
    return a


def main():
    d, out = sys.argv[1], sys.argv[2]
    pre = load(f"{d}/grid_10min_pre.dat.gz", 26)
    rd0 = load(f"{d}/grid_10min_rd0.dat.gz", 14)
    # join on (lat, lon) in 1/1000 degree
    def key(a):
        return np.round(a[:, 0] * 1000).astype(np.int64) * 1_000_000 + np.round(a[:, 1] * 1000).astype(np.int64)
    kp, kr = key(pre), key(rd0)
    common, ip, ir = np.intersect1d(kp, kr, return_indices=True)
    lat = pre[ip, 0]
    P = pre[ip, 2:14] / NDAYS          # mm/day
    F = rd0[ir, 2:14] / NDAYS          # wet-day fraction
    ann = pre[ip, 2:14].sum(axis=1)    # mm/yr
    w_cell = np.cos(np.deg2rad(lat))
    n_cells = len(common)

    # flatten (cell, month)
    Pf, Ff = P.ravel(), F.ravel()
    W = np.repeat(w_cell, 12)
    LAT = np.repeat(lat, 12)
    ANN = np.repeat(ann, 12)

    artefacts = {
        "cells_pre": int(len(pre)), "cells_rd0": int(len(rd0)), "cells_joined": int(n_cells),
        "points": int(Pf.size),
        "rd0_zero_with_pre_gt0": int(((Ff == 0) & (Pf > 0)).sum()),
        "pre_zero": int((Pf == 0).sum()),
        "rd0_ge_ndays": int((Ff >= 1).sum()),
        "f_gt_1": int((Ff > 1).sum()),
    }
    Ff = np.clip(Ff, 0, 1)  # CRU constrains rd0 <= days in month; clip only rounding

    def model(theta, p):
        a, b = theta
        return 1.0 - np.exp(-a * np.power(p, b))

    sw = np.sqrt(W)
    res = least_squares(lambda t: sw * (model(t, Pf) - Ff), x0=[0.3, 0.6], bounds=([1e-6, 0.05], [50, 3]))
    a, b = res.x
    pred = model(res.x, Pf)
    r = pred - Ff

    def wstats(mask):
        ww, rr, ff = W[mask], r[mask], Ff[mask]
        if ww.sum() == 0:
            return None
        mean_f = np.average(ff, weights=ww)
        sse = np.sum(ww * rr ** 2)
        sst = np.sum(ww * (ff - mean_f) ** 2)
        order = np.argsort(rr)
        cw = np.cumsum(ww[order]) / ww.sum()
        q = {f"q{int(p*100):02d}": float(rr[order][np.searchsorted(cw, p)]) for p in (0.05, 0.25, 0.5, 0.75, 0.95)}
        abs_order = np.argsort(np.abs(rr))
        cwa = np.cumsum(ww[abs_order]) / ww.sum()
        return {
            "n": int(mask.sum()),
            "R2": float(1 - sse / sst) if sst > 0 else None,
            "bias": float(np.average(rr, weights=ww)),
            "MAE": float(np.average(np.abs(rr), weights=ww)),
            "RMSE": float(np.sqrt(np.average(rr ** 2, weights=ww))),
            "abs_p90": float(np.abs(rr)[abs_order][np.searchsorted(cwa, 0.9)]),
            "mean_f_obs": float(mean_f),
            "MAE_days_per_30d": float(30 * np.average(np.abs(rr), weights=ww)),
            **q,
        }

    allm = np.ones_like(Pf, bool)
    strata = {"global": allm}
    for lo, hi in [(-60, -30), (-30, -10), (-10, 10), (10, 30), (30, 50), (50, 70), (70, 90)]:
        strata[f"lat[{lo},{hi})"] = (LAT >= lo) & (LAT < hi)
    strata["arid_ann_lt_250mm"] = ANN < 250
    strata["semiarid_ann_250_500mm"] = (ANN >= 250) & (ANN < 500)
    strata["humid_ann_ge_1000mm"] = ANN >= 1000
    for lo, hi in [(0, 0.1), (0.1, 0.5), (0.5, 1), (1, 2), (2, 3), (1, 3), (3, 5), (5, 10), (10, 100)]:
        strata[f"P[{lo},{hi})mm/d"] = (Pf >= lo) & (Pf < hi)
    for lo, hi in [(10, 30), (30, 50), (50, 70)]:
        strata[f"P[1,3)&lat[{lo},{hi})"] = (Pf >= 1) & (Pf < 3) & (np.abs(LAT) >= lo) & (np.abs(LAT) < hi)
    strata["P[1,3)&arid"] = (Pf >= 1) & (Pf < 3) & (ANN < 250)
    stats = {k: wstats(m) for k, m in strata.items()}

    # Nonparametric ceiling: best possible predictor using P alone = conditional mean in narrow P bins.
    edges = np.quantile(Pf, np.linspace(0, 1, 401))
    edges = np.unique(edges)
    idx = np.clip(np.searchsorted(edges, Pf, side="right") - 1, 0, len(edges) - 2)
    sw_b = np.bincount(idx, W)
    mean_b = np.bincount(idx, W * Ff) / np.maximum(sw_b, 1e-300)
    rb = mean_b[idx] - Ff
    mf = np.average(Ff, weights=W)
    ceiling = {
        "bins": int(len(edges) - 1),
        "R2_ceiling": float(1 - np.sum(W * rb ** 2) / np.sum(W * (Ff - mf) ** 2)),
        "MAE_ceiling": float(np.average(np.abs(rb), weights=W)),
        "MAE_ceiling_P1_3": float(np.average(np.abs(rb)[(Pf >= 1) & (Pf < 3)], weights=W[(Pf >= 1) & (Pf < 3)])),
    }

    # Conditional spread of observed f at selected P values (width of what P alone cannot resolve).
    spread = {}
    for p0 in (0.5, 1.0, 2.0, 3.0, 5.0):
        m = (Pf >= p0 * 0.95) & (Pf < p0 * 1.05)
        ff, ww = Ff[m], W[m]
        o = np.argsort(ff)
        cw = np.cumsum(ww[o]) / ww.sum()
        spread[f"P~{p0}"] = {f"f_q{int(q*100):02d}": float(ff[o][np.searchsorted(cw, q)]) for q in (0.05, 0.25, 0.5, 0.75, 0.95)}
        spread[f"P~{p0}"]["model"] = float(model(res.x, np.array([p0]))[0])

    # Per-latitude-band refit to see whether zonal partitioning helps
    zonal = {}
    for lo, hi in [(0, 10), (10, 30), (30, 50), (50, 90)]:
        m = (np.abs(LAT) >= lo) & (np.abs(LAT) < hi)
        rz = least_squares(lambda t: np.sqrt(W[m]) * (model(t, Pf[m]) - Ff[m]), x0=[a, b], bounds=([1e-6, 0.05], [50, 3]))
        rr = model(rz.x, Pf[m]) - Ff[m]
        m13 = (Pf[m] >= 1) & (Pf[m] < 3)
        zonal[f"|lat|[{lo},{hi})"] = {
            "a": float(rz.x[0]), "b": float(rz.x[1]),
            "MAE": float(np.average(np.abs(rr), weights=W[m])),
            "MAE_P1_3": float(np.average(np.abs(rr[m13]), weights=W[m][m13])),
            "MAE_global_fit_same_points": stats_mae(r[m], W[m]),
        }

    result = {"model": "f = 1 - exp(-a * P^b), P in mm/day, weights cos(lat)",
              "a": float(a), "b": float(b), "cost": float(res.cost), "status": int(res.status),
              "artefacts": artefacts, "stats": stats, "ceiling_P_only": ceiling,
              "conditional_spread": spread, "zonal_refit": zonal}
    with open(out, "w") as fh:
        json.dump(result, fh, indent=1)
    print(json.dumps(result, indent=1))


def stats_mae(r, w):
    return float(np.average(np.abs(r), weights=w))


if __name__ == "__main__":
    main()
