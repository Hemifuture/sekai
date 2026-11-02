// SPDX-License-Identifier: AGPL-3.0-only
// Fixed-scale source zoom evaluator and Sekai specification amendment A3 lake anchors.
use crate::{
    creeks,
    estuary::{self, Mouth},
    gully::Gullies,
    raster::{Coverage, RasterBase, HEIGHT, WIDTH},
    river_geometry::{self, DenseRiver},
    shoreline::{self, Detail, Raw, SampleSource},
    util::check_cancel,
    valleys::Valleys,
    Terrain, TerrainError,
};
use std::{
    f64::consts::{PI, TAU},
    sync::Arc,
};
/// Version of the canonical source evaluator and A3 lake-bed adaptation.
pub const FINE_TERRAIN_VERSION: u32 = 1;
/// Supported original source zoom scales. Each terrain fixes one scale for its lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FineScale {
    /// Two source pixels per world unit.
    Two,
    /// Four source pixels per world unit.
    Four,
    /// Eight source pixels per world unit.
    Eight,
    /// Sixteen source pixels per world unit.
    Sixteen,
}
impl FineScale {
    /// Return original source pixels per world unit; this is not a physical accuracy claim.
    pub fn pixels_per_unit(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }
}
/// An original lake-node initialization constraint, before spill and merged-basin closure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LakePrior {
    /// Source mesh site, also the stable prior identifier within this terrain.
    pub site: usize,
    /// Original unit-sphere direction (source f32 coordinates promoted to f64).
    pub direction: [f64; 3],
    /// Original bed restored at this node, in metres.
    pub bed_elevation_m: f64,
    /// Original constant lake-level prior, in metres; not a closed final water surface.
    pub surface_elevation_m: f64,
}
/// Canonical point bed. Water classification and depth require initial water closure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSample {
    /// Bed height in metres, including the A3 nodal residual.
    pub bed_elevation_m: f64,
}
#[derive(Debug)]
struct FineSource {
    terrain: Arc<Terrain>,
    cover: Coverage,
    base: RasterBase,
    detail: Detail,
    mouths: Vec<Mouth>,
}
impl FineSource {
    fn new(
        terrain: Arc<Terrain>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(Self, Vec<DenseRiver>), TerrainError> {
        check_cancel(cancel)?;
        let cover = Coverage::new(&terrain.mesh, cancel)?;
        let base = RasterBase::new(&terrain, &cover, cancel)?;
        let detail = Detail::new(terrain.params.seed, terrain.mesh.spacing);
        check_cancel(cancel)?;
        let lines = river_geometry::river_geometry(
            &terrain.initial_rivers,
            terrain.river_threshold,
            WIDTH as f64,
        );
        let valleys = Valleys::new(
            &lines,
            terrain.river_threshold,
            1.,
            [WIDTH as f64, HEIGHT as f64],
        );
        let whole = shoreline::whole(
            &SampleSource {
                base: &base,
                cover: &cover,
                valleys: &valleys,
                scale: 1.,
                zoom: false,
            },
            &detail,
            cancel,
        )?;
        let mut rivers = terrain.initial_rivers.clone();
        rivers.extend(creeks::creeks(&terrain, cancel)?);
        let lines = river_geometry::river_geometry(&rivers, terrain.river_threshold, WIDTH as f64);
        check_cancel(cancel)?;
        let mouths = estuary::mouths(&terrain, &lines, &whole, &base.elevation, cancel)?;
        Ok((
            Self {
                terrain,
                cover,
                base,
                detail,
                mouths,
            },
            lines,
        ))
    }
}
/// Immutable canonical terrain; thread-local sampling scratch never changes its identity.
/// This is an author initial bed, not the final water, climate or current river artifact.
#[derive(Debug)]
pub struct FineTerrain {
    source: Arc<FineSource>,
    scale: FineScale,
    valleys: Valleys,
    residual: Vec<f64>,
    priors: Vec<LakePrior>,
}
impl FineTerrain {
    /// Borrow the original immutable coarse author data for substrate and spatial adaptation.
    pub fn coarse_terrain(&self) -> &Terrain {
        &self.source.terrain
    }
    /// Build the original fine terrain at one fixed source scale.
    ///
    /// `terrain` supplies the complete coarse author state; `scale` is fixed independently
    /// of any camera. `cancel` interrupts initialization. Returns a finite bed evaluator
    /// and unclosed lake priors, or `Cancelled`/`InvalidOutput` on failure.
    pub fn new(
        terrain: Arc<Terrain>,
        scale: FineScale,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, TerrainError> {
        let (source, lines) = FineSource::new(terrain, cancel)?;
        Self::from_source(Arc::new(source), &lines, scale, cancel)
    }
    fn from_source(
        source: Arc<FineSource>,
        lines: &[DenseRiver],
        scale: FineScale,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, TerrainError> {
        check_cancel(cancel)?;
        let valleys = Valleys::new(
            lines,
            source.terrain.river_threshold,
            scale.pixels_per_unit() as f64,
            [WIDTH as f64, HEIGHT as f64],
        );
        let mut out = Self {
            residual: vec![0.; source.terrain.mesh.n],
            source,
            scale,
            valleys,
            priors: Vec::new(),
        };
        let mut scratch = out.source.detail.workspace();
        for i in 0..out.source.terrain.mesh.n {
            check_cancel(cancel)?;
            if out.source.terrain.water[i] != 2 {
                continue;
            }
            let world = &out.source.terrain;
            let direction = std::array::from_fn(|j| world.mesh.xyz[3 * i + j] as f64);
            let bed = world.elevation[i] as f64;
            let level = world.water_level[i] as f64;
            let xy = coordinates(direction)?;
            out.residual[i] = bed - out.raw(xy, &mut scratch)?.height as f64;
            out.priors.push(LakePrior {
                site: i,
                direction,
                bed_elevation_m: bed,
                surface_elevation_m: level,
            });
        }
        Ok(out)
    }
    /// Return the immutable canonical sampling scale.
    pub fn scale(&self) -> FineScale {
        self.scale
    }
    /// Return original lake constraints. Consumers must resolve connectivity, spill and
    /// overlapping basins before integrating or publishing physical water.
    pub fn lake_priors(&self) -> &[LakePrior] {
        &self.priors
    }
    /// Evaluate the fixed bed for any finite nonzero direction.
    ///
    /// Direction magnitude is ignored. Returns `InvalidDirection` for zero/nonfinite
    /// inputs, or `InvalidOutput` if the source cannot locate a valid triangle.
    pub fn sample_surface(&self, direction: [f64; 3]) -> Result<SurfaceSample, TerrainError> {
        self.sample(direction, &mut self.source.detail.workspace())
    }
    /// Evaluate a bounded caller batch, reusing mutable noise scratch locally.
    ///
    /// Preserves input order. Returns the same errors as `sample_surface`, or `Cancelled`;
    /// a failed/cancelled batch returns no partial output and drops its owned allocations.
    pub fn sample_surface_batch(
        &self,
        directions: &[[f64; 3]],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<SurfaceSample>, TerrainError> {
        check_cancel(cancel)?;
        let mut scratch = self.source.detail.workspace();
        let mut out = Vec::with_capacity(directions.len());
        for &d in directions {
            check_cancel(cancel)?;
            out.push(self.sample(d, &mut scratch)?);
        }
        Ok(out)
    }
    fn sample(
        &self,
        direction: [f64; 3],
        scratch: &mut Gullies,
    ) -> Result<SurfaceSample, TerrainError> {
        let xy = coordinates(direction)?;
        let raw = self.raw(xy, scratch)?;
        Ok(SurfaceSample {
            bed_elevation_m: self.restore_bed(xy, direction, raw.height)?,
        })
    }
    fn restore_bed(
        &self,
        xy: [f64; 2],
        direction: [f64; 3],
        raw: f32,
    ) -> Result<f64, TerrainError> {
        let mut bed = raw as f64;
        let max = direction.iter().map(|v| v.abs()).fold(0_f64, f64::max);
        let (t, w) = self
            .source
            .cover
            .locate(xy, direction.map(|v| v / max))
            .ok_or(TerrainError::InvalidOutput {
                field: "lake P1 coverage",
            })?;
        let nodes = self.source.cover.triangles[t].nodes;
        if nodes.iter().any(|&i| self.residual[i] != 0.) {
            for j in 0..3 {
                bed += w[j] * self.residual[nodes[j]];
            }
        }
        if !bed.is_finite() {
            return Err(TerrainError::InvalidOutput { field: "fine bed" });
        }
        Ok(bed)
    }
    fn raw(&self, xy: [f64; 2], scratch: &mut Gullies) -> Result<Raw, TerrainError> {
        let scale = self.scale.pixels_per_unit() as f64;
        let mut raw = self.source.detail.sample(
            &SampleSource {
                base: &self.source.base,
                cover: &self.source.cover,
                valleys: &self.valleys,
                scale,
                zoom: true,
            },
            xy,
            scratch,
        )?;
        for mouth in &self.source.mouths {
            mouth.sample(xy, scale, self.source.terrain.river_threshold, &mut raw);
        }
        Ok(raw)
    }
}
fn coordinates(d: [f64; 3]) -> Result<[f64; 2], TerrainError> {
    if d.iter().any(|v| !v.is_finite()) {
        return Err(TerrainError::InvalidDirection);
    }
    let max = d.iter().map(|v| v.abs()).fold(0_f64, f64::max);
    if max == 0. {
        return Err(TerrainError::InvalidDirection);
    }
    let p = d.map(|v| v / max);
    let norm = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    let lat = (p[2] / norm).clamp(-1., 1.).asin();
    let lon = p[1].atan2(p[0]);
    Ok([
        ((lon + PI) / TAU * WIDTH as f64).rem_euclid(WIDTH as f64),
        (PI / 2. - lat) / PI * HEIGHT as f64,
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[allow(dead_code)]
    mod reference {
        include!("../reference/fine-world.rs");
    }
    fn hash(values: impl IntoIterator<Item = u32>) -> u64 {
        values.into_iter().fold(14695981039346656037, |h, v| {
            (h ^ u64::from(v)).wrapping_mul(1099511628211)
        })
    }
    fn exact_direction(xy: [f64; 2]) -> [f64; 3] {
        let lon = xy[0] / WIDTH as f64 * TAU - PI;
        let lat = PI / 2. - xy[1] / HEIGHT as f64 * PI;
        [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
    }
    #[test]
    fn finite_direction_domain_and_immutable_thread_safety() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<FineTerrain>();
        for d in [[0.; 3], [f64::NAN, 1., 0.], [f64::INFINITY, 0., 0.]] {
            assert_eq!(coordinates(d), Err(TerrainError::InvalidDirection));
        }
        for d in [
            [f64::MAX, 0., 0.],
            [f64::from_bits(1), 0., 0.],
            [0., 0., 1.],
            [0., 0., -1.],
        ] {
            assert!(coordinates(d)
                .expect("finite direction")
                .iter()
                .all(|v| v.is_finite()));
        }
    }
    #[test]
    #[ignore = "original default-size world/window fidelity audit; run serialized Release"]
    fn original_fine_windows() {
        let p = crate::TerrainParams {
            seed: 42,
            ..Default::default()
        };
        let terrain = Arc::new(crate::generate_terrain(&p, &|| false).expect("terrain"));
        let (source, lines) = FineSource::new(terrain, &|| false).expect("fine source");
        let source = Arc::new(source);
        let exact = crate::geometry_reference::trig_matches();
        let main_lines = river_geometry::river_geometry(
            &source.terrain.initial_rivers,
            source.terrain.river_threshold,
            WIDTH as f64,
        );
        let main_valleys = Valleys::new(
            &main_lines,
            source.terrain.river_threshold,
            1.,
            [WIDTH as f64, HEIGHT as f64],
        );
        let whole = shoreline::whole(
            &SampleSource {
                base: &source.base,
                cover: &source.cover,
                valleys: &main_valleys,
                scale: 1.,
                zoom: false,
            },
            &source.detail,
            &|| false,
        )
        .expect("whole");
        if exact {
            assert_eq!(
                hash(source.base.elevation.iter().map(|v| v.to_bits())),
                reference::BASE_HASH,
                "base hash"
            );
            assert_eq!(
                hash(whole.iter().map(|r| r.height.to_bits())),
                reference::WHOLE_ELEVATION_HASH,
                "whole height hash"
            );
            assert_eq!(
                hash(whole.iter().map(|r| u32::from(r.water))),
                reference::WHOLE_WATER_HASH,
                "whole water hash"
            );
            assert_eq!(
                hash(
                    crate::initial_sea_ice::nodes(&source.terrain, &|| false)
                        .expect("ice")
                        .iter()
                        .map(|v| v.to_bits())
                ),
                reference::ICE_NODES_HASH
            );
            assert_eq!(source.mouths.len(), reference::MOUTH_COUNT);
            assert_eq!(
                creeks::creeks(&source.terrain, &|| false)
                    .expect("creeks")
                    .len(),
                reference::CREEK_COUNT
            );
        } else {
            eprintln!("host trig differs: skip exact upstream fine fixtures; domain and lake contracts still run");
        }
        let mut controlled = lines.clone();
        for d in &mut controlled {
            d.f.fill(1000.);
        }
        let controlled_mouths = estuary::mouths(
            &source.terrain,
            &controlled,
            &whole,
            &source.base.elevation,
            &|| false,
        )
        .expect("controlled mouths");
        for scale in [
            FineScale::Two,
            FineScale::Four,
            FineScale::Eight,
            FineScale::Sixteen,
        ] {
            let fine = FineTerrain::from_source(Arc::clone(&source), &lines, scale, &|| false)
                .expect("fixed fine");
            let mut scratch = source.detail.workspace();
            let mut anchor_error = 0_f64;
            for p in fine.lake_priors() {
                let b = fine
                    .sample_surface(p.direction)
                    .expect("lake node")
                    .bed_elevation_m;
                anchor_error = anchor_error.max((b - p.bed_elevation_m).abs());
                assert_eq!(b as f32, p.bed_elevation_m as f32, "lake node {}", p.site);
            }
            eprintln!(
                "lake anchors {scale:?}: count {}, max f64 error {anchor_error}",
                fine.lake_priors().len()
            );
            let mut different = 0;
            let mut water_diff = 0;
            let mut max_error = 0_f64;
            let mut first = None;
            for &(s, x, y, e, w) in reference::WINDOW {
                if s != scale.pixels_per_unit() {
                    continue;
                }
                let raw = fine.raw([x, y], &mut scratch).expect("raw");
                if raw.height.to_bits() != e {
                    different += 1;
                    max_error = max_error.max((raw.height as f64 - f32::from_bits(e) as f64).abs());
                    first.get_or_insert((x, y, raw.height, f32::from_bits(e)));
                }
                if raw.water != w {
                    water_diff += 1;
                }
            }
            eprintln!("scale {scale:?}: height differences {different}, max {max_error}, water {water_diff}, first {first:?}");
            if exact {
                assert_eq!((different, water_diff), (0, 0));
            }
            if scale == FineScale::Four {
                let mut branch_diffs = 0;
                let mut branch_max = 0_f64;
                for &(controlled, x, y, e, w) in reference::ESTUARY_BRANCH {
                    let raw = if controlled {
                        let mut r = source
                            .detail
                            .sample(
                                &SampleSource {
                                    base: &source.base,
                                    cover: &source.cover,
                                    valleys: &fine.valleys,
                                    scale: 4.,
                                    zoom: true,
                                },
                                [x, y],
                                &mut scratch,
                            )
                            .expect("pre mouth");
                        for m in &controlled_mouths {
                            m.sample([x, y], 4., source.terrain.river_threshold, &mut r);
                        }
                        r
                    } else {
                        fine.raw([x, y], &mut scratch).expect("bridge")
                    };
                    if (raw.height.to_bits(), raw.water) != (e, w) {
                        branch_diffs += 1;
                        branch_max =
                            branch_max.max((raw.height as f64 - f32::from_bits(e) as f64).abs());
                    }
                }
                eprintln!(
                    "bridge/delta branch differences {branch_diffs}, max height error {branch_max}"
                );
                if exact {
                    assert_eq!(branch_diffs, 0);
                }
                let dirs = reference::WINDOW
                    .iter()
                    .step_by(137)
                    .map(|&(_, x, y, _, _)| exact_direction([x, y]))
                    .chain(fine.priors.iter().map(|p| p.direction))
                    .chain([[0., 0., 1.], [0., 0., -1.], [-1., 0., 0.], [-1., -0., 0.]])
                    .collect::<Vec<_>>();
                let expected = fine
                    .sample_surface_batch(&dirs, &|| false)
                    .expect("forward");
                let reverse = fine
                    .sample_surface_batch(&dirs.iter().copied().rev().collect::<Vec<_>>(), &|| {
                        false
                    })
                    .expect("reverse");
                assert_eq!(expected, reverse.into_iter().rev().collect::<Vec<_>>());
                let chunks = dirs
                    .chunks(7)
                    .flat_map(|c| fine.sample_surface_batch(c, &|| false).expect("tile"))
                    .collect::<Vec<_>>();
                assert_eq!(expected, chunks);
                let calls = std::cell::Cell::new(0);
                let cancel = || {
                    calls.set(calls.get() + 1);
                    calls.get() == 4
                };
                assert_eq!(
                    fine.sample_surface_batch(&dirs, &cancel),
                    Err(TerrainError::Cancelled)
                );
                assert_eq!(calls.get(), 4);
                let calls = std::cell::Cell::new(0);
                let cancel = || {
                    calls.set(calls.get() + 1);
                    calls.get() == 10
                };
                assert!(matches!(
                    FineTerrain::new(Arc::clone(&source.terrain), scale, &cancel),
                    Err(TerrainError::Cancelled)
                ));
                assert_eq!(calls.get(), 10);
                assert!(matches!(
                    fine.sample_surface([f64::NAN, 0., 0.]),
                    Err(TerrainError::InvalidDirection)
                ));
                let prior = fine.priors[0];
                let mut outer_edges = 0;
                let mut edge_error = 0_f64;
                for tri in &source.cover.triangles {
                    if !tri.nodes.contains(&prior.site) {
                        continue;
                    }
                    let edge = tri
                        .nodes
                        .iter()
                        .copied()
                        .filter(|&i| i != prior.site)
                        .collect::<Vec<_>>();
                    if edge.iter().any(|&i| fine.residual[i] != 0.) {
                        continue;
                    }
                    let direction = std::array::from_fn(|j| {
                        source.terrain.mesh.xyz[3 * edge[0] + j] as f64
                            + source.terrain.mesh.xyz[3 * edge[1] + j] as f64
                    });
                    let bed = fine
                        .sample_surface(direction)
                        .expect("outer shared edge")
                        .bed_elevation_m;
                    let raw = fine
                        .raw(
                            coordinates(direction).expect("edge direction"),
                            &mut scratch,
                        )
                        .expect("raw edge");
                    edge_error = edge_error.max((bed - raw.height as f64).abs());
                    assert_eq!(bed as f32, raw.height);
                    outer_edges += 1;
                }
                assert!(outer_edges > 0);
                eprintln!(
                    "lake patch outer shared edges {outer_edges}, maximum residual {edge_error}"
                );
                for &direction in &dirs {
                    let xy = coordinates(direction).expect("direction");
                    let max = direction.iter().map(|v| v.abs()).fold(0_f64, f64::max);
                    let (t, _) = source
                        .cover
                        .locate(xy, direction.map(|v| v / max))
                        .expect("triangle");
                    if source.cover.triangles[t]
                        .nodes
                        .iter()
                        .all(|&i| fine.residual[i] == 0.)
                    {
                        assert_eq!(
                            fine.sample_surface(direction).expect("bed").bed_elevation_m,
                            fine.raw(xy, &mut scratch).expect("raw").height as f64
                        );
                    }
                }
                let center = coordinates(prior.direction).expect("lake location");
                let mut lake_disagreement = 0;
                let mut raw_lake_high = 0;
                for &(s, x, y, _, _) in reference::WINDOW {
                    if s != 4 || (x - center[0]).abs() > 16. || (y - center[1]).abs() > 16. {
                        continue;
                    }
                    let raw = fine.raw([x, y], &mut scratch).expect("lake raw");
                    let bed = fine
                        .sample_surface(exact_direction([x, y]))
                        .expect("lake bed")
                        .bed_elevation_m;
                    if (raw.water == 2) != (bed < prior.surface_elevation_m) {
                        lake_disagreement += 1;
                    }
                    if raw.water == 2 && bed >= prior.surface_elevation_m {
                        raw_lake_high += 1;
                    }
                }
                eprintln!("lake local prior sublevel/noisy-mask disagreements {lake_disagreement}, noisy lake still above prior {raw_lake_high}; connectivity awaits Task5");
            }
            let mut support_samples = 0;
            let mut ocean_intersections = 0;
            let mut whole_intersections = 0;
            let mut sign_changes = [0_usize; 2];
            let mut affected_bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            let mut affected_sites = std::collections::BTreeMap::<usize, usize>::new();
            let mut changed_raw = [f64::INFINITY, f64::NEG_INFINITY];
            let mut changed_bed = [f64::INFINITY, f64::NEG_INFINITY];
            let mut max_delta = 0_f64;
            for [x0, y0] in source.cover.support_tiles(&fine.residual) {
                let s = usize::from(scale.pixels_per_unit());
                for y in y0 * s..(y0 + 16) * s {
                    for x in x0 * s..(x0 + 16) * s {
                        let direction = exact_direction([
                            (x as f64 + 0.5) / s as f64,
                            (y as f64 + 0.5) / s as f64,
                        ]);
                        let xy = coordinates(direction).expect("canonical direction");
                        let max = direction.iter().map(|v| v.abs()).fold(0_f64, f64::max);
                        let (t, _) = source
                            .cover
                            .locate(xy, direction.map(|v| v / max))
                            .expect("support cover");
                        if !source.cover.triangles[t]
                            .nodes
                            .iter()
                            .any(|&i| fine.residual[i] != 0.)
                        {
                            continue;
                        }
                        support_samples += 1;
                        let raw = fine.raw(xy, &mut scratch).expect("lake star");
                        if raw.water == 1 {
                            ocean_intersections += 1;
                        }
                        if whole[y / s * WIDTH + x / s].water == 1 {
                            whole_intersections += 1;
                        }
                        let bed = fine
                            .restore_bed(xy, direction, raw.height)
                            .expect("canonical bed");
                        max_delta = max_delta.max((bed - raw.height as f64).abs());
                        if (bed < 0.) != (raw.height < 0.) {
                            sign_changes[usize::from(raw.height < 0.)] += 1;
                            affected_bounds[0] = affected_bounds[0].min(xy[0]);
                            affected_bounds[1] = affected_bounds[1].min(xy[1]);
                            affected_bounds[2] = affected_bounds[2].max(xy[0]);
                            affected_bounds[3] = affected_bounds[3].max(xy[1]);
                            for &i in &source.cover.triangles[t].nodes {
                                if fine.residual[i] != 0. {
                                    *affected_sites.entry(i).or_default() += 1;
                                }
                            }
                            changed_raw[0] = changed_raw[0].min(raw.height as f64);
                            changed_raw[1] = changed_raw[1].max(raw.height as f64);
                            changed_bed[0] = changed_bed[0].min(bed);
                            changed_bed[1] = changed_bed[1].max(bed);
                        }
                    }
                }
            }
            eprintln!("lake star {scale:?}: {support_samples} canonical pixels, raw ocean {ocean_intersections}, whole ocean {whole_intersections}");
            eprintln!("A3b {scale:?}: land-to-negative {}, negative-to-land {}, xy bounds {affected_bounds:?}, sites {affected_sites:?}, changed raw {changed_raw:?}, changed bed {changed_bed:?}, max star delta {max_delta}",sign_changes[0],sign_changes[1]);
        }
    }
}
