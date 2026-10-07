//! Product-level gates of the T1 v2 hierarchical derivation on the real
//! Draft seed-42 formation bundle (spec `2026-08-20-t1v2-hierarchical-
//! derivation.md` §7 and amendment A4), restored as stage 0 of the T1 v2.2
//! spec (`2026-09-29-t1v2-2-lod-refinement-design.md` §2). The unit tests in
//! `hierarchical_derivation` cover the same invariants on synthetic fields;
//! only this suite measures them on the product every later stage changes.

mod support;

use sekai::generators::natural::{
    fibonacci_probe, FormationDerivationInputs, HierarchicalEvaluator,
};
use sekai::world::natural::{ELEVATION_MAX_M, ELEVATION_MIN_M};
use sekai::world::spatial::{audited_float_platform, spherical_triangle_area_unit};
use sekai::world::{CellId, RootSeed};
use support::causal_formation::causal_formation_fixture;

/// Draft seed-42 probe fingerprint (spec A4 formula), measured on the bundle
/// once milestone A8b (endpoint P4 reuses the start FAS correction) changed
/// the bundled endpoint climate. It supersedes A12's `992518cf…`; L0 identity
/// held and the land drift stayed 0.0139 (T1 v2 spec amendment A13).
const EXPECTED_PROBE_FINGERPRINT: &str =
    "0eb17f04062f97e98d9246087f1276ad8df0fbcd4183ccf4c2d354ae54d7281a";

