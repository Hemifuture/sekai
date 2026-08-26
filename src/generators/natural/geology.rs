use std::collections::BTreeMap;

use rand::RngCore;
use thiserror::Error;

use super::random::{LabeledSubstreams, BEDROCK_PROVINCE_LABEL};
use super::topology::{multi_source_ownership, NaturalTopologyIndex};
use crate::engine::StageRng;
use crate::world::natural::{
    BedrockKind, BedrockKindField, BoundaryKind, BoundaryRecord, CrustKind, CrustKindField,
    GeologicSnapshot, GeologicSpec, GeologicSpecError, GeologicValidationError, MantleSnapshot,
    MantleValidationError, PlateIdField, ReliefSnapshot, ReliefValidationError, TectonicSnapshot,
    TectonicValidationError, GEOLOGIC_SNAPSHOT_SCHEMA_V1, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::CellId;

const CLASSIFICATION_QUANTIZATION: f32 = 1_000_000.0;
const PROVINCE_NOISE_SCALE: i64 = 1_000_000;
const VOLCANIC_THRESHOLD: f32 = 0.55;
const METAMORPHIC_THRESHOLD: f32 = 0.45;
const SEDIMENTARY_THRESHOLD: f32 = 0.33;

/// Deterministic synthesis of present-day bedrock, physical properties, and formation potentials.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeologicGenerator;

impl GeologicGenerator {
    /// Generates a current-slice geologic substrate from immutable physical inputs.
    pub fn generate(
        spatial: &SpatialSnapshot,
        tectonic: &TectonicSnapshot,
        mantle: &MantleSnapshot,
        relief: &ReliefSnapshot,
        spec: &GeologicSpec,
        rng: &mut StageRng,
    ) -> Result<GeologicSnapshot, GeologicGenerationError> {
        spec.validate()?;
        tectonic.validate_against(spatial)?;
        mantle.validate_against(spatial)?;
        relief.validate_against(spatial)?;

        let topology = NaturalTopologyIndex::new(spatial);
        let boundary =
            boundary_influences(&topology, tectonic.cell_plates(), tectonic.boundaries());
        let streams = LabeledSubstreams::capture(rng);
        let fields = synthesize_geologic_fields(
            &topology,
            &boundary,
            tectonic.crust_kinds(),
            mantle.volcanic_influence(),
            mantle.heat_flow_mw_m2(),
            relief.tectonic_offset_m().values(),
            relief.elevation_m().values(),
            &streams,
        );

        let snapshot = GeologicSnapshot::new(
            GEOLOGIC_SNAPSHOT_SCHEMA_V1,
            spatial.cell_count() as u32,
            BedrockKindField::from_kinds(fields.bedrock),
            fields.fracture,
            fields.resistance,
            fields.permeability,
            fields.metallic,
            fields.geothermal,
            fields.sedimentary,
        )?;
        snapshot.validate_against(spatial, tectonic, mantle, relief)?;
        Ok(snapshot)
    }
}

#[derive(Debug)]
pub(super) struct BoundaryInfluences {
    active: Vec<f32>,
    collision: Vec<f32>,
    magmatic: Vec<f32>,
}

pub(crate) fn boundary_influences(
    topology: &NaturalTopologyIndex,
    cell_plates: &PlateIdField,
    boundaries: &[BoundaryRecord],
) -> BoundaryInfluences {
    let mut active = BTreeMap::new();
    let mut collision = BTreeMap::new();
    let mut magmatic = BTreeMap::new();

    for (edge_index, &owners) in topology.edge_owners().iter().enumerate() {
        let record = boundaries
            .get(edge_index)
            .expect("validated tectonic boundaries are edge aligned");
        let [Some(first), Some(second)] = owners else {
            continue;
        };
        match record.kind {
            BoundaryKind::None => {}
            BoundaryKind::Weak => {
                insert_source(&mut active, first, record.strength * 0.25);
                insert_source(&mut active, second, record.strength * 0.25);
            }
            BoundaryKind::ContinentalCollision => {
                insert_source(&mut active, first, record.strength);
                insert_source(&mut active, second, record.strength);
                insert_source(&mut collision, first, record.strength);
                insert_source(&mut collision, second, record.strength);
            }
            BoundaryKind::Subduction => {
                insert_source(&mut active, first, record.strength);
                insert_source(&mut active, second, record.strength);
                let descending = record
                    .subducting_plate
                    .expect("validated subduction identifies the descending plate");
                let overriding = if cell_plates.get(first.raw() as usize) == Some(descending) {
                    second
                } else {
                    first
                };
                insert_source(&mut magmatic, overriding, record.strength);
            }
            BoundaryKind::ContinentalRift | BoundaryKind::OceanicRidge => {
                insert_source(&mut active, first, record.strength);
                insert_source(&mut active, second, record.strength);
                insert_source(&mut magmatic, first, record.strength);
                insert_source(&mut magmatic, second, record.strength);
            }
            BoundaryKind::Transform => {
                insert_source(&mut active, first, record.strength);
                insert_source(&mut active, second, record.strength);
            }
        }
    }

    BoundaryInfluences {
        active: spread_sources(topology, &active, 2),
        collision: spread_sources(topology, &collision, 3),
        magmatic: spread_sources(topology, &magmatic, 2),
    }
}

pub(super) struct GeneratedGeologicFields {
    pub(super) bedrock: Vec<BedrockKind>,
    pub(super) fracture: Vec<f32>,
    pub(super) resistance: Vec<f32>,
    pub(super) permeability: Vec<f32>,
    pub(super) metallic: Vec<f32>,
    pub(super) geothermal: Vec<f32>,
    pub(super) sedimentary: Vec<f32>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_geologic_fields(
    topology: &NaturalTopologyIndex,
    boundary: &BoundaryInfluences,
    crust_kinds: &CrustKindField,
    volcanic_influence: &[f32],
    heat_flow_mw_m2: &[f32],
    tectonic_offset_m: &[f32],
    elevation_m: &[f32],
    streams: &LabeledSubstreams,
) -> GeneratedGeologicFields {
    let province = province_field(topology, streams);
    let local_low = local_relative_low(topology, elevation_m);
    let cell_count = topology.arcs().len();
    let mut bedrock = Vec::with_capacity(cell_count);
    let mut fracture = Vec::with_capacity(cell_count);
    let mut resistance = Vec::with_capacity(cell_count);
    let mut permeability = Vec::with_capacity(cell_count);
    let mut metallic = Vec::with_capacity(cell_count);
    let mut geothermal = Vec::with_capacity(cell_count);
    let mut sedimentary = Vec::with_capacity(cell_count);

    for index in 0..cell_count {
        let volcanic = quantize_unit(volcanic_influence[index].max(boundary.magmatic[index]));
        let metamorphic = quantize_unit(boundary.collision[index]);
        let active = boundary.active[index];
        let subsidence = (-tectonic_offset_m[index] / 1_500.0).clamp(0.0, 1.0);
        let province_tendency = (0.5 + 0.5 * province[index]).clamp(0.0, 1.0);
        let basin = quantize_unit(
            (0.55 * subsidence + 0.25 * local_low[index] + 0.20 * province_tendency)
                * (1.0 - 0.55 * active),
        );
        let crust = crust_kinds
            .get(index)
            .expect("validated tectonic fields are spatially aligned");
        let kind = if volcanic >= VOLCANIC_THRESHOLD {
            BedrockKind::Volcanic
        } else if crust == CrustKind::Continental && metamorphic >= METAMORPHIC_THRESHOLD {
            BedrockKind::Metamorphic
        } else if basin >= SEDIMENTARY_THRESHOLD {
            BedrockKind::Sedimentary
        } else {
            match crust {
                CrustKind::Oceanic => BedrockKind::OceanicMafic,
                CrustKind::Continental => BedrockKind::ContinentalCrystalline,
            }
        };

        let fractured =
            quantize_unit(1.0 - (1.0 - active) * (1.0 - 0.45 * volcanic_influence[index]));
        let (base_resistance, base_permeability) = category_properties(kind);
        let erosion = quantize_unit((base_resistance - 0.30 * fractured).clamp(0.0, 1.0));
        let permeable = quantize_unit(
            (base_permeability + 0.55 * fractured * (1.0 - base_permeability)).clamp(0.0, 1.0),
        );
        let normalized_heat = ((heat_flow_mw_m2[index] - HEAT_FLOW_MIN_MW_M2)
            / (HEAT_FLOW_MAX_MW_M2 - HEAT_FLOW_MIN_MW_M2))
            .clamp(0.0, 1.0);
        let geothermal_value = quantize_unit(normalized_heat * (0.45 + 0.55 * fractured));
        let metallic_value = quantize_unit(
            1.0 - (1.0 - 0.92 * volcanic) * (1.0 - 0.78 * metamorphic) * (1.0 - 0.40 * fractured),
        );
        let sedimentary_class = if kind == BedrockKind::Sedimentary {
            1.0
        } else {
            0.0
        };
        let sedimentary_value =
            quantize_unit(0.55 * basin + 0.35 * sedimentary_class + 0.10 * (1.0 - active));

        bedrock.push(kind);
        fracture.push(fractured);
        resistance.push(erosion);
        permeability.push(permeable);
        metallic.push(metallic_value);
        geothermal.push(geothermal_value);
        sedimentary.push(sedimentary_value);
    }

    GeneratedGeologicFields {
        bedrock,
        fracture,
        resistance,
        permeability,
        metallic,
        geothermal,
        sedimentary,
    }
}

fn insert_source(sources: &mut BTreeMap<CellId, f32>, cell: CellId, strength: f32) {
    let stored = sources.entry(cell).or_insert(strength);
    *stored = stored.max(strength);
}

fn spread_sources(
    topology: &NaturalTopologyIndex,
    sources: &BTreeMap<CellId, f32>,
    support_steps: u64,
) -> Vec<f32> {
    if sources.is_empty() {
        return vec![0.0; topology.arcs().len()];
    }
    let cells: Vec<_> = sources.keys().copied().collect();
    let strengths: Vec<_> = sources.values().copied().collect();
    let assignment = multi_source_ownership(topology, &cells);
    let support = typical_traversal_cost(topology)
        .saturating_mul(support_steps)
        .max(1);
    assignment
        .owners
        .iter()
        .zip(assignment.distances)
        .map(|(&owner, distance)| {
            if owner == u32::MAX {
                0.0
            } else {
                quantize_unit(strengths[owner as usize] * compact_kernel(distance, support))
            }
        })
        .collect()
}

fn province_field(topology: &NaturalTopologyIndex, streams: &LabeledSubstreams) -> Vec<f32> {
    let mut rng = streams.stream(BEDROCK_PROVINCE_LABEL);
    let mut values: Vec<i64> = (0..topology.arcs().len())
        .map(|_| {
            i64::from(rng.next_u32() % (PROVINCE_NOISE_SCALE as u32 * 2 + 1)) - PROVINCE_NOISE_SCALE
        })
        .collect();
    for _ in 0..4 {
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
                let numerator = i128::from(previous[index]) * 3 + neighbor_sum;
                (numerator / (arcs.len() + 3) as i128) as i64
            })
            .collect();
    }
    let mean =
        values.iter().map(|&value| i128::from(value)).sum::<i128>() / values.len().max(1) as i128;
    values
        .into_iter()
        .map(|value| {
            ((value as f64 - mean as f64) / PROVINCE_NOISE_SCALE as f64).clamp(-1.0, 1.0) as f32
        })
        .collect()
}

