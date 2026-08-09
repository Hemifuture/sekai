use std::collections::{BTreeSet, VecDeque};
use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::TectonicGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryKind, CrustKind, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    SphericalTectonicSnapshot, SphericalTectonicValidationError, TectonicActivity, TectonicSpec,
    WorldFormationPreset, MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, PlateId, RootSeed, SphericalSpaceSpec};

const WEAK_SPEED_MM_PER_YEAR: f64 = 8.0;
const MODERATE_MINIMUM_INTERFACE_SPEED_MM_PER_YEAR: f64 = 16.0;

fn surface() -> &'static sekai::world::spatial::SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<sekai::world::spatial::SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        })
        .unwrap()
    })
}

fn formation(preset: ResolvedWorldFormationPreset) -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        preset,
    )
    .unwrap()
}

fn stage_rng(root_seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(root_seed),
        StageIdentity::new("natural.spherical-tectonics", 2, "sekai.core"),
    ))
}

fn generate(
    root_seed: u64,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
) -> SphericalTectonicSnapshot {
    generate_on(surface(), root_seed, spec, preset)
}

fn generate_on(
    target_surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    root_seed: u64,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
) -> SphericalTectonicSnapshot {
    TectonicGenerator::generate_spherical(
        target_surface,
        spec,
        &formation(preset),
        &mut stage_rng(root_seed),
    )
    .unwrap_or_else(|error| panic!("{preset:?} generation failed: {error:?}"))
}

fn morphology_surface() -> &'static sekai::world::spatial::SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<sekai::world::spatial::SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 642,
        })
        .unwrap()
    })
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn subtract(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[0] - second[0],
        first[1] - second[1],
        first[2] - second[2],
    ]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn normalized_pair(first: PlateId, second: PlateId) -> [PlateId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}

