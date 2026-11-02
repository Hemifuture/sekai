// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas src/gen/world.ts, without editor state or history playback.
use crate::{
    erosion::{accumulate, erode, Drainage, DrainageRouter, ErosionInput},
    geometry::SphereGeometry,
    initial_climate::{ClimateInput, InitialClimate, InitialClimateModel},
    initial_currents::{self, InitialCurrents},
    mesh::{build_mesh, sphere_spacing, Mesh},
    params::TerrainParams,
    random::{keyed, sub_seed, Mulberry32},
    tectonics::{build_tectonics, Tectonics},
    util::{check_cancel, smoothstep},
    TerrainError,
};

pub(crate) const MAP_W: f64 = 2048.0;
pub(crate) const MAP_H: f64 = 1024.0;
pub(crate) const ICE_SHEET_T: f64 = -9.0;
const EROSION_STEPS: usize = 36;

/// Completed author terrain. Initialization weather and rivers remain private authoring inputs.
#[derive(Debug)]
pub struct Terrain {
    pub(crate) params: TerrainParams,
    pub(crate) mesh: Mesh,
    pub(crate) elevation: Vec<f32>,
    pub(crate) water: Vec<u8>,
    pub(crate) water_level: Vec<f32>,
    pub(crate) initial_climate: InitialClimate,
    pub(crate) initial_currents: InitialCurrents,
    pub(crate) initial_flux: Vec<f32>,
    pub(crate) initial_rivers: Vec<River>,
    pub(crate) river_threshold: f64,
    pub(crate) max_elevation: f64,
}
#[derive(Debug)]
pub(crate) struct River {
    pub pts: Vec<f32>,
    pub cells: Vec<usize>,
}
impl Terrain {
    /// Return the validated author controls used to create this terrain.
    pub fn params(&self) -> &TerrainParams {
        &self.params
    }
    /// Return interleaved unit-sphere x/y/z site coordinates, in mesh order.
    pub fn site_xyz(&self) -> &[f32] {
        &self.mesh.xyz
    }
    /// Return bed elevations in metres, including lake beds and negative ocean depths.
    pub fn bed_elevation_m(&self) -> &[f32] {
        &self.elevation
    }
    /// Return water classes: zero land, one ocean, two lake, in mesh order.
    pub fn water_classification(&self) -> &[u8] {
        &self.water
    }
    /// Return lake surfaces in metres. Entries are meaningful only for lake cells.
    pub fn water_surface_elevation_m(&self) -> &[f32] {
        &self.water_level
    }
}

