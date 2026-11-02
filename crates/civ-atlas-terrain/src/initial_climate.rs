// SPDX-License-Identifier: AGPL-3.0-only
// Private authoring dependency from Civ Atlas climate.ts, never Sekai climate.
use crate::{
    geometry::SphereGeometry,
    mesh::{blur_field, Mesh},
    params::TerrainParams,
    random::{round24, sub_seed},
    util::{check_cancel, order_by_key, piecewise, smoothstep},
    TerrainError,
};
#[derive(Debug)]
pub(crate) struct InitialClimate {
    pub(crate) temperature: Vec<f32>,
    pub(crate) precipitation: Vec<f32>,
    pub(crate) wind_x: Vec<f32>,
    pub(crate) wind_y: Vec<f32>,
}
pub(crate) struct ClimateInput<'a> {
    pub elevation: &'a [f32],
    pub water: &'a [u8],
    pub sst: Option<&'a [f32]>,
    pub seasons: bool,
}
const TEMP_BY_LAT: &[f64] = &[
    0.0, 27.0, 15.0, 26.0, 30.0, 20.0, 45.0, 11.0, 60.0, 1.0, 75.0, -11.0, 90.0, -24.0,
];
const RAIN_BY_LAT: &[f64] = &[
    0.0, 1.25, 8.0, 1.1, 16.0, 0.7, 24.0, 0.38, 32.0, 0.5, 42.0, 0.85, 55.0, 0.9, 65.0, 0.6, 78.0,
    0.35, 90.0, 0.2,
];
const BASE: f64 = 0.01;