fn evaluator() -> HierarchicalEvaluator {
    let fixture = causal_formation_fixture();
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

/// Spec §7 invariant 6 on the product: the level-6 land fraction stays within
/// one percentage point of L0 over the same 16,384 Fibonacci probes as the
/// unit test. Red when restored (T1 v2.2 spec §1.8); stage 1 must turn it
/// green and remove the ignore, never widen the threshold.
#[test]
#[ignore = "red until T1 v2.2 stage 1: measured drift 0.0139 on Draft seed 42 (spec §1.8)"]
fn deep_land_fraction_stays_within_one_point_of_l0() {
    let evaluator = evaluator();
    let sea_level = causal_formation_fixture()
        .artifact
        .bundle()
        .surface_formation()
        .terrain_fields()
        .sea_level_m();
    let total = 16_384_usize;
    let (mut l0_land, mut deep_land) = (0_u32, 0_u32);
    for index in 0..total {
        let probe = fibonacci_probe(index, total);
        l0_land += u32::from(evaluator.sample(probe, 0).elevation_m >= sea_level);
        deep_land += u32::from(evaluator.sample(probe, 6).elevation_m >= sea_level);
    }
    let drift = (f64::from(l0_land) - f64::from(deep_land)).abs() / total as f64;
    assert!(
        drift <= 0.01,
        "land fraction drift {drift} (L0 land {l0_land}, level-6 land {deep_land} of {total})"
    );
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
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
    format!(
        "p05 {:8.2}  p25 {:8.2}  p50 {:8.2}  p75 {:8.2}  p95 {:8.2}  mean {:8.2}",
        at(0.05),
        at(0.25),
        at(0.5),
        at(0.75),
        at(0.95),
        values.iter().sum::<f64>() / values.len() as f64
    )
}

/// T1 v2.2 spec §3.4: per land cell, the area-weighted mean of its L1 sector
/// faces and of its level-6 leaves minus the cell's P5 value, in metres,
/// grouped by local relief (range of the cell and its edge neighbours), and
/// the L1 faces that leave that range or reach the elevation bounds (spec
/// §9.2 overshoot count).
/// Level-6 leaves are weighted equally inside a sector (the four-way split
/// is only approximately equal-area, spec §1.2). Prints numbers; no gate.
#[test]
#[ignore = "release-only offline aggregate probe (T1 v2.2 spec §3.4)"]
fn aggregate_bias_probe() {
    let evaluator = evaluator();
    let fixture = causal_formation_fixture();
    let terrain = fixture
        .artifact
        .bundle()
        .surface_formation()
        .terrain_fields();
    let elevation = terrain.current_elevation_m();
    let sea_level = terrain.sea_level_m();
    let mut range: Vec<(f32, f32)> = elevation.iter().map(|&z| (z, z)).collect();
    for edge in fixture.surface.edges() {
        let [a, b] = edge.cells.map(|cell| cell.raw() as usize);
        for (cell, other) in [(a, b), (b, a)] {
            range[cell].0 = range[cell].0.min(elevation[other]);
            range[cell].1 = range[cell].1.max(elevation[other]);
        }
    }
    let mut rows = Vec::new();
    let (mut faces, mut outside_neighborhood, mut at_bounds) = (0_u32, 0_u32, 0_u32);
    // Overshoot in metres by cell kind: [inland, inland extremum, coastal,
    // coastal extremum]; coastal means an ocean cell in the neighbourhood.
    let mut overshoot: [Vec<f64>; 4] = Default::default();
    for (index, &z) in elevation.iter().enumerate() {
        if z < sea_level {
            continue;
        }
        let cell = CellId::from_raw(index as u32);
        let (mut area, mut l1, mut l6) = (0.0_f64, 0.0_f64, 0.0_f64);
        for sector in 0..evaluator.sector_count(cell) as u8 {
            let [a, b, c] = evaluator.sector_corners(cell, sector);
            let weight = spherical_triangle_area_unit(a, b, c);
            let (mut sum, mut count) = (0.0_f64, 0_u32);
            evaluator.for_each_leaf_value(cell, sector, &[], 5, &mut |leaf| {
                sum += f64::from(leaf.elevation_m);
                count += 1;
            });
            area += weight;
            l1 += weight * f64::from(evaluator.value(cell, sector, &[]).elevation_m);
            l6 += weight * sum / f64::from(count);
        }
        let relief = f64::from(range[index].1 - range[index].0);
        rows.push((relief, l1 / area - f64::from(z), l6 / area - f64::from(z)));
        let extremum = z == range[index].0 || z == range[index].1;
        let coastal = range[index].0 < sea_level;
        for sector in 0..evaluator.sector_count(cell) as u8 {
            let face = evaluator.value(cell, sector, &[]).elevation_m;
            let excess = (face - range[index].1).max(range[index].0 - face);
            if excess > 0.0 {
                let group = usize::from(extremum) + 2 * usize::from(coastal);
                overshoot[group].push(f64::from(excess));
            }
            outside_neighborhood += u32::from(face < range[index].0 || face > range[index].1);
            at_bounds += u32::from(face <= ELEVATION_MIN_M || face >= ELEVATION_MAX_M);
            faces += 1;
        }
    }
    for (label, values) in ["inland", "inland extremum", "coastal", "coastal extremum"]
        .iter()
        .zip(&mut overshoot)
    {
        if !values.is_empty() {
            eprintln!(
                "[aggregate] overshoot {label:<16} faces {:5} | {}",
                values.len(),
                quantiles(values)
            );
        }
    }
    rows.sort_by(|left, right| left.0.total_cmp(&right.0));
    eprintln!(
        "[aggregate] land cells {}, L1 faces {faces}: outside the cell-and-neighbour range          {outside_neighborhood}, at the elevation bounds {at_bounds}",
        rows.len()
    );
    for (label, part) in [
        ("all", &rows[..]),
        ("low relief", &rows[..rows.len() / 3]),
        ("mid relief", &rows[rows.len() / 3..2 * rows.len() / 3]),
        ("high relief", &rows[2 * rows.len() / 3..]),
    ] {
        let mut l1: Vec<f64> = part.iter().map(|row| row.1).collect();
        let mut l6: Vec<f64> = part.iter().map(|row| row.2).collect();
        eprintln!(
            "[aggregate] {label:<11} relief {:6.0}-{:6.0} m | L1-z {} | L6-z {}",
            part[0].0,
            part[part.len() - 1].0,
            quantiles(&mut l1),
            quantiles(&mut l6)
        );
    }
}