/// Generate a complete terrain from author controls, with cooperative cancellation.
///
/// # Arguments
/// * `params` - Controls in the source UI's supported domain.
/// * `cancel` - Returns true when the caller no longer needs this generation.
///
/// # Returns
/// An atomically complete, finite coarse terrain with ocean/lake classification.
///
/// # Errors
/// Returns an invalid-parameter error before mesh allocation, `Cancelled` during work,
/// or `InvalidMesh` when sampling cannot produce a valid sphere triangulation.
pub fn generate_terrain(
    params: &TerrainParams,
    cancel: &dyn Fn() -> bool,
) -> Result<Terrain, TerrainError> {
    params.validate()?;
    check_cancel(cancel)?;
    let mesh = build_mesh(
        MAP_W,
        MAP_H,
        sphere_spacing(MAP_W, params.cells),
        &mut Mulberry32::new(sub_seed(params.seed, "mesh")),
        cancel,
    )?;
    generate_on_mesh(params, mesh, cancel)
}
fn generate_on_mesh(
    params: &TerrainParams,
    mesh: Mesh,
    cancel: &dyn Fn() -> bool,
) -> Result<Terrain, TerrainError> {
    let n = mesh.n;
    let geo = SphereGeometry::new(&mesh);
    check_cancel(cancel)?;
    let tect = build_tectonics(&mesh, &geo, params, cancel)?;
    let mut h = vec![0.0_f32; n];
    let hn = geo.fbm(sub_seed(params.seed, "h0"), 4, 0.5);
    for (i, height) in h.iter_mut().enumerate() {
        if tect.land[i] != 0 {
            *height = (0.05 + 0.05 * (hn.at_scale(i, 300.0, [0.0, 0.0]) + 1.0)) as f32;
        }
    }
    let water0: Vec<_> = tect.land.iter().map(|&l| u8::from(l == 0)).collect();
    let climate = InitialClimateModel::new(&mesh, &geo, params, cancel)?;
    let rain_weight = |elevation: &[f32]| -> Result<Vec<f32>, TerrainError> {
        let c = climate.compute(
            ClimateInput {
                elevation,
                water: &water0,
                sst: None,
                seasons: false,
            },
            cancel,
        )?;
        Ok((0..n)
            .map(|i| {
                if tect.land[i] != 0 {
                    (f64::from(c.precipitation[i]) / 1200.0).clamp(0.1, 2.5) as f32
                } else {
                    0.0
                }
            })
            .collect())
    };
    let peak = 3600.0 + 1700.0 * params.mountains.clamp(0.0, 2.0);
    let proxy: Vec<_> = (0..n)
        .map(|i| {
            if tect.land[i] != 0 {
                (f64::from(tect.uplift[i]) * 2500.0 + f64::from(tect.plateau[i])) as f32
            } else {
                tect.ocean_depth[i]
            }
        })
        .collect();
    let rain = rain_weight(&proxy)?;
    erode(
        &mesh,
        &geo,
        ErosionInput {
            land: &tect.land,
            h: &mut h,
            uplift: &tect.uplift,
            rain: &rain,
            steps: EROSION_STEPS / 2,
        },
        cancel,
    )?;
    let rain = rain_weight(&to_meters(&h, &tect, peak, &tect.plateau))?;
    erode(
        &mesh,
        &geo,
        ErosionInput {
            land: &tect.land,
            h: &mut h,
            uplift: &tect.uplift,
            rain: &rain,
            steps: EROSION_STEPS / 2,
        },
        cancel,
    )?;
    let router = DrainageRouter::new(&mesh, &geo, &tect.land);
    let mut flow = Drainage::new(n);
    router.route(&h, 1e-5, &mut flow, cancel)?;
    let scale = meter_scale(&h, &tect.land, peak);
    let mut base = tect.plateau.clone();
    for &i in flow.order.iter().rev() {
        let r = flow.receiver[i];
        if r == usize::MAX || tect.land[r] == 0 {
            continue;
        }
        let lim =
            f64::from(base[i]) + (f64::from(flow.filled[i]) - f64::from(flow.filled[r])) * scale;
        if f64::from(base[r]) > lim {
            base[r] = lim as f32;
        }
    }
    let mut elevation = to_meters(&h, &tect, peak, &base);
    let high_land = carve_depressions(
        &mesh,
        &geo,
        DepressionInput {
            tect: &tect,
            seed: params.seed,
        },
        &mut elevation,
        cancel,
    )?;
    let (water, water_level) = lakes(
        &mesh,
        &router,
        LakeInput {
            land: &tect.land,
            high_land,
        },
        &mut elevation,
        cancel,
    )?;
    let initial_currents = initial_currents::compute(&mesh, &geo, &water, cancel)?;
    let initial_climate = climate.compute(
        ClimateInput {
            elevation: &elevation,
            water: &water,
            sst: Some(&initial_currents.sst),
            seasons: true,
        },
        cancel,
    )?;
    router.route(&elevation, 1e-3, &mut flow, cancel)?;
    let runoff: Vec<_> = (0..n)
        .map(|i| {
            if tect.land[i] == 0 {
                return 0.0;
            }
            if f64::from(initial_climate.temperature[i]) < ICE_SHEET_T {
                return 0.004;
            }
            let pet = 250.0 + 22.0 * f64::from(initial_climate.temperature[i]).max(0.0);
            ((f64::from(initial_climate.precipitation[i]) - 0.45 * pet) / 1000.0).max(0.03) as f32
        })
        .collect();
    let mut initial_flux = vec![0.0_f32; n];
    accumulate(&flow, &tect.land, &runoff, &mut initial_flux);
    let river_threshold = 10.0 * (n as f64 / 36000.0);
    let initial_rivers = trace_rivers(
        &mesh,
        &geo,
        RiverInput {
            water: &water,
            receiver: &flow.receiver,
            flux: &initial_flux,
            threshold: river_threshold,
            temperature: &initial_climate.temperature,
        },
        cancel,
    )?;
    let max_elevation = (0..n)
        .filter(|&i| tect.land[i] != 0)
        .fold(0.0_f64, |m, i| m.max(f64::from(elevation[i])));
    check_cancel(cancel)?;
    let terrain = Terrain {
        params: params.clone(),
        mesh,
        elevation,
        water,
        water_level,
        initial_climate,
        initial_currents,
        initial_flux,
        initial_rivers,
        river_threshold,
        max_elevation,
    };
    terrain.validate_output(cancel)?;
    Ok(terrain)
}

