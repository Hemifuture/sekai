//! Product-level gates of the T1 v2 hierarchical derivation on the real
//! Draft seed-42 formation bundle (spec `2026-08-20-t1v2-hierarchical-
//! derivation.md` §7 and amendment A4), restored as stage 0 of the T1 v2.2
//! spec (`2026-09-29-t1v2-2-lod-refinement-design.md` §2). The unit tests in
//! `hierarchical_derivation` cover the same invariants on synthetic fields;
//! only this suite measures them on the product every later stage changes.

mod support;

use std::sync::OnceLock;

use rayon::prelude::*;
use sekai::generators::natural::{
    fibonacci_probe, FineShoreline, FormationDerivationInputs, HierarchicalEvaluator,
    LocatedPrimitive,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    NaturalQualityProfile, SurfaceWaterKind, ELEVATION_MAX_M, ELEVATION_MIN_M,
};
use sekai::world::spatial::{audited_float_platform, spherical_triangle_area_unit, UnitVector3};
use sekai::world::{CellId, Meters, RootSeed};
use support::causal_formation::{
    build_causal_formation, causal_formation_fixture, CausalFormationFixture,
};

/// Draft seed-42 probe fingerprint (spec A4 formula with the LOD-1 §3.6
/// version prefix) at `T1_DERIVATION_VERSION` 3: P1 vertex anchors (T1 v2
/// spec amendment A16) and inland average-interpolating centroids (A17).
const EXPECTED_PROBE_FINGERPRINT: &str =
    "94a882af46b9e0ea4c973b5f9ba291a8a096f4dd500413abd12318be94bdb3c9";

fn evaluator() -> HierarchicalEvaluator {
    evaluator_for(causal_formation_fixture())
}

/// The Standard/seed-42 bundle, built once for the ignored LOD-1 probes only
/// (LOD-1 spec §6: mouth gaps and shoreline fractions scale with the cell
/// spacing, so the conclusions are re-checked on one finer tier).
fn standard_fixture() -> &'static CausalFormationFixture {
    static FIXTURE: OnceLock<CausalFormationFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let surface = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Standard,
            Meters::new(6_371_000.0).unwrap(),
            &sekai::engine::BuildCancellation::new(),
        )
        .unwrap()
        .authoritative_surface()
        .clone();
        let artifact = build_causal_formation(&surface, NaturalQualityProfile::Standard, 42);
        CausalFormationFixture { surface, artifact }
    })
}