fn local_relative_low(topology: &NaturalTopologyIndex, elevation: &[f32]) -> Vec<f32> {
    topology
        .arcs()
        .iter()
        .enumerate()
        .map(|(index, arcs)| {
            if arcs.is_empty() {
                return 0.0;
            }
            let neighbor_mean = arcs
                .iter()
                .map(|arc| elevation[arc.neighbor.raw() as usize] as f64)
                .sum::<f64>()
                / arcs.len() as f64;
            ((neighbor_mean as f32 - elevation[index]) / 1_200.0).clamp(0.0, 1.0)
        })
        .collect()
}

const fn category_properties(kind: BedrockKind) -> (f32, f32) {
    match kind {
        BedrockKind::OceanicMafic => (0.78, 0.18),
        BedrockKind::ContinentalCrystalline => (0.86, 0.12),
        BedrockKind::Sedimentary => (0.42, 0.58),
        BedrockKind::Metamorphic => (0.82, 0.10),
        BedrockKind::Volcanic => (0.68, 0.24),
    }
}

fn typical_traversal_cost(topology: &NaturalTopologyIndex) -> u64 {
    let mut costs: Vec<_> = topology
        .arcs()
        .iter()
        .flatten()
        .map(|arc| arc.traversal_cost)
        .collect();
    costs.sort_unstable();
    costs.get(costs.len() / 2).copied().unwrap_or(1).max(1)
}

