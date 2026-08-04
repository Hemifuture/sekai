use sekai::engine::{derive_stage_seed, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, ReliefGenerator, TectonicGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryKind, GeologicSpec, MantleFormationBias, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, SphericalMantleSnapshot, SphericalReliefSnapshot,
    SphericalTectonicSnapshot, TectonicSpec, WorldFormationPreset, COMPONENT_IDENTITY_TOLERANCE_M,
    ELEVATION_MAX_M, ELEVATION_MIN_M, REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1, TECTONIC_OFFSET_MAX_M, TECTONIC_OFFSET_MIN_M,
    VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
use sekai::world::spatial::{central_angle, project_tangent, UnitVector3};
use sekai::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

fn surface(target_cell_count: u32) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn stage_rng(name: &'static str, seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(name, 1, "sekai.spherical-relief-tests"),
    ))
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn upstream(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    seed: u64,
) -> (SphericalTectonicSnapshot, SphericalMantleSnapshot) {
    let tectonic = TectonicGenerator::generate_spherical(
        surface,
        &TectonicSpec::default(),
        &formation(),
        &mut stage_rng("spherical-relief-tectonics", seed),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        surface,
        &GeologicSpec::default(),
        MantleFormationBias::Neutral,
        &mut stage_rng("spherical-relief-mantle", seed),
    )
    .unwrap();
    (tectonic, mantle)
}

fn generate(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
    mantle: &SphericalMantleSnapshot,
    seed: u64,
) -> (SphericalReliefSnapshot, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let relief = ReliefGenerator::generate_spherical(
        surface,
        tectonic,
        mantle,
        &mut stage_rng("spherical-relief", seed),
        &mut diagnostics,
    )
    .unwrap();
    (relief, diagnostics)
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = norm(vector);
    (length > f64::EPSILON).then(|| vector.map(|component| component / length))
}

#[test]
fn spherical_relief_is_deterministic_explainable_bounded_and_seed_sensitive() {
    let sphere = surface(162);
    let (tectonic, mantle) = upstream(&sphere, 0x0005_10B3);
    let (first, diagnostics) = generate(&sphere, &tectonic, &mantle, 91);
    let (repeated, repeated_diagnostics) = generate(&sphere, &tectonic, &mantle, 91);
    let (changed, _) = generate(&sphere, &tectonic, &mantle, 92);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    assert_eq!(diagnostics, repeated_diagnostics);
    assert_ne!(first.regional_offset_m(), changed.regional_offset_m());
    first.validate_against(&sphere, &tectonic, &mantle).unwrap();

    let mut land = 0;
    let mut ocean = 0;
    for index in 0..sphere.cells().len() {
        let base = first.crust_base_elevation_m().values()[index];
        let tectonic = first.tectonic_offset_m().values()[index];
        let volcanic = first.volcanic_offset_m().values()[index];
        let regional = first.regional_offset_m().values()[index];
        let elevation = first.elevation_m().values()[index];
        assert!(
            (elevation - (base + tectonic + volcanic + regional)).abs()
                <= COMPONENT_IDENTITY_TOLERANCE_M
        );
        assert!((ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&elevation));
        assert!((TECTONIC_OFFSET_MIN_M..=TECTONIC_OFFSET_MAX_M).contains(&tectonic));
        assert!((VOLCANIC_OFFSET_MIN_M..=VOLCANIC_OFFSET_MAX_M).contains(&volcanic));
        assert!((REGIONAL_OFFSET_MIN_M..=REGIONAL_OFFSET_MAX_M).contains(&regional));
        match first
            .land_ocean_kind(CellId::from_raw(index as u32))
            .unwrap()
        {
            sekai::world::natural::LandOceanKind::Land => land += 1,
            sekai::world::natural::LandOceanKind::Ocean => ocean += 1,
        }
    }
    assert!(land > 0 && ocean > 0, "land={land}, ocean={ocean}");
}

#[test]
fn spherical_regional_relief_has_area_weighted_zero_mean_and_no_cut_or_pole_jump() {
    let sphere = surface(642);
    let (tectonic, mantle) = upstream(&sphere, 0x000A_11CE);
    let (relief, _) = generate(&sphere, &tectonic, &mantle, 101);
    let regional = relief.regional_offset_m().values();

    let total_area = sphere.total_cell_area().get();
    let weighted_mean = sphere
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| f64::from(regional[index]) * cell.area.get())
        .sum::<f64>()
        / total_area;
    assert!(
        weighted_mean.abs() < 0.05,
        "area-weighted mean={weighted_mean}"
    );

    let mut all_jumps = Vec::new();
    let mut cut_jumps = Vec::new();
    let mut polar_jumps = Vec::new();
    for edge in sphere.edges() {
        let [first, second] = edge.cells;
        let jump = (regional[first.raw() as usize] - regional[second.raw() as usize]).abs();
        all_jumps.push(jump);
        let first_radial = sphere.cell(first).unwrap().centroid.components();
        let second_radial = sphere.cell(second).unwrap().centroid.components();
        if first_radial[1].is_sign_positive() != second_radial[1].is_sign_positive()
            && first_radial[0] < 0.0
            && second_radial[0] < 0.0
        {
            cut_jumps.push(jump);
        }
        if first_radial[2].abs().max(second_radial[2].abs()) > 0.9 {
            polar_jumps.push(jump);
        }
    }
    let mean = |values: &[f32]| {
        values.iter().map(|&value| f64::from(value)).sum::<f64>() / values.len() as f64
    };
    assert!(!cut_jumps.is_empty() && !polar_jumps.is_empty());
    assert!(mean(&cut_jumps) <= mean(&all_jumps) * 2.5 + 1.0);
    assert!(mean(&polar_jumps) <= mean(&all_jumps) * 2.5 + 1.0);
}