fn evaluator_for(fixture: &CausalFormationFixture) -> HierarchicalEvaluator {
    let bundle = fixture.artifact.bundle();
    HierarchicalEvaluator::from_formation_product(
        FormationDerivationInputs {
            surface: &fixture.surface,
            compatibility: bundle.tectonics().compatibility(),
            substrate: bundle.substrate(),
            formation: bundle.surface_formation(),
            climate: bundle.climate(),
        },
        // The fixture's root seed; production passes the world's root seed.
        RootSeed::new(42),
    )
    .expect("the published bundle is a valid derivation input")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Spec §7 invariant 4 on the product: every L0 value is the published P5
/// elevation, bit for bit.
#[test]
fn l0_cells_reproduce_the_published_elevation_bit_exactly() {
    let evaluator = evaluator();
    let elevation = causal_formation_fixture()
        .artifact
        .bundle()
        .surface_formation()
        .terrain_fields()
        .current_elevation_m();
    assert_eq!(evaluator.cell_count(), elevation.len());
    for (index, expected) in elevation.iter().enumerate() {
        let value = evaluator.cell_value(CellId::from_raw(index as u32));
        assert_eq!(
            value.elevation_m.to_bits(),
            expected.to_bits(),
            "cell {index}"
        );
    }
}

/// T1 v2.2 spec §3.3.5 / §5.2.2: with land and water decided by the §5.1
/// fine-shoreline rule, the level-6 land fraction stays within one
/// percentage point of L0 over the same 16,384 Fibonacci probes as the unit
/// test. Never widen the threshold.
#[test]
fn deep_land_fraction_stays_within_one_point_of_l0() {
    let evaluator = evaluator();
    let shoreline = evaluator.shoreline();
    let total = 16_384_usize;
    let (mut l0_land, mut deep_land) = (0_u32, 0_u32);
    for index in 0..total {
        let probe = fibonacci_probe(index, total);
        l0_land += u32::from(is_land(&evaluator, shoreline, probe, 0));
        deep_land += u32::from(is_land(&evaluator, shoreline, probe, 6));
    }
    let drift = (f64::from(l0_land) - f64::from(deep_land)).abs() / total as f64;
    let report = format!("drift {drift} (L0 land {l0_land}, level-6 land {deep_land} of {total})");
    eprintln!("fine-shoreline land {report}");
    assert!(drift <= 0.01, "land fraction {report}");
}

fn is_land(
    evaluator: &HierarchicalEvaluator,
    shoreline: &FineShoreline,
    probe: UnitVector3,
    level: u8,
) -> bool {
    displayed_kind(evaluator, shoreline, probe, level) == SurfaceWaterKind::DryLand
}

fn root_cell(evaluator: &HierarchicalEvaluator, direction: UnitVector3) -> CellId {
    match evaluator.locate(direction, 0) {
        LocatedPrimitive::Cell(cell) => cell,
        LocatedPrimitive::Triangle { cell, .. } => cell,
    }
}

/// The water class the terrain mesh paints at `direction` for `level`: the
/// production leaf value classified by the production fine shoreline.
fn displayed_kind(
    evaluator: &HierarchicalEvaluator,
    shoreline: &FineShoreline,
    direction: UnitVector3,
    level: u8,
) -> SurfaceWaterKind {
    shoreline.classify(
        root_cell(evaluator, direction),
        evaluator.sample(direction, level).elevation_m,
    )
}

/// Per cell: whether the cell or one of its edge neighbours is a P5 water
/// cell — the LOD-1 spec §3.2 "near water" partition (a non-empty
/// fine-shoreline candidate row).
fn near_water_cells(fixture: &CausalFormationFixture) -> Vec<bool> {
    let water = fixture
        .artifact
        .bundle()
        .surface_formation()
        .hydrology()
        .surface_water();
    let wet = |index: usize| water.get(index) != Some(SurfaceWaterKind::DryLand);
    let mut near: Vec<bool> = (0..fixture.surface.cells().len()).map(wet).collect();
    for edge in fixture.surface.edges() {
        let [a, b] = edge.cells.map(|cell| cell.raw() as usize);
        if wet(a) || wet(b) {
            near[a] = true;
            near[b] = true;
        }
    }
    near
}

/// Spec A4: the frozen probe fingerprint, checked only where frozen
/// identities are audited (other platforms build different worlds).
#[test]
fn probe_fingerprint_matches_the_frozen_value_on_the_audited_platform() {
    let actual = hex(&evaluator().probe_fingerprint());
    eprintln!("hierarchical probe fingerprint {actual}");
    if audited_float_platform() {
        assert_eq!(actual, EXPECTED_PROBE_FINGERPRINT);
    } else {
        eprintln!("exact identity checks skipped: unaudited float platform");
    }
}

fn quantiles(values: &mut [f64]) -> String {
    if values.is_empty() {
        return "n 0".to_owned();
    }
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
    format!(
        "n {:6}  p05 {:8.2}  p25 {:8.2}  p50 {:8.2}  p75 {:8.2}  p95 {:8.2}  mean {:8.2}",
        values.len(),
        at(0.05),
        at(0.25),
        at(0.5),
        at(0.75),
        at(0.95),
        values.iter().sum::<f64>() / values.len() as f64
    )
}

/// T1 v2.2 spec §5.2.3 and LOD-1 spec §4 I4: the level-6 fine-shoreline
/// wet area per water body over the published P5 member area, and the
/// fine ocean fraction of every ocean-touching cell against P5's published
/// sub-cell `ocean_area_fraction`. Level-6 leaves are weighted equally
/// inside a sector, as in the aggregate probe. Prints numbers; no gate
/// (LOD-1 spec §9 Q2 and Q5 rulings).
#[test]
#[ignore = "release-only offline area probe (T1 v2.2 §5.2.3, LOD-1 §4 I4)"]
fn fine_water_body_area_probe() {
    fine_water_body_area(causal_formation_fixture(), "draft");
}

#[test]
#[ignore = "release-only offline Standard re-check (LOD-1 spec §6)"]
fn fine_water_body_area_probe_standard() {
    fine_water_body_area(standard_fixture(), "standard");
}

fn fine_water_body_area(fixture: &CausalFormationFixture, tier: &str) {
    let evaluator = evaluator_for(fixture);
    let shoreline = evaluator.shoreline();
    let formation = fixture.artifact.bundle().surface_formation();
    let p1_ocean = formation
        .terrain_fields()
        .surface_water_geometry()
        .ocean_area_fraction();
    let hydrology = formation.hydrology();
    let water = hydrology.surface_water();
    let is_ocean = |index: usize| water.get(index) == Some(SurfaceWaterKind::Ocean);
    let mut touches_ocean: Vec<bool> = (0..fixture.surface.cells().len()).map(is_ocean).collect();
    for edge in fixture.surface.edges() {
        let [a, b] = edge.cells.map(|cell| cell.raw() as usize);
        if is_ocean(a) || is_ocean(b) {
            touches_ocean[a] = true;
            touches_ocean[b] = true;
        }
    }
    let mut lake_of = vec![None; fixture.surface.cells().len()];
    for (slot, lake) in hydrology.lakes().iter().enumerate() {
        for cell in lake.cells() {
            lake_of[cell.raw() as usize] = Some(slot);
        }
    }
    // Per cell: (area, fine ocean area, fine lake area), unit-sphere sr.
    let per_cell: Vec<(f64, f64, f64)> = (0..fixture.surface.cells().len())
        .into_par_iter()
        .map(|index| {
            let cell = CellId::from_raw(index as u32);
            let (mut area, mut ocean_area, mut lake_area) = (0.0, 0.0, 0.0);
            for sector in 0..evaluator.sector_count(cell) as u8 {
                let [a, b, c] = evaluator.sector_corners(cell, sector);
                let weight = spherical_triangle_area_unit(a, b, c);
                let (mut ocean, mut lake, mut count) = (0_u32, 0_u32, 0_u32);
                evaluator.for_each_leaf_value(cell, sector, &[], 5, &mut |leaf| {
                    match shoreline.classify(cell, leaf.elevation_m) {
                        SurfaceWaterKind::Ocean => ocean += 1,
                        SurfaceWaterKind::Lake => lake += 1,
                        SurfaceWaterKind::DryLand => {}
                    }
                    count += 1;
                });
                area += weight;
                ocean_area += weight * f64::from(ocean) / f64::from(count);
                lake_area += weight * f64::from(lake) / f64::from(count);
            }
            (area, ocean_area, lake_area)
        })
        .collect();
    let (mut ocean_p5, mut ocean_p1, mut ocean_fine, mut lake_p5, mut lake_fine) =
        (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut per_lake = vec![(0.0_f64, 0.0_f64); hydrology.lakes().len()];
    let mut dry_coast_minus_p1 = Vec::new();
    for (index, &(area, ocean_area, lake_area)) in per_cell.iter().enumerate() {
        let kind = water
            .get(index)
            .expect("validated water covers the surface");
        ocean_fine += ocean_area;
        lake_fine += lake_area;
        if touches_ocean[index] {
            ocean_p1 += area * f64::from(p1_ocean[index]);
            if kind == SurfaceWaterKind::DryLand {
                dry_coast_minus_p1.push(ocean_area / area - f64::from(p1_ocean[index]));
            }
        }
        match kind {
            SurfaceWaterKind::Ocean => ocean_p5 += area,
            SurfaceWaterKind::Lake => lake_p5 += area,
            SurfaceWaterKind::DryLand => {}
        }
        // Attribute fine lake leaves to the root's own lake only; ring
        // leaves of a neighbouring lake are counted in the totals.
        if let Some(slot) = lake_of[index] {
            per_lake[slot].0 += area;
            per_lake[slot].1 += lake_area;
        }
    }
    eprintln!(
        "[fine water {tier}] ocean fine/P5 whole-cell {:.4}; fine/P5 P1 ocean {:.4}",
        ocean_fine / ocean_p5,
        ocean_fine / ocean_p1
    );
    eprintln!(
        "[fine water {tier}] dry ocean-adjacent: fine ocean fraction - P5 ocean_area_fraction | {}",
        quantiles(&mut dry_coast_minus_p1)
    );
    eprintln!(
        "[fine water {tier}] lakes fine/P5 {:.4} (all lake leaves incl. ring)",
        lake_fine / lake_p5
    );
    let mut ratios: Vec<f64> = per_lake.iter().map(|(p5, fine)| fine / p5).collect();
    eprintln!(
        "[fine water {tier}] {} lakes, own-cell wet/P5 | {}",
        ratios.len(),
        quantiles(&mut ratios)
    );
}

/// One P5 dry cell's aggregation record for the aggregate probe.
struct AggregateRow {
    relief: f64,
    near_water: bool,
    river: bool,
    l1_bias: f64,
    l6_bias: f64,
    /// Per L1 face: metres outside the cell-and-neighbour range (0 inside).
    excess: Vec<f64>,
    at_bounds: u32,
}

/// T1 v2.2 spec §3.4 and LOD-1 spec §1.2 / §4 I3: per P5 dry cell, the
/// area-weighted mean of its L1 sector faces and of its level-6 leaves
/// minus the cell's P5 value, in metres, by local relief tercile (range of
/// the cell and its edge neighbours) and by the LOD-1 §3.2 inland /
/// near-water partition, plus the L1 faces that leave that range or reach
/// the elevation bounds (T1 v2.2 §9.2 overshoot count). Inland river-free
/// cells also report the largest |L1 mean − z| (the I3 residual).
/// Level-6 leaves are weighted equally inside a sector (the four-way split
/// is only approximately equal-area, T1 v2.2 §1.2). Prints numbers; no gate.
#[test]
#[ignore = "release-only offline aggregate probe (T1 v2.2 §3.4, LOD-1 §4 I3)"]
fn aggregate_bias_probe() {
    aggregate_bias(causal_formation_fixture(), "draft");
}

#[test]
#[ignore = "release-only offline Standard re-check (LOD-1 spec §6)"]
fn aggregate_bias_probe_standard() {
    aggregate_bias(standard_fixture(), "standard");
}

fn aggregate_bias(fixture: &CausalFormationFixture, tier: &str) {
    let evaluator = evaluator_for(fixture);
    let formation = fixture.artifact.bundle().surface_formation();
    let elevation = formation.terrain_fields().current_elevation_m();
    let water = formation.hydrology().surface_water();
    let near_water = near_water_cells(fixture);
    let mut river_cell = vec![false; elevation.len()];
    for segment in formation.hydrology().river_segments() {
        river_cell[segment.from().raw() as usize] = true;
        river_cell[segment.to().raw() as usize] = true;
    }
    let mut range: Vec<(f32, f32)> = elevation.iter().map(|&z| (z, z)).collect();
    for edge in fixture.surface.edges() {
        let [a, b] = edge.cells.map(|cell| cell.raw() as usize);
        for (cell, other) in [(a, b), (b, a)] {
            range[cell].0 = range[cell].0.min(elevation[other]);
            range[cell].1 = range[cell].1.max(elevation[other]);
        }
    }
    let dry: Vec<usize> = (0..elevation.len())
        .filter(|&index| water.get(index) == Some(SurfaceWaterKind::DryLand))
        .collect();
    let mut rows: Vec<AggregateRow> = dry
        .par_iter()
        .map(|&index| {
            let cell = CellId::from_raw(index as u32);
            let (mut area, mut l1, mut l6) = (0.0_f64, 0.0_f64, 0.0_f64);
            let mut excess = Vec::new();
            let mut at_bounds = 0;
            for sector in 0..evaluator.sector_count(cell) as u8 {
                let [a, b, c] = evaluator.sector_corners(cell, sector);
                let weight = spherical_triangle_area_unit(a, b, c);
                let (mut sum, mut count) = (0.0_f64, 0_u32);
                evaluator.for_each_leaf_value(cell, sector, &[], 5, &mut |leaf| {
                    sum += f64::from(leaf.elevation_m);
                    count += 1;
                });
                let face = evaluator.value(cell, sector, &[]).elevation_m;
                area += weight;
                l1 += weight * f64::from(face);
                l6 += weight * sum / f64::from(count);
                excess.push(f64::from(
                    (face - range[index].1).max(range[index].0 - face).max(0.0),
                ));
                at_bounds += u32::from(face <= ELEVATION_MIN_M || face >= ELEVATION_MAX_M);
            }
            AggregateRow {
                relief: f64::from(range[index].1 - range[index].0),
                near_water: near_water[index],
                river: river_cell[index],
                l1_bias: l1 / area - f64::from(elevation[index]),
                l6_bias: l6 / area - f64::from(elevation[index]),
                excess,
                at_bounds,
            }
        })
        .collect();
    for (label, near) in [("inland", false), ("near-water", true)] {
        let group: Vec<&AggregateRow> = rows.iter().filter(|row| row.near_water == near).collect();
        let faces: usize = group.iter().map(|row| row.excess.len()).sum();
        let mut outside: Vec<f64> = group
            .iter()
            .flat_map(|row| row.excess.iter().copied())
            .filter(|&excess| excess > 0.0)
            .collect();
        let at_bounds: u32 = group.iter().map(|row| row.at_bounds).sum();
        eprintln!(
            "[aggregate {tier}] {label:<10} L1 faces {faces:6}, at the bounds {at_bounds}, \
             outside the cell-and-neighbour range | {}",
            quantiles(&mut outside)
        );
    }
    let inland_residual = rows
        .iter()
        .filter(|row| !row.near_water && !row.river)
        .map(|row| row.l1_bias.abs())
        .fold(0.0_f64, f64::max);
    eprintln!("[aggregate {tier}] inland river-free max |L1 mean - z| {inland_residual:.6} m (I3)");
    rows.sort_by(|left, right| left.relief.total_cmp(&right.relief));
    let (low, high) = (rows[rows.len() / 3].relief, rows[2 * rows.len() / 3].relief);
    eprintln!(
        "[aggregate {tier}] dry cells {} relief terciles {low:.0}/{high:.0} m",
        rows.len()
    );
    let band_of =
        |row: &AggregateRow| usize::from(row.relief >= low) + usize::from(row.relief >= high);
    for (partition, filter) in [
        ("all", None),
        ("inland", Some(false)),
        ("near-water", Some(true)),
    ] {
        for (label, band) in [
            ("all", None),
            ("low", Some(0)),
            ("mid", Some(1)),
            ("high", Some(2)),
        ] {
            let part: Vec<&AggregateRow> = rows
                .iter()
                .filter(|row| filter.is_none_or(|near| row.near_water == near))
                .filter(|row| band.is_none_or(|band| band_of(row) == band))
                .collect();
            let mut l1: Vec<f64> = part.iter().map(|row| row.l1_bias).collect();
            let mut l6: Vec<f64> = part.iter().map(|row| row.l6_bias).collect();
            eprintln!(
                "[aggregate {tier}] {partition:<10} {label:<4} L1-z {} | L6-z {}",
                quantiles(&mut l1),
                quantiles(&mut l6)
            );
        }
    }
}

/// Great-arc interpolation between two unit directions (normalized chord
/// lerp; exact enough for sampling at sub-cell spacing).
fn lerp_direction(a: [f64; 3], b: [f64; 3], t: f64) -> UnitVector3 {
    UnitVector3::new(
        a[0] + t * (b[0] - a[0]),
        a[1] + t * (b[1] - a[1]),
        a[2] + t * (b[2] - a[2]),
    )
    .expect("sub-cell interpolation stays inside one hemisphere")
}

fn arc_km(a: [f64; 3], b: [f64; 3], radius_km: f64) -> f64 {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    dot.clamp(-1.0, 1.0).acos() * radius_km
}

/// Samples per centroid–portal leg of the straight L0 chain.
const MOUTH_CHAIN_STEPS: u32 = 256;

/// Per-tier, per-level estuary counters of the mouth probe.
#[derive(Default)]
struct MouthTally {
    reaches: u32,
    /// First fine-water sample on the straight L0 chain, km before / after
    /// the portal; `chain_never` counts chains that never meet water.
    chain_before_km: Vec<f64>,
    chain_after_km: Vec<f64>,
    chain_never: u32,
    /// Drawn line: km of line drawn over water past its first wet vertex
    /// (the drawn end is wet), km from a dry drawn end to the first water
    /// along the arc to the P5 water node, and dry ends that never meet
    /// water before that node.
    overlap_km: Vec<f64>,
    gap_km: Vec<f64>,
    gap_never: u32,
    drawn_vertices: u64,
    drawn_wet_vertices: u64,
}

/// LOD-1 spec §1.2 / §4 I8 (T0 baseline, T3 re-measure): every P5 reach
/// joining a dry cell and a water body, at display levels 6 and 10. The
/// drawn polyline is the production `river_path` at the display depth
/// `level − 1`, each vertex classified by the production fine shoreline on
/// the production leaf value under it. Dry→dry reaches report their drawn
/// vertices on fine water too. Prints numbers; no gate.
#[test]
#[ignore = "release-only offline estuary probe (LOD-1 spec §4 I8)"]
fn river_mouth_probe() {
    river_mouth(causal_formation_fixture(), "draft");
}

#[test]
#[ignore = "release-only offline Standard re-check (LOD-1 spec §6)"]
fn river_mouth_probe_standard() {
    river_mouth(standard_fixture(), "standard");
}

fn river_mouth(fixture: &CausalFormationFixture, tier: &str) {
    let evaluator = evaluator_for(fixture);
    let shoreline = evaluator.shoreline();
    let hydrology = fixture.artifact.bundle().surface_formation().hydrology();
    let water = hydrology.surface_water();
    let cells = fixture.surface.cells();
    let radius_km = evaluator.radius_m() / 1_000.0;
    let segments = hydrology.river_segments();
    for level in [6_u8, 10] {
        let wet = |direction: UnitVector3| {
            displayed_kind(&evaluator, shoreline, direction, level) != SurfaceWaterKind::DryLand
        };
        let tallies: Vec<(bool, MouthTally)> = segments
            .par_iter()
            .enumerate()
            .filter_map(|(reach, segment)| {
                let from = segment.from().raw() as usize;
                let to = segment.to().raw() as usize;
                let dry = |index: usize| water.get(index) == Some(SurfaceWaterKind::DryLand);
                let mut tally = MouthTally::default();
                let drawn = evaluator.river_path(reach as u32, level - 1);
                let flags: Vec<bool> = drawn.iter().map(|&point| wet(point)).collect();
                let mut path: Vec<[f64; 3]> =
                    drawn.iter().map(|point| point.components()).collect();
                tally.drawn_vertices = path.len() as u64;
                tally.drawn_wet_vertices = flags.iter().filter(|&&flag| flag).count() as u64;
                match (dry(from), dry(to)) {
                    (true, true) => return Some((false, tally)),
                    (false, false) => return None,
                    _ => {}
                }
                tally.reaches = 1;
                // Orient dry end first, water end last.
                let (dry_cell, water_cell) = if dry(from) { (from, to) } else { (to, from) };
                let mut flags = flags;
                if !dry(from) {
                    path.reverse();
                    flags.reverse();
                }
                let dry_center = cells[dry_cell].centroid.components();
                let water_center = cells[water_cell].centroid.components();
                let portal = cells[dry_cell]
                    .boundary_edges
                    .iter()
                    .find(|edge| cells[water_cell].boundary_edges.contains(edge))
                    .and_then(|&edge| fixture.surface.edge(edge))
                    .expect("published reaches cross one shared edge")
                    .midpoint
                    .components();
                // The straight L0 chain dry centroid → portal → water node.
                let first_leg = arc_km(dry_center, portal, radius_km);
                let second_leg = arc_km(portal, water_center, radius_km);
                let chain_hit = (0..=2 * MOUTH_CHAIN_STEPS).find_map(|step| {
                    let (point, signed_km) = if step <= MOUTH_CHAIN_STEPS {
                        let t = f64::from(step) / f64::from(MOUTH_CHAIN_STEPS);
                        (
                            lerp_direction(dry_center, portal, t),
                            -first_leg * (1.0 - t),
                        )
                    } else {
                        let t = f64::from(step - MOUTH_CHAIN_STEPS) / f64::from(MOUTH_CHAIN_STEPS);
                        (lerp_direction(portal, water_center, t), second_leg * t)
                    };
                    wet(point).then_some(signed_km)
                });
                match chain_hit {
                    None => tally.chain_never = 1,
                    Some(km) if km < 0.0 => tally.chain_before_km.push(-km),
                    Some(km) => tally.chain_after_km.push(km),
                }
                // The drawn line against the fine shoreline.
                if path.is_empty() {
                    return Some((true, tally));
                }
                if let Some(first_wet) = flags.iter().position(|&flag| flag) {
                    if flags.last() == Some(&true) {
                        let overlap = path[first_wet..]
                            .windows(2)
                            .map(|pair| arc_km(pair[0], pair[1], radius_km))
                            .sum();
                        tally.overlap_km.push(overlap);
                        return Some((true, tally));
                    }
                }
                let end = *path.last().expect("checked non-empty");
                let span = arc_km(end, water_center, radius_km);
                match (1..=2 * MOUTH_CHAIN_STEPS).find(|&step| {
                    let t = f64::from(step) / f64::from(2 * MOUTH_CHAIN_STEPS);
                    wet(lerp_direction(end, water_center, t))
                }) {
                    Some(step) => tally
                        .gap_km
                        .push(span * f64::from(step) / f64::from(2 * MOUTH_CHAIN_STEPS)),
                    None => tally.gap_never = 1,
                }
                Some((true, tally))
            })
            .collect();
        let mut mouth = MouthTally::default();
        let (mut inner_vertices, mut inner_wet) = (0_u64, 0_u64);
        for (is_mouth, tally) in tallies {
            if !is_mouth {
                inner_vertices += tally.drawn_vertices;
                inner_wet += tally.drawn_wet_vertices;
                continue;
            }
            mouth.reaches += tally.reaches;
            mouth.chain_before_km.extend(tally.chain_before_km);
            mouth.chain_after_km.extend(tally.chain_after_km);
            mouth.chain_never += tally.chain_never;
            mouth.overlap_km.extend(tally.overlap_km);
            mouth.gap_km.extend(tally.gap_km);
            mouth.gap_never += tally.gap_never;
            mouth.drawn_vertices += tally.drawn_vertices;
            mouth.drawn_wet_vertices += tally.drawn_wet_vertices;
        }
        let tag = format!("[mouth {tier} L{level}]");
        eprintln!(
            "{tag} dry<->water reaches {}; L0 chain first water before portal {} | {}",
            mouth.reaches,
            mouth.chain_before_km.len(),
            quantiles(&mut mouth.chain_before_km)
        );
        eprintln!(
            "{tag} L0 chain first water after portal {} | {}; never {}",
            mouth.chain_after_km.len(),
            quantiles(&mut mouth.chain_after_km),
            mouth.chain_never
        );
        eprintln!(
            "{tag} drawn end on water (overlap km) {} | {}",
            mouth.overlap_km.len(),
            quantiles(&mut mouth.overlap_km)
        );
        eprintln!(
            "{tag} drawn end dry (gap km) {} | {}; never meets water {}",
            mouth.gap_km.len(),
            quantiles(&mut mouth.gap_km),
            mouth.gap_never
        );
        eprintln!(
            "{tag} drawn vertices on fine water: mouth reaches {:.2}% of {}; dry->dry reaches \
             {:.2}% of {inner_vertices}",
            100.0 * mouth.drawn_wet_vertices as f64 / mouth.drawn_vertices.max(1) as f64,
            mouth.drawn_vertices,
            100.0 * inner_wet as f64 / inner_vertices.max(1) as f64
        );
    }
}

/// LOD-1 spec §1.2 (audit §2.8) and §4 I9: over 16,384 Fibonacci picks,
/// how often the L0 inspector reading (P5 whole-cell water kind, P5 cell
/// elevation) agrees with the leaf the terrain mesh draws at levels 6 and
/// 10. The derived inspector rows are bit-identical to the drawn leaf by
/// the I9 unit test; this probe measures what the L0 rows alone would show.
#[test]
#[ignore = "release-only offline picking probe (LOD-1 spec §4 I9)"]
fn pick_display_probe() {
    pick_display(causal_formation_fixture(), "draft");
}

#[test]
#[ignore = "release-only offline Standard re-check (LOD-1 spec §6)"]
fn pick_display_probe_standard() {
    pick_display(standard_fixture(), "standard");
}

fn pick_display(fixture: &CausalFormationFixture, tier: &str) {
    let evaluator = evaluator_for(fixture);
    let shoreline = evaluator.shoreline();
    let water = fixture
        .artifact
        .bundle()
        .surface_formation()
        .hydrology()
        .surface_water();
    let total = 16_384_usize;
    for level in [6_u8, 10] {
        let picks: Vec<(bool, Option<f64>)> = (0..total)
            .into_par_iter()
            .map(|index| {
                let probe = fibonacci_probe(index, total);
                let cell = root_cell(&evaluator, probe);
                let leaf = evaluator.sample(probe, level).elevation_m;
                let drawn = shoreline.classify(cell, leaf);
                let l0_kind = water
                    .get(cell.raw() as usize)
                    .expect("validated water covers the surface");
                let l0_elevation = evaluator.cell_value(cell).elevation_m;
                let dry_delta = (drawn == SurfaceWaterKind::DryLand)
                    .then(|| f64::from((leaf - l0_elevation).abs()));
                (drawn == l0_kind, dry_delta)
            })
            .collect();
        let agree = picks.iter().filter(|(same, _)| *same).count();
        let mut deltas: Vec<f64> = picks.iter().filter_map(|(_, delta)| *delta).collect();
        eprintln!(
            "[pick {tier} L{level}] L0 water kind = drawn leaf class on {:.2}% of {total}; dry \
             picks |L0 elevation - leaf| m | {}",
            100.0 * agree as f64 / total as f64,
            quantiles(&mut deltas)
        );
    }
}