fn continental_component_areas(
    target_surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
) -> Vec<f64> {
    let mut visited = vec![false; target_surface.cells().len()];
    let mut component_areas = Vec::new();
    for cell in target_surface.cells() {
        let start = cell.id.raw() as usize;
        if visited[start] || snapshot.crust_kind(cell.id) != Some(CrustKind::Continental) {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([cell.id]);
        let mut area = 0.0;
        while let Some(current) = queue.pop_front() {
            area += target_surface.cell(current).unwrap().area.get();
            for &edge in target_surface.cell_edges(current).unwrap() {
                let neighbor = target_surface.opposite_cell(current, edge).unwrap();
                let index = neighbor.raw() as usize;
                if !visited[index] && snapshot.crust_kind(neighbor) == Some(CrustKind::Continental)
                {
                    visited[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        component_areas.push(area);
    }
    component_areas
}

#[test]
fn spherical_rotations_are_repeatable_bounded_connected_and_locally_separated() {
    let spec = TectonicSpec::default();
    let first = generate(0xC0_FFEE, &spec, ResolvedWorldFormationPreset::Continents);
    let repeated = generate(0xC0_FFEE, &spec, ResolvedWorldFormationPreset::Continents);
    let changed = generate(0xC0_FFEF, &spec, ResolvedWorldFormationPreset::Continents);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    let decoded: SphericalTectonicSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    assert_eq!(decoded, first);
    assert_ne!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&changed).unwrap()
    );
    assert_ne!(first.plates(), changed.plates());
    assert_eq!(first.plates().len(), usize::from(spec.plate_count));
    first.validate_against(surface()).unwrap();

    for plate in first.plates() {
        assert_eq!(first.plate_for_cell(plate.seed_cell()), Some(plate.id()));
        assert!(
            plate
                .rotation()
                .maximum_speed_mm_per_year(surface().radius())
                .unwrap()
                <= MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR
        );

        let expected = first
            .cell_plates()
            .raw_values()
            .iter()
            .filter(|&&owner| owner == plate.id().raw())
            .count();
        let mut visited = vec![false; surface().cells().len()];
        let mut queue = VecDeque::from([plate.seed_cell()]);
        visited[plate.seed_cell().raw() as usize] = true;
        let mut reached = 0;
        while let Some(cell) = queue.pop_front() {
            reached += 1;
            for &edge in surface().cell_edges(cell).unwrap() {
                let neighbor = surface().opposite_cell(cell, edge).unwrap();
                let index = neighbor.raw() as usize;
                if !visited[index] && first.plate_for_cell(neighbor) == Some(plate.id()) {
                    visited[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        assert_eq!(reached, expected);
    }

    let mut adjacent_pairs = BTreeSet::new();
    for edge in surface().edges() {
        let plates = edge.cells.map(|cell| first.plate_for_cell(cell).unwrap());
        if plates[0] == plates[1] {
            continue;
        }
        adjacent_pairs.insert(normalized_pair(plates[0], plates[1]));
        let velocities = plates.map(|plate| {
            first.plates()[plate.raw() as usize]
                .rotation()
                .velocity_mm_per_year(surface().radius(), edge.midpoint)
                .unwrap()
        });
        let relative = subtract(velocities[1], velocities[0]);
        assert!(
            norm(relative) + 1.0e-9 >= MODERATE_MINIMUM_INTERFACE_SPEED_MM_PER_YEAR,
            "edge {:?} has only {} mm/year relative motion",
            edge.id,
            norm(relative)
        );
        assert!(dot(velocities[0], edge.midpoint.components()).abs() <= 1.0e-8);
        assert!(dot(velocities[1], edge.midpoint.components()).abs() <= 1.0e-8);
    }
    assert!(!adjacent_pairs.is_empty());
}

#[test]
fn activity_and_plate_count_matrix_stays_inside_the_physical_envelope() {
    for activity in [
        TectonicActivity::Quiet,
        TectonicActivity::Moderate,
        TectonicActivity::Active,
    ] {
        for plate_count in [2, 12, 64] {
            let spec = TectonicSpec {
                plate_count,
                activity,
                ..TectonicSpec::default()
            };
            let snapshot = generate(
                0xA11C_E000 + u64::from(plate_count),
                &spec,
                ResolvedWorldFormationPreset::Continents,
            );
            assert_eq!(snapshot.plates().len(), usize::from(plate_count));
            snapshot.validate_against(surface()).unwrap();
            assert!(snapshot.plates().iter().all(|plate| {
                plate
                    .rotation()
                    .maximum_speed_mm_per_year(surface().radius())
                    .unwrap()
                    <= MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR
            }));
        }
    }
}

#[test]
fn spherical_boundaries_use_each_edges_local_tangent_frame_and_canonical_vertices() {
    let snapshot = generate(
        0xC0_FFEE,
        &TectonicSpec::default(),
        ResolvedWorldFormationPreset::Continents,
    );
    let mut saw_polar_edge = false;
    let mut saw_antimeridian_edge = false;
    for edge in surface().edges() {
        let record = snapshot.boundaries()[edge.id.raw() as usize];
        assert!(record.strength.is_finite() && (0.0..=1.0).contains(&record.strength));
        let owner_plates = edge
            .cells
            .map(|cell| snapshot.plate_for_cell(cell).unwrap());
        if owner_plates[0] == owner_plates[1] {
            assert_eq!(record.kind, BoundaryKind::None);
            continue;
        }
        assert_ne!(record.kind, BoundaryKind::None);
        assert!(record.segment_id.is_some());

        let velocities = owner_plates.map(|plate| {
            snapshot.plates()[plate.raw() as usize]
                .rotation()
                .velocity_mm_per_year(surface().radius(), edge.midpoint)
                .unwrap()
        });
        let relative = subtract(velocities[1], velocities[0]);
        let speed = norm(relative);
        let normal_speed = dot(relative, edge.normal_from_first.components());
        if speed < WEAK_SPEED_MM_PER_YEAR {
            assert_eq!(record.kind, BoundaryKind::Weak);
        } else if normal_speed.abs() < speed * 0.4 {
            assert_eq!(record.kind, BoundaryKind::Transform);
        } else if normal_speed < 0.0 {
            assert!(matches!(
                record.kind,
                BoundaryKind::ContinentalCollision | BoundaryKind::Subduction
            ));
        } else {
            assert!(matches!(
                record.kind,
                BoundaryKind::ContinentalRift | BoundaryKind::OceanicRidge
            ));
        }

        if record.kind == BoundaryKind::Subduction {
            let crust = edge.cells.map(|cell| snapshot.crust_kind(cell).unwrap());
            let thickness = edge
                .cells
                .map(|cell| snapshot.crust_thickness_for_cell(cell).unwrap());
            let expected = match crust {
                [CrustKind::Oceanic, CrustKind::Continental] => owner_plates[0],
                [CrustKind::Continental, CrustKind::Oceanic] => owner_plates[1],
                _ if thickness[0] < thickness[1] => owner_plates[0],
                _ if thickness[1] < thickness[0] => owner_plates[1],
                _ => owner_plates[0].min(owner_plates[1]),
            };
            assert_eq!(record.subducting_plate, Some(expected));
        }

        let midpoint = edge.midpoint.components();
        saw_polar_edge |= midpoint[2].abs() > 0.9;
        saw_antimeridian_edge |= midpoint[0] < -0.9;
    }
    assert!(saw_polar_edge && saw_antimeridian_edge);

    for (index, segment) in snapshot.boundary_segments().iter().enumerate() {
        assert_eq!(segment.id().raw() as usize, index);
        assert!(segment.plates()[0] < segment.plates()[1]);
        assert!(segment
            .member_edges()
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
        let members = segment
            .member_edges()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut reached = BTreeSet::new();
        let mut queue = VecDeque::from([segment.member_edges()[0]]);
        while let Some(edge_id) = queue.pop_front() {
            if !reached.insert(edge_id) {
                continue;
            }
            let vertices = surface().edge(edge_id).unwrap().vertices;
            for &candidate in &members {
                let candidate_vertices = surface().edge(candidate).unwrap().vertices;
                if vertices
                    .iter()
                    .any(|vertex| candidate_vertices.contains(vertex))
                {
                    queue.push_back(candidate);
                }
            }
        }
        assert_eq!(reached, members);
    }
}

#[test]
fn authoritative_surface_kinematics_reject_forged_boundary_strengths() {
    let snapshot = generate(
        0xC0_FFEE,
        &TectonicSpec::default(),
        ResolvedWorldFormationPreset::Continents,
    );
    let segment = snapshot
        .boundary_segments()
        .first()
        .expect("the multi-plate fixture has a boundary segment");
    let forged_strength = if segment.mean_strength() < 0.5 {
        1.0
    } else {
        0.0
    };
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    for edge in segment.member_edges() {
        encoded["boundaries"][edge.raw() as usize]["strength"] = serde_json::json!(forged_strength);
    }
    encoded["boundary_segments"][segment.id().raw() as usize]["mean_strength"] =
        serde_json::json!(forged_strength);

    let forged: SphericalTectonicSnapshot = serde_json::from_value(encoded).unwrap();
    assert!(forged.validate().is_ok());
    assert!(matches!(
        forged.validate_against(surface()),
        Err(SphericalTectonicValidationError::BoundaryKinematicsMismatch { .. })
    ));
}

#[test]
fn every_formation_preset_uses_global_area_and_soft_plate_coupling() {
    let target_surface = morphology_surface();
    let cases = [
        (ResolvedWorldFormationPreset::Continents, 0.38, 3..=5),
        (ResolvedWorldFormationPreset::Archipelago, 0.26, 2..=6),
        (ResolvedWorldFormationPreset::Supercontinent, 0.42, 1..=1),
        (ResolvedWorldFormationPreset::GreatIsland, 0.28, 1..=1),
        (ResolvedWorldFormationPreset::VolcanicIslands, 0.16, 0..=2),
    ];
    let total_area = target_surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    let maximum_cell_area = target_surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .fold(0.0, f64::max);

    for (preset, fraction, component_envelope) in cases {
        let spec = TectonicSpec {
            continental_crust_fraction: fraction,
            ..TectonicSpec::default()
        };
        let first = generate_on(target_surface, 0xC0_FFEE, &spec, preset);
        let repeated = generate_on(target_surface, 0xC0_FFEE, &spec, preset);
        assert_eq!(first, repeated);

        let continental_area = target_surface
            .cells()
            .iter()
            .filter(|cell| first.crust_kind(cell.id) == Some(CrustKind::Continental))
            .map(|cell| cell.area.get())
            .sum::<f64>();
        assert!((continental_area - total_area * f64::from(fraction)).abs() <= maximum_cell_area);
        assert!(first
            .crust_kinds()
            .raw_values()
            .iter()
            .any(|&kind| kind == CrustKind::Continental.raw()));
        assert!(first
            .crust_kinds()
            .raw_values()
            .iter()
            .any(|&kind| kind == CrustKind::Oceanic.raw()));
        assert!(target_surface
            .edges()
            .iter()
            .all(|edge| edge.cells[0] != edge.cells[1]));
        let component_areas = continental_component_areas(target_surface, &first);
        let component_total = component_areas.iter().sum::<f64>();
        let component_count = component_areas
            .iter()
            .filter(|&&area| area * 10.0 >= component_total)
            .count();
        assert!(
            component_envelope.contains(&component_count),
            "{preset:?} produced {component_count} major continental components"
        );
    }

    let twelve = generate_on(
        target_surface,
        0xC0_FFEE,
        &TectonicSpec::default(),
        ResolvedWorldFormationPreset::Continents,
    );
    let seventeen = generate_on(
        target_surface,
        0xC0_FFEE,
        &TectonicSpec {
            plate_count: 17,
            ..TectonicSpec::default()
        },
        ResolvedWorldFormationPreset::Continents,
    );
    assert_ne!(twelve.crust_kinds(), seventeen.crust_kinds());
    let intersection = twelve
        .crust_kinds()
        .raw_values()
        .iter()
        .zip(seventeen.crust_kinds().raw_values())
        .filter(|&(&first, &second)| {
            first == CrustKind::Continental.raw() && second == CrustKind::Continental.raw()
        })
        .count();
    let union = twelve
        .crust_kinds()
        .raw_values()
        .iter()
        .zip(seventeen.crust_kinds().raw_values())
        .filter(|&(&first, &second)| {
            first == CrustKind::Continental.raw() || second == CrustKind::Continental.raw()
        })
        .count();
    assert!(intersection as f64 / union as f64 >= 0.55);
}
