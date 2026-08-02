use std::array;
use std::collections::BTreeMap;

use rand::RngCore;
use thiserror::Error;

use super::random::{LabeledSubstreams, RELIEF_REGIONAL_LABEL};
use super::topology::{multi_source_distance, multi_source_ownership, NaturalTopologyIndex};
use crate::engine::{Diagnostic, DiagnosticContext, DiagnosticSeverity, StageRng};
use crate::world::natural::{
    BoundaryKind, CrustKind, ElevationField, LandOceanField, MantleSnapshot, MantleValidationError,
    ReliefSnapshot, ReliefValidationError, TectonicSnapshot, TectonicValidationError,
    CRUST_BASE_ELEVATION_MAX_M, CRUST_BASE_ELEVATION_MIN_M, ELEVATION_MAX_M, ELEVATION_MIN_M,
    REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M, RELIEF_SCHEMA_V2, TECTONIC_OFFSET_MAX_M,
    TECTONIC_OFFSET_MIN_M, VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::{CellId, PlateId};

const SEA_LEVEL_M: f32 = 0.0;
const CLOSED_OCEAN_FRAME_SHORT_SIDE_FRACTION: f64 = 0.08;
const OCEAN_FRAME_BASE_M: f32 = -5_200.0;
const MARGIN_SUPPORT_STEPS: u64 = 4;
const REGIONAL_NOISE_SCALE: i64 = 1_000;
const MAX_CLAMP_DIAGNOSTICS: usize = 32;
const CLAMP_DIAGNOSTIC_CODE: &str = "natural.relief-clamped";

/// Deterministic synthesis of explainable present-day relief fields.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReliefGenerator;

impl ReliefGenerator {
    /// Generates crust-base, tectonic, volcanic, regional, and final elevation fields.
    pub fn generate(
        spatial: &SpatialSnapshot,
        tectonic: &TectonicSnapshot,
        mantle: &MantleSnapshot,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<ReliefSnapshot, ReliefGenerationError> {
        tectonic.validate_against(spatial)?;
        mantle.validate_against(spatial)?;
        let streams = LabeledSubstreams::capture(rng);
        let topology = NaturalTopologyIndex::new(spatial);
        let mut crust_base = synthesize_crust_base(&topology, tectonic);
        let mut tectonic_offset = synthesize_tectonic_offset(spatial, &topology, tectonic);
        let mut volcanic_offset = synthesize_volcanic_offset(tectonic, mantle);
        let mut regional_offset = synthesize_regional_offset(&topology, tectonic, &streams);
        apply_closed_ocean_frame(
            &topology,
            &mut crust_base,
            &mut tectonic_offset,
            &mut volcanic_offset,
            &mut regional_offset,
        );
        let elevation = reconcile_final_safety(
            &mut crust_base,
            &mut tectonic_offset,
            &mut volcanic_offset,
            &mut regional_offset,
            diagnostics,
        );
        verify_closed_ocean_frame(&topology, &elevation)?;

        let crust_base = ElevationField::from_values(crust_base)?;
        let tectonic_offset = ElevationField::from_values(tectonic_offset)?;
        let volcanic_offset = ElevationField::from_values(volcanic_offset)?;
        let regional_offset = ElevationField::from_values(regional_offset)?;
        let elevation = ElevationField::from_values(elevation)?;
        let land_ocean = LandOceanField::classify(&elevation, SEA_LEVEL_M);
        let snapshot = ReliefSnapshot::new(
            RELIEF_SCHEMA_V2,
            spatial.cell_count() as u32,
            SEA_LEVEL_M,
            crust_base,
            tectonic_offset,
            volcanic_offset,
            regional_offset,
            elevation,
            land_ocean,
        )?;
        snapshot.validate_against(spatial)?;
        Ok(snapshot)
    }
}

fn apply_closed_ocean_frame(
    topology: &NaturalTopologyIndex,
    crust_base: &mut [f32],
    tectonic_offset: &mut [f32],
    volcanic_offset: &mut [f32],
    regional_offset: &mut [f32],
) {
    let boundary_sources: Vec<_> = topology
        .boundary_cells()
        .iter()
        .enumerate()
        .filter_map(|(index, &boundary)| boundary.then_some(CellId::from_raw(index as u32)))
        .collect();
    let distance = multi_source_distance(topology, &boundary_sources, None);
    let support = topology
        .quantized_short_side_fraction(CLOSED_OCEAN_FRAME_SHORT_SIDE_FRACTION)
        .max(1);
    for index in 0..crust_base.len() {
        let weight = smooth_rise(distance[index], support);
        crust_base[index] = OCEAN_FRAME_BASE_M + (crust_base[index] - OCEAN_FRAME_BASE_M) * weight;
        tectonic_offset[index] = attenuate_positive(tectonic_offset[index], weight);
        volcanic_offset[index] = attenuate_positive(volcanic_offset[index], weight);
        regional_offset[index] = attenuate_positive(regional_offset[index], weight);
    }
}

fn attenuate_positive(value: f32, weight: f32) -> f32 {
    if value > 0.0 {
        value * weight
    } else {
        value
    }
}

fn verify_closed_ocean_frame(
    topology: &NaturalTopologyIndex,
    elevation: &[f32],
) -> Result<(), ReliefGenerationError> {
    for (index, &boundary) in topology.boundary_cells().iter().enumerate() {
        if boundary && elevation[index] >= SEA_LEVEL_M {
            return Err(ReliefGenerationError::ExposedBoundaryCell {
                cell: CellId::from_raw(index as u32),
                elevation_m: elevation[index],
            });
        }
    }
    Ok(())
}

fn synthesize_volcanic_offset(tectonic: &TectonicSnapshot, mantle: &MantleSnapshot) -> Vec<f32> {
    mantle
        .volcanic_influence()
        .iter()
        .enumerate()
        .map(|(index, &influence)| {
            let cell = CellId::from_raw(index as u32);
            let amplitude = match tectonic
                .crust_kind(cell)
                .expect("tectonic and mantle fields are cell aligned")
            {
                CrustKind::Oceanic => 3_200.0,
                CrustKind::Continental => 2_200.0,
            };
            let response = influence * influence * (3.0 - 2.0 * influence);
            (amplitude * response).clamp(VOLCANIC_OFFSET_MIN_M, VOLCANIC_OFFSET_MAX_M)
        })
        .collect()
}

fn synthesize_crust_base(topology: &NaturalTopologyIndex, tectonic: &TectonicSnapshot) -> Vec<f32> {
    let transition_cells: Vec<_> = topology
        .arcs()
        .iter()
        .enumerate()
        .filter_map(|(index, arcs)| {
            let kind = tectonic
                .crust_kind(CellId::from_raw(index as u32))
                .expect("tectonic field is cell aligned");
            arcs.iter()
                .any(|arc| tectonic.crust_kind(arc.neighbor) != Some(kind))
                .then_some(CellId::from_raw(index as u32))
        })
        .collect();
    let distance = if transition_cells.is_empty() {
        vec![u64::MAX; topology.arcs().len()]
    } else {
        multi_source_distance(topology, &transition_cells, None)
    };
    let support = typical_traversal_cost(topology)
        .saturating_mul(MARGIN_SUPPORT_STEPS)
        .max(1);

    (0..topology.arcs().len())
        .map(|index| {
            let cell = CellId::from_raw(index as u32);
            let kind = tectonic
                .crust_kind(cell)
                .expect("tectonic field is cell aligned");
            let thickness = tectonic
                .crust_thickness_for_cell(cell)
                .expect("tectonic field is cell aligned");
            let (margin, interior) = match kind {
                CrustKind::Oceanic => (-200.0, -5_200.0 + thickness * 110.0),
                CrustKind::Continental => (100.0, 250.0 + (thickness - 25.0) * 45.0),
            };
            let blend = if distance[index] == u64::MAX {
                1.0
            } else {
                smooth_rise(distance[index], support)
            };
            (margin + (interior - margin) * blend)
                .clamp(CRUST_BASE_ELEVATION_MIN_M, CRUST_BASE_ELEVATION_MAX_M)
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum EffectClass {
    Collision,
    Trench,
    Arc,
    Rift,
    Ridge,
    Transform,
}

impl EffectClass {
    const COUNT: usize = 6;

    const fn index(self) -> usize {
        self as usize
    }

    const fn support_steps(self) -> u64 {
        match self {
            Self::Collision => 3,
            Self::Trench => 2,
            Self::Arc => 4,
            Self::Rift => 3,
            Self::Ridge => 3,
            Self::Transform => 2,
        }
    }
}

fn synthesize_tectonic_offset(
    spatial: &SpatialSnapshot,
    topology: &NaturalTopologyIndex,
    tectonic: &TectonicSnapshot,
) -> Vec<f32> {
    let mut sources: [BTreeMap<CellId, f32>; EffectClass::COUNT] =
        array::from_fn(|_| BTreeMap::new());
    for edge in spatial.edges() {
        let record = tectonic
            .boundary_for_edge(edge.id)
            .expect("tectonic boundary field is edge aligned");
        let [Some(first), Some(second)] = edge.cells else {
            continue;
        };
        let first_plate = tectonic
            .plate_for_cell(first)
            .expect("tectonic plate field is cell aligned");
        let second_plate = tectonic
            .plate_for_cell(second)
            .expect("tectonic plate field is cell aligned");
        if first_plate == second_plate {
            continue;
        }
        let strength = record.strength;
        match record.kind {
            BoundaryKind::None | BoundaryKind::Weak => {}
            BoundaryKind::ContinentalCollision => {
                insert_source(
                    &mut sources[EffectClass::Collision.index()],
                    first,
                    2_200.0 * strength,
                );
                insert_source(
                    &mut sources[EffectClass::Collision.index()],
                    second,
                    2_200.0 * strength,
                );
            }
            BoundaryKind::Subduction => {
                let subducting = record
                    .subducting_plate
                    .expect("validated subduction has a descending plate");
                let (trench_cell, arc_cell) =
                    orient_subduction(first, second, first_plate, second_plate, subducting);
                insert_source(
                    &mut sources[EffectClass::Trench.index()],
                    trench_cell,
                    -2_800.0 * strength,
                );
                insert_source(
                    &mut sources[EffectClass::Arc.index()],
                    arc_cell,
                    1_800.0 * strength,
                );
            }
            BoundaryKind::ContinentalRift => {
                insert_source(
                    &mut sources[EffectClass::Rift.index()],
                    first,
                    -1_500.0 * strength,
                );
                insert_source(
                    &mut sources[EffectClass::Rift.index()],
                    second,
                    -1_500.0 * strength,
                );
            }
            BoundaryKind::OceanicRidge => {
                insert_source(
                    &mut sources[EffectClass::Ridge.index()],
                    first,
                    1_200.0 * strength,
                );
                insert_source(
                    &mut sources[EffectClass::Ridge.index()],
                    second,
                    1_200.0 * strength,
                );
            }
            BoundaryKind::Transform => {
                let (positive, negative) = if first_plate < second_plate {
                    (first, second)
                } else {
                    (second, first)
                };
                insert_source(
                    &mut sources[EffectClass::Transform.index()],
                    positive,
                    350.0 * strength,
                );
                insert_source(
                    &mut sources[EffectClass::Transform.index()],
                    negative,
                    -350.0 * strength,
                );
            }
        }
    }

    let typical_cost = typical_traversal_cost(topology);
    let mut result = vec![0.0_f32; topology.arcs().len()];
    for class in [
        EffectClass::Collision,
        EffectClass::Trench,
        EffectClass::Arc,
        EffectClass::Rift,
        EffectClass::Ridge,
        EffectClass::Transform,
    ] {
        let class_sources = &sources[class.index()];
        if class_sources.is_empty() {
            continue;
        }
        let cells: Vec<_> = class_sources.keys().copied().collect();
        let amplitudes: Vec<_> = class_sources.values().copied().collect();
        let assignment = multi_source_ownership(topology, &cells);
        let support = typical_cost.saturating_mul(class.support_steps()).max(1);
        for (index, value) in result.iter_mut().enumerate() {
            let owner = assignment.owners[index];
            if owner == u32::MAX {
                continue;
            }
            let kernel = compact_kernel(assignment.distances[index], support);
            *value += amplitudes[owner as usize] * kernel;
        }
    }
    for value in &mut result {
        *value = value.clamp(TECTONIC_OFFSET_MIN_M, TECTONIC_OFFSET_MAX_M);
    }
    result
}

fn orient_subduction(
    first: CellId,
    second: CellId,
    first_plate: PlateId,
    second_plate: PlateId,
    subducting: PlateId,
) -> (CellId, CellId) {
    if first_plate == subducting {
        (first, second)
    } else {
        debug_assert_eq!(second_plate, subducting);
        (second, first)
    }
}

fn insert_source(sources: &mut BTreeMap<CellId, f32>, cell: CellId, amplitude: f32) {
    let stored = sources.entry(cell).or_insert(amplitude);
    if amplitude.abs() > stored.abs() {
        *stored = amplitude;
    }
}

fn synthesize_regional_offset(
    topology: &NaturalTopologyIndex,
    tectonic: &TectonicSnapshot,
    streams: &LabeledSubstreams,
) -> Vec<f32> {
    let mut rng = streams.stream(RELIEF_REGIONAL_LABEL);
    let raw = random_noise(topology.arcs().len(), &mut rng);
    let wide = diffuse(topology, raw.clone(), 12);
    let medium = diffuse(topology, raw.clone(), 5);
    let fine = diffuse(topology, raw, 2);
    let mut result: Vec<_> = (0..topology.arcs().len())
        .map(|index| {
            let combined = (wide[index] * 5 + medium[index] * 3 + fine[index] * 2) as f32
                / (REGIONAL_NOISE_SCALE as f32 * 10.0);
            let amplitude = match tectonic
                .crust_kind(CellId::from_raw(index as u32))
                .expect("tectonic field is cell aligned")
            {
                CrustKind::Oceanic => 300.0,
                CrustKind::Continental => 450.0,
            };
            combined * amplitude
        })
        .collect();
    center_and_bound(&mut result, -800.0, 800.0);
    result
}

fn random_noise(count: usize, rng: &mut impl RngCore) -> Vec<i64> {
    (0..count)
        .map(|_| {
            i64::from(rng.next_u32() % (REGIONAL_NOISE_SCALE as u32 * 2 + 1)) - REGIONAL_NOISE_SCALE
        })
        .collect()
}

fn diffuse(topology: &NaturalTopologyIndex, mut values: Vec<i64>, passes: usize) -> Vec<i64> {
    for _ in 0..passes {
        let previous = values;
        values = topology
            .arcs()
            .iter()
            .enumerate()
            .map(|(index, arcs)| {
                let neighbor_sum: i128 = arcs
                    .iter()
                    .map(|arc| i128::from(previous[arc.neighbor.raw() as usize]))
                    .sum();
                let numerator = i128::from(previous[index]) * 2 + neighbor_sum;
                (numerator / (arcs.len() + 2) as i128) as i64
            })
            .collect();
    }
    values
}

fn center_and_bound(values: &mut [f32], min: f32, max: f32) {
    for _ in 0..2 {
        let mean = values.iter().map(|&value| f64::from(value)).sum::<f64>() / values.len() as f64;
        for value in values.iter_mut() {
            *value = (*value - mean as f32).clamp(min, max);
        }
    }
}

fn reconcile_final_safety(
    crust_base: &mut [f32],
    tectonic_offset: &mut [f32],
    volcanic_offset: &mut [f32],
    regional_offset: &mut [f32],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<f32> {
    let mut elevation = Vec::with_capacity(crust_base.len());
    let mut clamped_count = 0_usize;
    for index in 0..crust_base.len() {
        let raw = crust_base[index]
            + tectonic_offset[index]
            + volcanic_offset[index]
            + regional_offset[index];
        let target = raw.clamp(ELEVATION_MIN_M, ELEVATION_MAX_M);
        if target != raw {
            clamped_count += 1;
            let mut remaining = target - raw;
            if remaining < 0.0 {
                remaining = adjust_component(
                    &mut volcanic_offset[index],
                    remaining,
                    VOLCANIC_OFFSET_MIN_M,
                    VOLCANIC_OFFSET_MAX_M,
                );
            }
            remaining = adjust_component(
                &mut regional_offset[index],
                remaining,
                REGIONAL_OFFSET_MIN_M,
                REGIONAL_OFFSET_MAX_M,
            );
            remaining = adjust_component(
                &mut tectonic_offset[index],
                remaining,
                TECTONIC_OFFSET_MIN_M,
                TECTONIC_OFFSET_MAX_M,
            );
            let _remaining = adjust_component(
                &mut crust_base[index],
                remaining,
                CRUST_BASE_ELEVATION_MIN_M,
                CRUST_BASE_ELEVATION_MAX_M,
            );
            if diagnostics.len() < MAX_CLAMP_DIAGNOSTICS {
                diagnostics.push(
                    Diagnostic::with_context(
                        DiagnosticSeverity::Warning,
                        CLAMP_DIAGNOSTIC_CODE,
                        format!("clamped raw elevation {raw} m to {target} m"),
                        DiagnosticContext {
                            cell_id: Some(CellId::from_raw(index as u32)),
                            ..DiagnosticContext::default()
                        },
                    )
                    .expect("engine-owned relief diagnostic code is valid"),
                );
            }
        }
        elevation.push(
            crust_base[index]
                + tectonic_offset[index]
                + volcanic_offset[index]
                + regional_offset[index],
        );
    }
    if clamped_count > MAX_CLAMP_DIAGNOSTICS {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticSeverity::Warning,
                CLAMP_DIAGNOSTIC_CODE,
                format!(
                    "{} additional cells required bounded elevation reconciliation",
                    clamped_count - MAX_CLAMP_DIAGNOSTICS
                ),
            )
            .expect("engine-owned relief diagnostic code is valid"),
        );
    }
    elevation
}

fn adjust_component(value: &mut f32, delta: f32, min: f32, max: f32) -> f32 {
    let previous = *value;
    *value = (previous + delta).clamp(min, max);
    delta - (*value - previous)
}

fn typical_traversal_cost(topology: &NaturalTopologyIndex) -> u64 {
    let mut costs: Vec<_> = topology
        .arcs()
        .iter()
        .flatten()
        .map(|arc| arc.traversal_cost)
        .collect();
    costs.sort_unstable();
    costs[costs.len() / 2].max(1)
}

fn smooth_rise(distance: u64, support: u64) -> f32 {
    if distance >= support {
        1.0
    } else {
        let t = distance as f32 / support as f32;
        t * t * (3.0 - 2.0 * t)
    }
}

fn compact_kernel(distance: u64, support: u64) -> f32 {
    if distance >= support {
        0.0
    } else {
        let t = 1.0 - distance as f32 / support as f32;
        t * t * (3.0 - 2.0 * t)
    }
}

/// Errors returned when relief inputs or generated fields violate their contracts.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ReliefGenerationError {
    /// The supplied tectonic snapshot is incompatible with the spatial snapshot.
    #[error("invalid tectonic input: {0}")]
    InvalidTectonics(#[from] TectonicValidationError),
    /// The supplied mantle snapshot is incompatible with the spatial snapshot.
    #[error("invalid mantle input: {0}")]
    InvalidMantle(#[from] MantleValidationError),
    /// The closed planar boundary escaped the formal ocean envelope.
    #[error("closed boundary cell {cell:?} has exposed elevation {elevation_m} m")]
    ExposedBoundaryCell {
        /// Boundary cell that reached or exceeded sea level.
        cell: CellId,
        /// Reconciled constructional elevation at the boundary.
        elevation_m: f32,
    },
    /// Generated relief fields violate the relief snapshot contract.
    #[error("invalid generated relief: {0}")]
    InvalidRelief(#[from] ReliefValidationError),
}

#[cfg(test)]
mod tests {
    use super::{compact_kernel, smooth_rise};

    #[test]
    fn compact_polynomials_have_exact_support_endpoints() {
        assert_eq!(compact_kernel(0, 100), 1.0);
        assert_eq!(compact_kernel(100, 100), 0.0);
        assert_eq!(compact_kernel(101, 100), 0.0);
        assert_eq!(smooth_rise(0, 100), 0.0);
        assert_eq!(smooth_rise(100, 100), 1.0);
    }
}