impl Terrain {
    fn validate_output(&self, cancel: &dyn Fn() -> bool) -> Result<(), TerrainError> {
        let n = self.mesh.n;
        for (field, values) in [
            ("bed elevation", &self.elevation),
            ("lake surface", &self.water_level),
            ("initial temperature", &self.initial_climate.temperature),
            ("initial precipitation", &self.initial_climate.precipitation),
            ("initial east wind", &self.initial_climate.wind_x),
            ("initial south wind", &self.initial_climate.wind_y),
            ("initial sea heat", &self.initial_currents.sst),
            ("initial east current", &self.initial_currents.u),
            ("initial south current", &self.initial_currents.v),
            ("initial flux", &self.initial_flux),
        ] {
            check_cancel(cancel)?;
            if values.len() != n || values.iter().any(|v| !v.is_finite()) {
                return Err(TerrainError::InvalidOutput { field });
            }
        }
        if self.water.len() != n || self.water.iter().any(|&w| w > 2) {
            return Err(TerrainError::InvalidOutput {
                field: "water classification",
            });
        }
        if !self.max_elevation.is_finite()
            || !self.river_threshold.is_finite()
            || self.river_threshold <= 0.0
        {
            return Err(TerrainError::InvalidOutput {
                field: "initial terrain scale",
            });
        }
        for river in &self.initial_rivers {
            check_cancel(cancel)?;
            if river.pts.len() != 3 * river.cells.len()
                || river.cells.iter().any(|&c| c >= n)
                || river.pts.iter().any(|v| !v.is_finite())
            {
                return Err(TerrainError::InvalidOutput {
                    field: "initial river chain",
                });
            }
        }
        Ok(())
    }
}

/// Source quantile conversion is an authoring rule, only for initial terrain (spec A1).
fn meter_scale(h: &[f32], land: &[u8], peak: f64) -> f64 {
    let mut vals: Vec<_> = h
        .iter()
        .zip(land)
        .filter_map(|(&v, &land)| (land != 0).then_some(v))
        .collect();
    vals.sort_by(f32::total_cmp);
    let reference = if vals.is_empty() {
        1.0
    } else {
        let r = f64::from(vals[((vals.len() as f64 * 0.997).floor() as usize).min(vals.len() - 1)]);
        if r == 0.0 {
            1.0
        } else {
            r
        }
    };
    peak / reference
}
fn to_meters(h: &[f32], tect: &Tectonics, peak: f64, base: &[f32]) -> Vec<f32> {
    let scale = meter_scale(h, &tect.land, peak);
    let knee = peak * 0.85;
    (0..h.len())
        .map(|i| {
            if tect.land[i] == 0 {
                tect.ocean_depth[i]
            } else {
                let e = f64::from(h[i]) * scale + f64::from(base[i]);
                if e > knee {
                    (knee + (e - knee) * 0.45) as f32
                } else {
                    e as f32
                }
            }
        })
        .collect()
}
struct DepressionInput<'a> {
    tect: &'a Tectonics,
    seed: u32,
}
fn carve_depressions(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    input: DepressionInput<'_>,
    elevation: &mut [f32],
    cancel: &dyn Fn() -> bool,
) -> Result<usize, TerrainError> {
    let DepressionInput { tect, seed } = input;
    for (i, e) in elevation.iter_mut().enumerate() {
        if tect.land[i] == 0 {
            continue;
        }
        let cont_f = if tect.plate_continental[tect.plate[i]] != 0 {
            1.0
        } else {
            0.5
        };
        let rift = 220.0
            * cont_f
            * smoothstep(0.2, 0.65, f64::from(tect.strength_div[i]))
            * (-(f64::from(tect.dist_div[i]) / 20.0).powi(2)).exp();
        *e = (f64::from(*e) - rift.min(0.7 * f64::from(*e).max(0.0))) as f32;
    }
    let dent_base = sub_seed(seed, "lakes-at");
    let mut high_land = 0;
    let mut dents = Vec::new();
    for (c, &e) in elevation.iter().enumerate() {
        if tect.land[c] == 0 || e <= 60.0 {
            continue;
        }
        high_land += 1;
        if keyed(dent_base, c as i32, 0, 0) < 1.0 / 600.0 {
            dents.push(c);
        }
    }
    for c in dents {
        check_cancel(cancel)?;
        let r = mesh.spacing * (1.6 + keyed(dent_base, c as i32, 1, 0) * 2.8);
        let depth = 40.0 + keyed(dent_base, c as i32, 2, 0) * 160.0;
        let r3 = r * 3.0;
        for i in geo.cells_near(c, r3) {
            if tect.land[i] == 0 {
                continue;
            }
            elevation[i] =
                (f64::from(elevation[i]) - depth * (-geo.near2(i, c, r3) / (r * r)).exp()) as f32;
        }
    }
    Ok(high_land)
}