#[test]
fn boundary_relief_has_causal_sides_and_hotspots_keep_current_centers_and_plate_motion_trails() {
    let sphere = surface(642);
    let (tectonic, mantle) = upstream(&sphere, 0xC0_A57);
    let (relief, _) = generate(&sphere, &tectonic, &mantle, 113);
    let offsets = relief.tectonic_offset_m().values();
    let mut collision = Vec::new();
    let mut ridge = Vec::new();
    let mut rift = Vec::new();
    let mut subduction = Vec::new();

    for edge in sphere.edges() {
        let record = &tectonic.boundaries()[edge.id.raw() as usize];
        let [first, second] = edge.cells;
        if tectonic.plate_for_cell(first) == tectonic.plate_for_cell(second) {
            continue;
        }
        let pair_mean = (offsets[first.raw() as usize] + offsets[second.raw() as usize]) * 0.5;
        match record.kind {
            BoundaryKind::ContinentalCollision => collision.push(pair_mean),
            BoundaryKind::OceanicRidge => ridge.push(pair_mean),
            BoundaryKind::ContinentalRift => {
                rift.push(-pair_mean);
            }
            BoundaryKind::Subduction => {
                let subducting = record.subducting_plate.unwrap();
                let first_plate = tectonic.plate_for_cell(first).unwrap();
                let (trench, arc) = if first_plate == subducting {
                    (first, second)
                } else {
                    (second, first)
                };
                subduction.push(offsets[arc.raw() as usize] - offsets[trench.raw() as usize]);
            }
            BoundaryKind::None | BoundaryKind::Weak | BoundaryKind::Transform => {}
        }
    }
    assert!([&collision, &ridge, &rift, &subduction]
        .into_iter()
        .flatten()
        .any(|&value| value > 0.0));
    for (kind, values) in [
        (BoundaryKind::ContinentalCollision, collision),
        (BoundaryKind::OceanicRidge, ridge),
        (BoundaryKind::ContinentalRift, rift),
        (BoundaryKind::Subduction, subduction),
    ] {
        if values.is_empty() {
            continue;
        }
        assert!(
            values.iter().copied().fold(f32::NEG_INFINITY, f32::max) > 0.0,
            "{kind:?} never produced its expected relief sign"
        );
    }

    let volcanic = relief.volcanic_offset_m().values();
    for hotspot in mantle.hotspots() {
        let source = hotspot.source_cell();
        let source_index = source.raw() as usize;
        assert!(volcanic[source_index] > 0.0);
        assert!(
            volcanic[source_index]
                >= volcanic
                    .iter()
                    .copied()
                    .filter(|value| value.is_finite())
                    .fold(0.0_f32, f32::max)
                    * 0.2
        );

        let source_radial = sphere.cell(source).unwrap().centroid;
        let source_plate = tectonic.plate_for_cell(source).unwrap();
        let velocity = tectonic.plates()[source_plate.raw() as usize]
            .rotation()
            .velocity_mm_per_year(sphere.radius(), source_radial)
            .unwrap();
        let Some(direction) = normalize(velocity) else {
            continue;
        };
        let mut positive = Vec::new();
        let mut negative = Vec::new();
        for cell in sphere.cells() {
            let index = cell.id.raw() as usize;
            if tectonic.plate_for_cell(cell.id) != Some(source_plate)
                || mantle.volcanic_influence()[index] <= 0.0
            {
                continue;
            }
            let angle = central_angle(source_radial, cell.centroid);
            let distance_fraction =
                angle * sphere.radius().get() / hotspot.support_radius_m().get();
            if !(0.12..0.75).contains(&distance_fraction) {
                continue;
            }
            let tangent = project_tangent(cell.centroid.components(), source_radial);
            let Some(local_direction) = normalize(tangent) else {
                continue;
            };
            if dot(local_direction, direction) > 0.35 {
                positive.push(volcanic[index]);
            } else if dot(local_direction, direction) < -0.35 {
                negative.push(volcanic[index]);
            }
        }
        if !positive.is_empty() && !negative.is_empty() {
            let maximum = |values: &[f32]| values.iter().copied().fold(0.0_f32, f32::max);
            assert!(maximum(&positive) >= maximum(&negative));
        }
    }

    for (index, &influence) in mantle.volcanic_influence().iter().enumerate() {
        if influence <= 0.0 {
            assert_eq!(volcanic[index], 0.0);
        }
    }
}

#[test]
fn tangent_projection_used_by_trails_is_well_defined_at_poles_and_antimeridian() {
    for radial in [
        UnitVector3::new(0.0, 0.0, 1.0).unwrap(),
        UnitVector3::new(-1.0, 0.0, 0.0).unwrap(),
    ] {
        let tangent = project_tangent([0.3, -0.5, 0.7], radial);
        assert!(tangent.iter().all(|value| value.is_finite()));
        assert!(dot(tangent, radial.components()).abs() < 1.0e-12);
    }
}