fn compact_kernel(distance: u64, support: u64) -> f32 {
    if distance >= support {
        0.0
    } else {
        let t = 1.0 - distance as f32 / support as f32;
        t * t * (3.0 - 2.0 * t)
    }
}

fn quantize_unit(value: f32) -> f32 {
    ((value.clamp(0.0, 1.0) * CLASSIFICATION_QUANTIZATION).round() / CLASSIFICATION_QUANTIZATION)
        .clamp(0.0, 1.0)
}

/// Errors returned when geologic inputs or generated fields violate their contracts.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GeologicGenerationError {
    /// The supplied geologic specification is invalid.
    #[error("invalid geologic specification: {0}")]
    InvalidSpec(#[from] GeologicSpecError),
    /// The tectonic snapshot is incompatible with the spatial topology.
    #[error("invalid tectonic input: {0}")]
    InvalidTectonics(#[from] TectonicValidationError),
    /// The mantle snapshot is incompatible with the spatial topology.
    #[error("invalid mantle input: {0}")]
    InvalidMantle(#[from] MantleValidationError),
    /// The relief snapshot is incompatible with the spatial topology.
    #[error("invalid relief input: {0}")]
    InvalidRelief(#[from] ReliefValidationError),
    /// Generated geologic fields violate the immutable domain contract.
    #[error("invalid generated geology: {0}")]
    InvalidGeology(#[from] GeologicValidationError),
}