struct LakeInput<'a> {
    land: &'a [u8],
    high_land: usize,
}
fn lakes(
    mesh: &Mesh,
    router: &DrainageRouter<'_>,
    input: LakeInput<'_>,
    elevation: &mut [f32],
    cancel: &dyn Fn() -> bool,
) -> Result<(Vec<u8>, Vec<f32>), TerrainError> {
    let LakeInput { land, high_land } = input;
    let n = mesh.n;
    let mut pool = Drainage::new(n);
    router.route(elevation, 0.0, &mut pool, cancel)?;
    let mut lowered = false;
    for i in 0..n {
        if land[i] != 0 && pool.filled[i] == 0.0 && elevation[i] < 0.0 {
            elevation[i] = 0.0;
            lowered = true;
        }
    }
    if lowered {
        router.route(elevation, 0.0, &mut pool, cancel)?;
    }
    let mut water = vec![0_u8; n];
    let mut water_level = vec![0.0_f32; n];
    for i in 0..n {
        if land[i] == 0 {
            water[i] = 1;
        } else if f64::from(pool.filled[i]) - f64::from(elevation[i]) > 3.0 {
            water[i] = 2;
            water_level[i] = pool.filled[i];
        }
    }
    let max_lake = 40.max((high_land as f64 * 0.004 + 0.5).floor() as usize);
    let mut seen = vec![false; n];
    for i in 0..n {
        if water[i] != 2 || seen[i] {
            continue;
        }
        check_cancel(cancel)?;
        let mut comp = vec![i];
        seen[i] = true;
        let mut q = 0;
        while q < comp.len() {
            let c = comp[q];
            q += 1;
            for &j in &mesh.adj[mesh.adj_start[c]..mesh.adj_start[c + 1]] {
                if water[j] == 2 && !seen[j] {
                    seen[j] = true;
                    comp.push(j);
                }
            }
        }
        if comp.len() > max_lake {
            for c in comp {
                water[c] = 0;
                elevation[c] = (f64::from(pool.filled[c]) - 1.0) as f32;
            }
        }
    }
    seen.fill(false);
    let islet = 1.max(((6.0 * n as f64) / 36000.0 + 0.5).floor() as usize);
    for i in 0..n {
        if water[i] != 0 || seen[i] {
            continue;
        }
        check_cancel(cancel)?;
        let mut comp = vec![i];
        seen[i] = true;
        let mut lake = false;
        let mut ocean = false;
        let mut level = f32::NEG_INFINITY;
        let mut q = 0;
        while q < comp.len() {
            let c = comp[q];
            q += 1;
            for &j in &mesh.adj[mesh.adj_start[c]..mesh.adj_start[c + 1]] {
                if water[j] == 0 && !seen[j] {
                    seen[j] = true;
                    comp.push(j);
                } else if water[j] == 2 {
                    lake = true;
                    level = level.max(water_level[j]);
                } else if water[j] == 1 {
                    ocean = true;
                }
            }
        }
        if lake && !ocean && comp.len() <= islet {
            for c in comp {
                water[c] = 2;
                water_level[c] = level;
                elevation[c] = f64::from(elevation[c]).min(f64::from(level) - 4.0) as f32;
            }
        }
    }
    Ok((water, water_level))
}

