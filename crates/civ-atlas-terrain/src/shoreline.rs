// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas raster.ts shadeSphere, retaining every elevation/water consumer.
use crate::{
    gully::Gullies,
    noise::tileable_fbm,
    random::sub_seed,
    raster::{self, Coverage, RasterBase, HEIGHT, WIDTH},
    util::{check_cancel, smoothstep},
    valleys::Valleys,
    TerrainError,
};
#[derive(Debug)]
pub(crate) struct Detail {
    tile: Vec<f32>,
    seed: u32,
    spacing: f64,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct Raw {
    pub height: f32,
    pub water: u8,
    pub ice: f32,
}
struct TriNoise<'a> {
    tile: &'a [f32],
    p: [f64; 3],
    weights: [f64; 3],
}
impl TriNoise<'_> {
    fn at(&self, f: f64, o: f64) -> f64 {
        let c = 0.61_f64.cos();
        let s = 0.61_f64.sin();
        let mut out = 0.;
        for p in 0..3 {
            let wp = self.weights[p];
            if wp <= 0. {
                continue;
            }
            let u = self.p[(p + 1) % 3] * f;
            let v = self.p[(p + 2) % 3] * f;
            let (su, sv) = match p {
                0 => (c * u - s * v + o, s * u + c * v + 0.37 * o + 17.9),
                1 => (
                    c * u - s * v + 1.31 * o + 131.3,
                    s * u + c * v + 0.73 * o + 7.7,
                ),
                _ => (
                    c * u - s * v + 0.59 * o + 61.1,
                    s * u + c * v + 1.13 * o + 233.9,
                ),
            };
            let ui = su.floor();
            let vi = sv.floor();
            let tx = su - ui;
            let ty = sv - vi;
            let x0 = (ui as isize & 511) as usize;
            let x1 = ((ui as isize + 1) & 511) as usize;
            let r0 = (vi as isize & 511) as usize * 512;
            let r1 = ((vi as isize + 1) & 511) as usize * 512;
            let a = self.tile[r0 + x0] as f64;
            let b = self.tile[r0 + x1] as f64;
            let c = self.tile[r1 + x0] as f64;
            let d = self.tile[r1 + x1] as f64;
            let top = a + (b - a) * tx;
            out += wp * (top + (c + (d - c) * tx - top) * ty);
        }
        out
    }
}
impl Detail {
    pub(crate) fn new(seed: u32, spacing: f64) -> Self {
        let mut tile = tileable_fbm(sub_seed(seed, "detail"), 512, 36, 4, 0.55);
        for v in &mut tile {
            *v = (*v as f64 * 0.25) as f32;
        }
        Self {
            tile,
            seed,
            spacing,
        }
    }
    pub(crate) fn workspace(&self) -> Gullies {
        Gullies::new(sub_seed(self.seed, "gully"))
    }
    pub(crate) fn sample(
        &self,
        source: &SampleSource<'_>,
        xy: [f64; 2],
        gullies: &mut Gullies,
    ) -> Result<Raw, TerrainError> {
        let SampleSource {
            base,
            cover,
            valleys,
            scale,
            zoom,
        } = *source;
        let f = raster::frame([xy[0], xy[1].clamp(0.5 / scale, HEIGHT as f64 - 0.5 / scale)]);
        let [clon, slon, cl, sl] = f;
        let q = raster::direction(f);
        let (t, w) = cover.locate(xy, q).ok_or(TerrainError::InvalidOutput {
            field: "fine coverage",
        })?;
        let [lake, amp, cw, rug0, ice] = base.fields(t, w);
        let r = WIDTH as f64 / std::f64::consts::TAU;
        let p = q.map(|v| v * r);
        let mut weights = q.map(|v| {
            let a = v.abs() - 0.45;
            if a > 0. {
                a * a
            } else {
                0.
            }
        });
        let nrm = 1.
            / (weights[0] * weights[0] + weights[1] * weights[1] + weights[2] * weights[2]).sqrt();
        for w in &mut weights {
            *w *= nrm;
        }
        let noise = TriNoise {
            tile: &self.tile,
            p,
            weights,
        };
        let e = if zoom {
            raster::spline(&base.elevation, [xy[0] - 0.5, xy[1] - 0.5]) as f32 as f64
        } else {
            base.elevation[(xy[1].floor() as usize) * WIDTH + xy[0].floor() as usize] as f64
        };
        let [cv, calm] = valleys.sample(xy).map(f64::from);
        let fine2 = zoom && scale >= 2.;
        let fine4 = zoom && scale >= 4.;
        let mut d = noise.at(1., 0.);
        if fine2 {
            d += 0.45 * noise.at(2., 19.7);
        }
        if fine4 {
            d += 0.2 * noise.at(4., 27.1);
        }
        let is_lake = lake + 0.12 * d > 0.5;
        let mut ee = e;
        if !is_lake {
            let calm_q = 1. - 0.85 * smoothstep(0.65, 1., calm);
            let mut eb = e;
            let mut rug = 0.;
            let stretch = 1. / cl.max(0.05);
            if cw > 0.01 {
                rug = rug0;
                let coarse = ((self.spacing - 2.1) / 2.85).clamp(0., 2.);
                let flat = coarse * (1. - rug);
                let sd = scale * smoothstep(0.01, 0.5, cw) / 0.25;
                let disp = (1.2 + 3.5 * rug) * sd;
                let mut wx = noise.at(1., 211.3);
                let mut wy = noise.at(1., 53.9);
                if flat > 0.02 {
                    let m = 5. * flat / (1.2 + 3.5 * rug);
                    wx += m * (noise.at(0.25, 401.9) + 0.5 * noise.at(0.5, 433.7));
                    wy += m * (noise.at(0.25, 277.3) + 0.5 * noise.at(0.5, 129.1));
                }
                if fine2 {
                    wx += 0.25 * noise.at(2., 311.3);
                    wy += 0.25 * noise.at(2., 83.9);
                }
                if rug > 0.02 {
                    wx += 1.3 * rug * noise.at(1. / 3., 101.7);
                    wy += 1.3 * rug * noise.at(1. / 3., 157.1);
                }
                let pos = [
                    (xy[0] * scale + disp * wx * stretch) / scale - 0.5,
                    (xy[1] * scale + disp * wy) / scale - 0.5,
                ];
                eb = if zoom {
                    raster::spline(&base.elevation, pos)
                } else {
                    raster::bilinear(&base.elevation, pos)
                };
            }
            let mut dd = d;
            if amp > 60. && e > 0. {
                dd += 0.5 * smoothstep(60., 250., amp) * noise.at(2.37, 33.1);
            }
            ee = eb + amp * calm_q * dd;
            if cw == 0. && ee < 2. {
                ee = 2.;
            }
            if cv > 0. && ee > 0. {
                ee = (ee - cv * (1. + amp / 120.)).max(ee.min(1.));
            }
            if zoom && (amp > 15. || cw > 0.01) && eb > -120. {
                let dx = 1. / scale;
                let neighbors = [
                    [xy[0] + dx, xy[1]],
                    [xy[0] - dx, xy[1]],
                    [xy[0], xy[1] - dx],
                    [xy[0], xy[1] + dx],
                ];
                let nb = neighbors.map(|p| {
                    raster::spline(&base.elevation, [p[0] - 0.5, p[1] - 0.5]) as f32 as f64
                });
                let cuts = neighbors.map(|p| valleys.sample(p)[0] as f64);
                let mut ge = (nb[0] - nb[1]) * 0.5 * scale * stretch;
                let mut gn = (nb[2] - nb[3]) * 0.5 * scale;
                if cv > 0. || cuts[0] > 0. || cuts[1] > 0. {
                    let kc = 2. * (1. + amp / 120.);
                    ge -= (cuts[0] - cuts[1]) * 0.5 * scale * stretch * kc;
                    gn -= (cuts[2] - cuts[3]) * 0.5 * scale * kc;
                }
                let st = smoothstep(2., 30., (ge * ge + gn * gn).sqrt());
                let a = 1.1
                    * calm_q
                    * (amp * (0.3 + (1. - 0.3) * st)
                        + if rug > 0. {
                            160. * rug * smoothstep(0.01, 0.5, cw)
                        } else {
                            0.
                        });
                if a > 5. {
                    let gb = eb + amp * calm_q * dd * (1. - 0.6 * st);
                    ee = gb
                        + gullies.height(
                            [
                                p[0],
                                p[1],
                                p[2],
                                -slon,
                                clon,
                                -sl * clon,
                                -sl * slon,
                                cl,
                                ge,
                                gn,
                                a,
                            ],
                            scale,
                        );
                    if cw == 0. && ee < 2. {
                        ee = 2.;
                    }
                    if cv > 0. && ee > 0. {
                        ee = (ee - cv * (1. + amp / 120.)).max(ee.min(1.));
                    }
                }
            }
        }
        let water = if is_lake {
            2
        } else if ee < 0. {
            1
        } else {
            0
        };
        Ok(Raw {
            height: ee as f32,
            water,
            ice: if water == 1 && ice > 0.002 {
                ice as f32
            } else {
                0.
            },
        })
    }
}
pub(crate) struct SampleSource<'a> {
    pub base: &'a RasterBase,
    pub cover: &'a Coverage,
    pub valleys: &'a Valleys,
    pub scale: f64,
    pub zoom: bool,
}
pub(crate) fn whole(
    source: &SampleSource<'_>,
    detail: &Detail,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Raw>, TerrainError> {
    let mut out = Vec::with_capacity(WIDTH * HEIGHT);
    let mut g = detail.workspace();
    for y in 0..HEIGHT {
        check_cancel(cancel)?;
        for x in 0..WIDTH {
            out.push(detail.sample(source, [x as f64 + 0.5, y as f64 + 0.5], &mut g)?);
        }
    }
    let mut seen = vec![false; out.len()];
    let mut stack = Vec::new();
    let mut comp = Vec::new();
    for k0 in 0..out.len() {
        if k0 % WIDTH == 0 {
            check_cancel(cancel)?;
        }
        if out[k0].water != 0 || seen[k0] {
            continue;
        }
        comp.clear();
        stack.push(k0);
        seen[k0] = true;
        let mut shut = false;
        while let Some(k) = stack.pop() {
            comp.push(k);
            let x = k % WIDTH;
            let y = k / WIDTH;
            for d in 0..4 {
                if (d == 2 && y == 0) || (d == 3 && y == HEIGHT - 1) {
                    shut = true;
                    continue;
                }
                let q = match d {
                    0 => k - x + (x + WIDTH - 1) % WIDTH,
                    1 => k - x + (x + 1) % WIDTH,
                    2 => k - WIDTH,
                    _ => k + WIDTH,
                };
                if out[q].water == 0 {
                    if !seen[q] {
                        seen[q] = true;
                        stack.push(q);
                    }
                } else if out[q].water != 1 || out[q].ice > 0. {
                    shut = true;
                }
            }
        }
        if shut || comp.len() > 4 {
            continue;
        }
        for &k in &comp {
            out[k].height = -2.;
            out[k].water = 1;
        }
    }
    Ok(out)
}