struct WindBase {
    wx: Vec<f64>,
    wy: Vec<f64>,
    wind_x: Vec<f32>,
    wind_y: Vec<f32>,
    strength: Vec<f32>,
    up_w: Vec<f32>,
    up_count: Vec<i32>,
    downstream: Vec<u8>,
    band_y: Option<Vec<f64>>,
}
fn wind_at(lat: f64) -> [f64; 2] {
    let a = lat.abs();
    let to_eq = if lat >= 0.0 { 1.0 } else { -1.0 };
    let ang = 0.4_f64.atan2(-1.0) - std::f64::consts::PI * smoothstep(25.0, 35.0, a)
        + std::f64::consts::PI * smoothstep(55.0, 65.0, a);
    [ang.cos(), ang.sin() * to_eq]
}
fn wind_base(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    shift: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<WindBase, TerrainError> {
    let n = mesh.n;
    let mut wx = vec![0.0; n];
    let mut wy = vec![0.0; n];
    let mut wind_x = vec![0.0_f32; n];
    let mut wind_y = vec![0.0_f32; n];
    let mut strength = vec![0.0_f32; n];
    let mut band_y = if shift != 0.0 {
        Some(vec![0.0; n])
    } else {
        None
    };
    for i in 0..n {
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        let lat0 = geo.latitude(i);
        let lat = if shift != 0.0 && (lat0 >= 0.0) == (shift > 0.0) {
            lat0 - shift
        } else {
            lat0
        };
        if let Some(by) = &mut band_y {
            by[i] = ((90.0 - lat) * mesh.height) / 180.0;
        }
        let [x, y] = wind_at(lat);
        wx[i] = x;
        wy[i] = y;
        wind_x[i] = x as f32;
        wind_y[i] = y as f32;
        let s30 = smoothstep(25.0, 35.0, lat.abs());
        let s60 = smoothstep(55.0, 65.0, lat.abs());
        strength[i] = ((1.0 - 2.0 * s30).powi(2) * (1.0 - 2.0 * s60).powi(2)) as f32;
    }
    let mut up_w = vec![0.0_f32; mesh.adj.len()];
    for i in 0..n {
        for (k, up) in up_w
            .iter_mut()
            .enumerate()
            .take(mesh.adj_start[i + 1])
            .skip(mesh.adj_start[i])
        {
            let dot = geo.edge_dot(
                i,
                mesh.adj[k],
                [-f64::from(wind_x[i]), -f64::from(wind_y[i])],
            );
            if dot > 0.1 {
                *up = dot as f32;
            }
        }
    }
    for i in 0..n {
        for k in mesh.adj_start[i]..mesh.adj_start[i + 1] {
            let j = mesh.adj[k];
            if j <= i || up_w[k] == 0.0 {
                continue;
            }
            for m in mesh.adj_start[j]..mesh.adj_start[j + 1] {
                if mesh.adj[m] != i {
                    continue;
                }
                if up_w[m] > 0.0 {
                    up_w[k] = 0.0;
                    up_w[m] = 0.0;
                }
                break;
            }
        }
    }
    let mut up_count = vec![0_i32; n];
    for (i, count) in up_count.iter_mut().enumerate() {
        for &up in &up_w[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
            if up > 0.0 {
                *count += 1;
            }
        }
    }
    let mut downstream = vec![0_u8; mesh.adj.len()];
    for j in 0..n {
        for (k, down) in downstream
            .iter_mut()
            .enumerate()
            .take(mesh.adj_start[j + 1])
            .skip(mesh.adj_start[j])
        {
            let i = mesh.adj[k];
            for (m, &up) in up_w
                .iter()
                .enumerate()
                .take(mesh.adj_start[i + 1])
                .skip(mesh.adj_start[i])
            {
                if mesh.adj[m] != j {
                    continue;
                }
                if up > 0.0 {
                    *down = 1;
                }
                break;
            }
        }
    }
    Ok(WindBase {
        wx,
        wy,
        wind_x,
        wind_y,
        strength,
        up_w,
        up_count,
        downstream,
        band_y,
    })
}

/// One mesh/seed cache for all three private climate evaluations in terrain initialization.
pub(crate) struct InitialClimateModel<'a> {
    mesh: &'a Mesh,
    geo: &'a SphereGeometry<'a>,
    p: &'a TerrainParams,
    sea_temp: Vec<f64>,
    zonal: Vec<f64>,
    t_noise: Vec<f64>,
    p_noise: Vec<f64>,
    winds: [WindBase; 3],
}
impl<'a> InitialClimateModel<'a> {
    pub(crate) fn new(
        mesh: &'a Mesh,
        geo: &'a SphereGeometry<'a>,
        p: &'a TerrainParams,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, TerrainError> {
        let n = mesh.n;
        let fs = 4.0 / mesh.width;
        let t = geo.fbm(sub_seed(p.seed, "temp"), 4, 0.5);
        let rain = geo.fbm(sub_seed(p.seed, "rain"), 4, 0.5);
        let mut sea_temp = Vec::with_capacity(n);
        let mut zonal = Vec::with_capacity(n);
        let mut t_noise = Vec::with_capacity(n);
        let mut p_noise = Vec::with_capacity(n);
        for i in 0..n {
            if i % 256 == 0 {
                check_cancel(cancel)?;
            }
            let lat = geo.latitude(i);
            sea_temp.push(piecewise(TEMP_BY_LAT, lat.abs()));
            zonal.push(piecewise(RAIN_BY_LAT, lat.abs()));
            t_noise.push(t.at(i, fs, 1.0, [0.0, 0.0]));
            p_noise.push(rain.at(i, fs, 1.0, [0.0, 0.0]));
        }
        let winds = [
            wind_base(mesh, geo, 0.0, cancel)?,
            wind_base(mesh, geo, 6.0, cancel)?,
            wind_base(mesh, geo, -6.0, cancel)?,
        ];
        Ok(Self {
            mesh,
            geo,
            p,
            sea_temp,
            zonal,
            t_noise,
            p_noise,
            winds,
        })
    }
    pub(crate) fn compute(
        &self,
        input: ClimateInput<'_>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<InitialClimate, TerrainError> {
        let ClimateInput {
            elevation,
            water,
            sst,
            seasons,
        } = input;
        let mesh = self.mesh;
        let n = mesh.n;
        let mut temperature = vec![0.0_f32; n];
        for i in 0..n {
            let e = if water[i] == 1 {
                0.0
            } else {
                f64::from(elevation[i]).max(0.0)
            };
            temperature[i] =
                (self.sea_temp[i] + self.p.temperature - 0.0065 * e + 1.5 * self.t_noise[i]) as f32;
            if let Some(sst) = sst {
                if water[i] == 1 {
                    temperature[i] = (f64::from(temperature[i]) + f64::from(sst[i])) as f32;
                }
            }
        }
        let mut coast_sst = None;
        let mut coast_w = None;
        if let Some(sst) = sst {
            let sea_f: Vec<_> = water.iter().map(|&w| f32::from(w == 1)).collect();
            let num = blur_field(mesh, sst, 5);
            let den = blur_field(mesh, &sea_f, 5);
            let mut cs = vec![0.0_f32; n];
            let mut cw = vec![0.0_f32; n];
            for i in 0..n {
                if water[i] == 1 || den[i] == 0.0 {
                    continue;
                }
                cs[i] = (f64::from(num[i]) / f64::from(den[i])) as f32;
                cw[i] = (0.85 * (f64::from(den[i]) * 3.0).clamp(0.0, 1.0)) as f32;
            }
            coast_sst = Some(cs);
            coast_w = Some(cw);
        }
        let land_elev: Vec<_> = (0..n)
            .map(|i| {
                if water[i] == 1 {
                    0.0
                } else {
                    elevation[i].max(0.0)
                }
            })
            .collect();
        let smooth_elev = blur_field(mesh, &land_elev, 3);
        let cuts = self.geo.wind_cuts(water);
        let air = Air {
            water,
            temperature: &temperature,
            smooth_elev: &smooth_elev,
            cuts: &cuts,
            sst,
            coast_sst: coast_sst.as_deref(),
            coast_w: coast_w.as_deref(),
        };
        let year = self.carry(&self.winds[0], &air, cancel)?;
        let mut precipitation = vec![0.0_f32; n];
        if !seasons {
            self.add_rain(&year.rain, 1.0, None, &mut precipitation);
        } else {
            for (wind, s) in [(1, 1.0), (2, -1.0)] {
                let mut t_season = vec![0.0_f32; n];
                for i in 0..n {
                    let lat = self.geo.latitude(i);
                    let amp = if water[i] == 1 { 0.08 } else { 0.22 } * lat.abs();
                    t_season[i] = (f64::from(temperature[i])
                        + f64::from(year.land_heat[i])
                        + if lat >= 0.0 { s } else { -s } * amp)
                        as f32;
                }
                let half = self.carry(
                    &self.winds[wind],
                    &Air {
                        temperature: &t_season,
                        ..air
                    },
                    cancel,
                )?;
                self.add_rain(
                    &half.rain,
                    0.5,
                    if sst.is_some() {
                        Some(&half.land_heat)
                    } else {
                        None
                    },
                    &mut precipitation,
                );
            }
        }
        if sst.is_some() {
            for (t, &heat) in temperature.iter_mut().zip(&year.land_heat) {
                *t = (f64::from(*t) + f64::from(heat)) as f32;
            }
        }
        Ok(InitialClimate {
            temperature,
            precipitation,
            wind_x: self.winds[0].wind_x.clone(),
            wind_y: self.winds[0].wind_y.clone(),
        })
    }
    fn add_rain(&self, rain: &[f32], share: f64, heat: Option<&[f32]>, precipitation: &mut [f32]) {
        let rain_s = blur_field(self.mesh, rain, 2);
        for i in 0..self.mesh.n {
            let rel = f64::from(rain_s[i]) / BASE;
            let mut z = self.zonal[i];
            if let Some(heat) = heat {
                if z < 1.0 && heat[i] > 0.0 {
                    z += (1.0 - z) * 0.6 * (f64::from(heat[i]) / 4.0).min(1.0);
                } else if z < 1.0 && heat[i] < 0.0 {
                    z *= 1.0 - 0.8 * (-f64::from(heat[i]) / 5.0).min(1.0);
                }
            }
            let v = rel * z * (1.0 + 0.18 * self.p_noise[i]);
            precipitation[i] = (f64::from(precipitation[i])
                + share * (v * 1500.0 * self.p.rainfall).max(0.0))
                as f32;
        }
    }
    fn carry(
        &self,
        wind: &WindBase,
        air: &Air<'_>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Carried, TerrainError> {
        let mesh = self.mesh;
        let n = mesh.n;
        let key: Vec<_> = (0..n)
            .map(|i| {
                self.geo.downwind(
                    i,
                    [wind.wx[i], wind.wy[i]],
                    Some(air.cuts),
                    wind.band_y.as_ref().map(|by| by[i]),
                ) as f32
            })
            .collect();
        let mut waiting = wind.up_count.clone();
        let by_key = order_by_key(&key);
        let mut order = Vec::with_capacity(n);
        let mut queued = vec![false; n];
        let mut head = 0;
        let mut next_key = 0;
        let mut cycles = 0;
        for &i in &by_key {
            if waiting[i] == 0 {
                order.push(i);
                queued[i] = true;
            }
        }
        while order.len() < n {
            if head == order.len() {
                cycles += 1;
                while queued[by_key[next_key]] {
                    next_key += 1;
                }
                let i = by_key[next_key];
                order.push(i);
                queued[i] = true;
            }
            let j = order[head];
            head += 1;
            for k in mesh.adj_start[j]..mesh.adj_start[j + 1] {
                let i = mesh.adj[k];
                if queued[i] {
                    continue;
                }
                if wind.downstream[k] != 0 {
                    waiting[i] -= 1;
                    if waiting[i] == 0 {
                        order.push(i);
                        queued[i] = true;
                    }
                }
            }
        }
        let sat_of = |i: usize| ((f64::from(air.temperature[i]) + 12.0) / 38.0).clamp(0.12, 1.0);
        let local_hum = |i: usize| if air.water[i] != 0 { sat_of(i) } else { 0.3 };
        let mut hum: Vec<_> = (0..n).map(|i| local_hum(i) as f32).collect();
        let mut rain = vec![0.0_f32; n];
        let mut heat = vec![0.0_f32; n];
        let mut land_heat = vec![0.0_f32; n];
        let keep_per_unit = round24((-1.0_f64 / (1500.0 / crate::initial_currents::KM)).exp());
        let heat_keep = round24(keep_per_unit.powf(mesh.spacing));
        for _ in 0..if cycles > 0 { 2 } else { 1 } {
            check_cancel(cancel)?;
            for (a, &i) in order.iter().enumerate() {
                if a % 256 == 0 {
                    check_cancel(cancel)?;
                }
                let mut sw = 0.0;
                let mut sh = 0.0;
                let mut se = 0.0;
                let mut sq = 0.0;
                for k in mesh.adj_start[i]..mesh.adj_start[i + 1] {
                    let dot = f64::from(wind.up_w[k]);
                    if dot == 0.0 {
                        continue;
                    }
                    let j = mesh.adj[k];
                    sw += dot;
                    sh += dot * f64::from(hum[j]);
                    se += dot * f64::from(air.smooth_elev[j]);
                    sq += dot * f64::from(heat[j]);
                }
                let is_sea = air.water[i] != 0;
                let sat = sat_of(i);
                let local = local_hum(i);
                let mut hm = if sw > 0.0 {
                    local + (sh / sw - local) * f64::from(wind.strength[i])
                } else {
                    local
                };
                let ue = if sw > 0.0 { se / sw } else { 0.0 };
                if is_sea {
                    hm += (sat - hm) * 0.22;
                    rain[i] = (hm * BASE * 1.2) as f32;
                } else {
                    let rise = (f64::from(air.smooth_elev[i]) - ue).max(0.0);
                    let mut frac = (BASE + 0.00018 * rise).clamp(0.0, 0.6);
                    if air.sst.is_some() {
                        let mut q = if sw > 0.0 {
                            (sq / sw) * f64::from(wind.strength[i]).sqrt()
                        } else {
                            0.0
                        };
                        q += (f64::from(air.coast_sst.expect("sst has coastal heat")[i]) - q)
                            * f64::from(air.coast_w.expect("sst has coastal weight")[i]);
                        frac *= if q < 0.0 {
                            (1.0 + 0.2 * q).max(0.2)
                        } else {
                            1.0 + 0.04 * q.min(5.0)
                        };
                        heat[i] = (q * heat_keep * round24((-rise / 1500.0).exp())) as f32;
                        land_heat[i] = q as f32;
                    }
                    let r = hm * frac;
                    hm = hm - r + r * 0.7 * sat.clamp(0.3, 1.0);
                    rain[i] = r as f32;
                }
                if let Some(sst) = air.sst {
                    if is_sea {
                        let up = if sw > 0.0 {
                            (sq / sw) * f64::from(wind.strength[i])
                        } else {
                            0.0
                        };
                        heat[i] = (up + (f64::from(sst[i]) - up) * 0.35) as f32;
                    }
                }
                hum[i] = hm as f32;
            }
        }
        Ok(Carried { rain, land_heat })
    }
}
struct Carried {
    rain: Vec<f32>,
    land_heat: Vec<f32>,
}
struct Air<'a> {
    water: &'a [u8],
    temperature: &'a [f32],
    smooth_elev: &'a [f32],
    cuts: &'a [f64],
    sst: Option<&'a [f32]>,
    coast_sst: Option<&'a [f32]>,
    coast_w: Option<&'a [f32]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mesh::{build_mesh, sphere_spacing},
        random::{sub_seed, Mulberry32},
        terrain_reference as r,
    };
    #[test]
    fn initial_proxy_climate_matches_upstream() {
        let p = TerrainParams {
            seed: 42,
            shape: crate::TerrainShape {
                cells: 256,
                ..Default::default()
            },
        };
        let mesh = build_mesh(
            2048.0,
            1024.0,
            sphere_spacing(2048.0, p.cells),
            &mut Mulberry32::new(sub_seed(p.seed, "mesh")),
            &|| false,
        )
        .expect("mesh");
        let proxy: Vec<_> = (0..mesh.n)
            .map(|i| {
                if r::TECT_LAND[i] != 0 {
                    (f64::from(f32::from_bits(r::TECT_UPLIFT[i])) * 2500.0
                        + f64::from(f32::from_bits(r::TECT_PLATEAU[i]))) as f32
                } else {
                    f32::from_bits(r::TECT_OCEAN_DEPTH[i])
                }
            })
            .collect();
        let water: Vec<_> = r::TECT_LAND.iter().map(|&v| u8::from(v == 0)).collect();
        let geo = SphereGeometry::new(&mesh);
        let climate = InitialClimateModel::new(&mesh, &geo, &p, &|| false)
            .expect("climate cache")
            .compute(
                ClimateInput {
                    elevation: &proxy,
                    water: &water,
                    sst: None,
                    seasons: false,
                },
                &|| false,
            )
            .expect("climate");
        assert!(climate
            .precipitation
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0));
        if !crate::geometry_reference::trig_matches() {
            eprintln!("host trig differs; skip exact climate fixture");
            return;
        }
        for (got, want) in [
            (&climate.temperature, r::INITIAL_CLIMATE_0_TEMPERATURE),
            (&climate.precipitation, r::INITIAL_CLIMATE_0_PRECIPITATION),
            (&climate.wind_x, r::INITIAL_CLIMATE_0_WIND_X),
            (&climate.wind_y, r::INITIAL_CLIMATE_0_WIND_Y),
        ] {
            assert_eq!(got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), want);
        }
    }
}