struct RiverInput<'a> {
    water: &'a [u8],
    receiver: &'a [usize],
    flux: &'a [f32],
    threshold: f64,
    temperature: &'a [f32],
}
fn trace_rivers(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    input: RiverInput<'_>,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<River>, TerrainError> {
    let RiverInput {
        water,
        receiver,
        flux,
        threshold,
        temperature,
    } = input;
    let n = mesh.n;
    let is_river: Vec<_> = (0..n)
        .map(|i| {
            water[i] == 0
                && f64::from(flux[i]) >= threshold
                && f64::from(temperature[i]) >= ICE_SHEET_T
        })
        .collect();
    let mut donor = vec![false; n];
    let mut lake_inlet = vec![usize::MAX; n];
    for i in 0..n {
        let r = receiver[i];
        if r == usize::MAX {
            continue;
        }
        if is_river[i] {
            donor[r] = true;
        } else if water[i] == 2 && is_river[r] {
            lake_inlet[r] = i;
        }
    }
    let mut sources: Vec<_> = (0..n).filter(|&i| is_river[i] && !donor[i]).collect();
    sources.sort_by(|&a, &b| flux[b].total_cmp(&flux[a]));
    let mut visited = vec![false; n];
    let mut rivers = Vec::new();
    for s in sources {
        check_cancel(cancel)?;
        let mut pts = Vec::new();
        let mut cells = Vec::new();
        let li = lake_inlet[s];
        if li != usize::MAX {
            let m = geo.mid(li, s);
            pts.extend([m[0] as f32, m[1] as f32, flux[s]]);
            cells.push(li);
        }
        let mut i = s;
        visited[i] = true;
        pts.extend([mesh.x[i], mesh.y[i], flux[i]]);
        cells.push(i);
        loop {
            let r = receiver[i];
            if r == usize::MAX {
                break;
            }
            if water[r] != 0 {
                let m = geo.mid(i, r);
                pts.extend([m[0] as f32, m[1] as f32, flux[i]]);
                cells.push(r);
                break;
            }
            if f64::from(temperature[r]) < ICE_SHEET_T {
                break;
            }
            pts.extend([mesh.x[r], mesh.y[r], flux[r]]);
            cells.push(r);
            if visited[r] {
                break;
            }
            visited[r] = true;
            i = r;
        }
        if pts.len() >= 6 {
            rivers.push(River { pts, cells });
        }
    }
    Ok(rivers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain_reference as r;
    #[test]
    fn default_world_terrain_matches_upstream() {
        let params = TerrainParams {
            seed: 42,
            cells: 256,
            ..TerrainParams::default()
        };
        let mesh = build_mesh(
            MAP_W,
            MAP_H,
            sphere_spacing(MAP_W, params.cells),
            &mut Mulberry32::new(sub_seed(params.seed, "mesh")),
            &|| false,
        )
        .expect("mesh");
        let terrain = generate_on_mesh(&params, mesh, &|| false).expect("terrain");
        assert!(terrain.elevation.iter().all(|v| v.is_finite()));
        if !crate::geometry_reference::trig_matches() {
            eprintln!("host trig differs; skip exact terrain fixture");
            return;
        }
        assert_eq!(
            terrain
                .elevation
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            r::ELEVATION
        );
        assert_eq!(
            terrain
                .water
                .iter()
                .map(|&v| usize::from(v))
                .collect::<Vec<_>>(),
            r::WATER
        );
        assert_eq!(
            terrain
                .water_level
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            r::WATER_LEVEL
        );
        for (name, got, want) in [
            (
                "temperature",
                &terrain.initial_climate.temperature,
                r::TEMPERATURE,
            ),
            (
                "precipitation",
                &terrain.initial_climate.precipitation,
                r::PRECIPITATION,
            ),
            ("flux", &terrain.initial_flux, r::FLUX),
        ] {
            assert_eq!(
                got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                want,
                "{name}"
            );
        }
        assert_eq!(terrain.initial_rivers.len(), r::RIVERS.len());
        for (river, &(cells, pts)) in terrain.initial_rivers.iter().zip(r::RIVERS) {
            assert_eq!(river.cells, cells);
            assert_eq!(
                river.pts.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                pts
            );
        }
    }
    #[test]
    fn combined_author_controls_match_upstream() {
        // One combined valid scene covers optional continent, gathering, roughness,
        // hardness and northern-bias branches that default parameters cannot exercise.
        let params = TerrainParams {
            seed: 42,
            cells: 256,
            plates: 24,
            land_fraction: 0.45,
            mountains: 1.4,
            temperature: 5.0,
            rainfall: 1.4,
            continents: 2,
            gather: 0.8,
            rugged: 1.6,
            hardness: 0.6,
            north: 0.75,
        };
        let mesh = build_mesh(
            MAP_W,
            MAP_H,
            sphere_spacing(MAP_W, params.cells),
            &mut Mulberry32::new(sub_seed(params.seed, "mesh")),
            &|| false,
        )
        .expect("mesh");
        let terrain = generate_on_mesh(&params, mesh, &|| false).expect("authored terrain");
        terrain.validate_output(&|| false).expect("finite terrain");
        let lake = terrain
            .water_classification()
            .iter()
            .position(|&w| w == 2)
            .expect("authored scene contains a real lake");
        assert!(terrain.water_surface_elevation_m()[lake] > terrain.bed_elevation_m()[lake]);
        if !crate::geometry_reference::trig_matches() {
            eprintln!("host trig differs; skip exact authored fixture");
            return;
        }
        assert_eq!(
            terrain
                .elevation
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            r::AUTHORED_ELEVATION
        );
        assert_eq!(
            terrain
                .water
                .iter()
                .map(|&v| usize::from(v))
                .collect::<Vec<_>>(),
            r::AUTHORED_WATER
        );
        assert_eq!(
            terrain
                .water_surface_elevation_m()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            r::AUTHORED_WATER_LEVEL
        );
    }
    #[test]
    fn facade_rejects_invalid_controls_before_cancellation_or_allocation() {
        let invalid = TerrainParams {
            cells: 256,
            ..TerrainParams::default()
        };
        assert!(matches!(
            generate_terrain(&invalid, &|| false),
            Err(TerrainError::InvalidParameter { .. })
        ));
        let invalid = TerrainParams {
            rugged: f64::NAN,
            ..TerrainParams::default()
        };
        assert!(matches!(
            generate_terrain(&invalid, &|| false),
            Err(TerrainError::InvalidParameter { .. })
        ));
    }
    #[test]
    fn facade_bounds_and_nonfinite_controls_are_rejected() {
        assert!(TerrainParams {
            continents: 7,
            ..TerrainParams::default()
        }
        .validate()
        .is_err());
        let setters: [fn(&mut TerrainParams, f64); 8] = [
            |p, v| p.land_fraction = v,
            |p, v| p.mountains = v,
            |p, v| p.temperature = v,
            |p, v| p.rainfall = v,
            |p, v| p.gather = v,
            |p, v| p.rugged = v,
            |p, v| p.hardness = v,
            |p, v| p.north = v,
        ];
        for set in setters {
            for value in [
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::MAX,
                -f64::MAX,
            ] {
                let mut p = TerrainParams::default();
                set(&mut p, value);
                assert!(p.validate().is_err());
            }
        }
        for cells in [0, 11999, 200001, usize::MAX] {
            assert!(TerrainParams {
                cells,
                ..TerrainParams::default()
            }
            .validate()
            .is_err());
        }
        for plates in [0, 7, 61, usize::MAX] {
            assert!(TerrainParams {
                plates,
                ..TerrainParams::default()
            }
            .validate()
            .is_err());
        }
        assert!(TerrainParams {
            continents: 9,
            ..TerrainParams::default()
        }
        .validate()
        .is_err());
        assert!(TerrainParams {
            continents: 3,
            plates: 8,
            ..TerrainParams::default()
        }
        .validate()
        .is_err());
        assert!(TerrainParams::default().validate().is_ok());
        assert!(matches!(
            generate_terrain(&TerrainParams::default(), &|| true),
            Err(TerrainError::Cancelled)
        ));
    }
    #[test]
    fn running_generation_observes_cancellation_and_seeded_rebuilds_repeat() {
        use std::cell::Cell;
        let params = TerrainParams {
            seed: 42,
            cells: 256,
            ..TerrainParams::default()
        };
        let mesh = || {
            build_mesh(
                MAP_W,
                MAP_H,
                sphere_spacing(MAP_W, params.cells),
                &mut Mulberry32::new(sub_seed(params.seed, "mesh")),
                &|| false,
            )
            .expect("mesh")
        };
        let calls = Cell::new(0);
        let cancelled = generate_on_mesh(&params, mesh(), &|| {
            calls.set(calls.get() + 1);
            calls.get() >= 12
        });
        assert!(matches!(cancelled, Err(TerrainError::Cancelled)));
        assert_eq!(calls.get(), 12);
        let a = generate_on_mesh(&params, mesh(), &|| false).expect("first");
        let b = generate_on_mesh(&params, mesh(), &|| false).expect("second");
        assert_eq!(a.elevation, b.elevation);
        assert_eq!(a.water, b.water);
        assert_eq!(a.water_level, b.water_level);
    }
}
