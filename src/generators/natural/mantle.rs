use rand::RngCore;
use thiserror::Error;

use super::random::{LabeledSubstreams, HOTSPOT_SEEDS_LABEL, HOTSPOT_STRENGTH_LABEL};
use super::topology::{
    farthest_point_seeds, farthest_point_seeds_from_candidates, multi_source_distance,
    NaturalTopologyIndex,
};
use crate::engine::StageRng;
use crate::world::natural::{
    GeologicSpec, GeologicSpecError, Hotspot, MantleActivity, MantleFormationBias, MantleSnapshot,
    MantleValidationError, MANTLE_SNAPSHOT_SCHEMA_V1, MAX_HOTSPOT_COUNT,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::{CellId, HotspotId, Meters};

const BACKGROUND_HEAT_FLOW: [f32; 3] = [45.0, 65.0, 85.0];
const HOTSPOT_ANOMALY_MAX: [f32; 3] = [160.0, 220.0, 280.0];
// Fractions of the domain reference length: planar short side or spherical
// half-circumference. Strength expands each nominal support by 0.8..=1.2.
const HOTSPOT_RADIUS_REFERENCE_SCALE: [f64; 3] = [0.04, 0.055, 0.07];
// In the planar policy, the strongest active support reaches 8.4% of the
// short side. A 10% graph inset keeps it away from the artificial map edge.
const HOTSPOT_SOURCE_MARGIN_SHORT_SIDE: f64 = 0.10;

/// Deterministic current-slice mantle forcing independent of tectonic state.
#[derive(Debug, Clone, Copy, Default)]
pub struct MantleGenerator;

impl MantleGenerator {
    /// Generates present-day hotspots, heat flow, and volcanic influence.
    pub fn generate(
        spatial: &SpatialSnapshot,
        spec: &GeologicSpec,
        formation_bias: MantleFormationBias,
        rng: &mut StageRng,
    ) -> Result<MantleSnapshot, MantleGenerationError> {
        spec.validate()?;
        let (hotspot_count, mantle_activity) = resolve_mantle_profile(spec, formation_bias);
        if usize::from(hotspot_count) > spatial.cell_count() {
            return Err(MantleGenerationError::HotspotCountExceedsCells {
                hotspots: hotspot_count,
                cells: spatial.cell_count(),
            });
        }

        let streams = LabeledSubstreams::capture(rng);
        let topology = NaturalTopologyIndex::new(spatial);
        let mut seed_rng = streams.stream(HOTSPOT_SEEDS_LABEL);
        let sources =
            select_hotspot_sources(&topology, usize::from(hotspot_count), seed_rng.next_u64());
        let short_side_m = spatial
            .bounds()
            .width()
            .get()
            .min(spatial.bounds().height().get());
        let fields =
            generate_mantle_fields(&topology, sources, mantle_activity, short_side_m, &streams)
                .map_err(MantleGenerationError::InvalidSnapshot)?;
        let snapshot = MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            spatial.cell_count() as u32,
            fields.hotspots,
            fields.heat_flow_mw_m2,
            fields.volcanic_influence,
        )
        .map_err(MantleGenerationError::InvalidSnapshot)?;
        snapshot
            .validate_against(spatial)
            .map_err(MantleGenerationError::InvalidSnapshot)?;
        Ok(snapshot)
    }
}

pub(super) fn resolve_mantle_profile(
    spec: &GeologicSpec,
    formation_bias: MantleFormationBias,
) -> (u16, MantleActivity) {
    match formation_bias {
        MantleFormationBias::Neutral => (spec.hotspot_count, spec.mantle_activity),
        MantleFormationBias::VolcanicIslands => (
            spec.hotspot_count.clamp(9, MAX_HOTSPOT_COUNT),
            MantleActivity::Active,
        ),
    }
}

pub(super) struct MantleFields {
    pub(super) hotspots: Vec<Hotspot>,
    pub(super) heat_flow_mw_m2: Vec<f32>,
    pub(super) volcanic_influence: Vec<f32>,
}

pub(super) fn generate_mantle_fields(
    topology: &NaturalTopologyIndex,
    sources: Vec<CellId>,
    mantle_activity: MantleActivity,
    support_length_scale_m: f64,
    streams: &LabeledSubstreams,
) -> Result<MantleFields, MantleValidationError> {
    let activity_index = activity_index(mantle_activity);
    let mut strength_rng = streams.stream(HOTSPOT_STRENGTH_LABEL);
    let mut hotspots = Vec::with_capacity(sources.len());
    for (index, source_cell) in sources.into_iter().enumerate() {
        let strength_permille = 650 + (strength_rng.next_u32() % 351) as u16;
        let normalized_strength = f64::from(strength_permille - 650) / 350.0;
        let radius_multiplier = 0.8 + 0.4 * normalized_strength;
        let support_radius_m = support_length_scale_m
            * HOTSPOT_RADIUS_REFERENCE_SCALE[activity_index]
            * radius_multiplier;
        hotspots.push(Hotspot::new(
            HotspotId::from_raw(index as u32),
            source_cell,
            strength_permille,
            Meters::new(support_radius_m)
                .expect("validated world dimensions produce a finite support radius"),
        )?);
    }

    let mut volcanic_influence = vec![0.0_f32; topology.arcs().len()];
    for hotspot in &hotspots {
        let support_distance =
            topology.quantized_distance_for_meters(hotspot.support_radius_m().get());
        let distances =
            multi_source_distance(topology, &[hotspot.source_cell()], Some(support_distance));
        for (combined, distance) in volcanic_influence.iter_mut().zip(distances) {
            if distance == u64::MAX || distance > support_distance {
                continue;
            }
            let normalized = distance as f64 / support_distance as f64;
            let individual = compact_smoothstep(normalized) as f32;
            *combined = (1.0 - (1.0 - *combined) * (1.0 - individual)).clamp(0.0, 1.0);
        }
    }

    let background = BACKGROUND_HEAT_FLOW[activity_index];
    let anomaly = HOTSPOT_ANOMALY_MAX[activity_index];
    let heat_flow_mw_m2 = volcanic_influence
        .iter()
        .map(|&influence| (background + anomaly * influence).clamp(20.0, 400.0))
        .collect();
    Ok(MantleFields {
        hotspots,
        heat_flow_mw_m2,
        volcanic_influence,
    })
}

fn select_hotspot_sources(
    topology: &NaturalTopologyIndex,
    count: usize,
    tie_rotation: u64,
) -> Vec<CellId> {
    let boundary_sources = topology
        .boundary_cells()
        .iter()
        .enumerate()
        .filter_map(|(index, &is_boundary)| is_boundary.then_some(CellId::from_raw(index as u32)))
        .collect::<Vec<_>>();
    let boundary_distance = multi_source_distance(topology, &boundary_sources, None);
    let margin = topology.quantized_short_side_fraction(HOTSPOT_SOURCE_MARGIN_SHORT_SIDE);
    let margin_candidates = boundary_distance
        .iter()
        .enumerate()
        .filter_map(|(index, &distance)| {
            (distance >= margin).then_some(CellId::from_raw(index as u32))
        })
        .collect::<Vec<_>>();
    if margin_candidates.len() >= count {
        return farthest_point_seeds_from_candidates(
            topology,
            &margin_candidates,
            count,
            tie_rotation,
        );
    }

    let interior_candidates = topology
        .boundary_cells()
        .iter()
        .enumerate()
        .filter_map(|(index, &is_boundary)| {
            (!is_boundary).then_some(CellId::from_raw(index as u32))
        })
        .collect::<Vec<_>>();
    if interior_candidates.len() >= count {
        farthest_point_seeds_from_candidates(topology, &interior_candidates, count, tie_rotation)
    } else {
        farthest_point_seeds(topology, count, tie_rotation)
    }
}

const fn activity_index(activity: MantleActivity) -> usize {
    match activity {
        MantleActivity::Quiet => 0,
        MantleActivity::Moderate => 1,
        MantleActivity::Active => 2,
    }
}

fn compact_smoothstep(normalized_distance: f64) -> f64 {
    let t = normalized_distance.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// Errors returned while generating current-slice mantle forcing.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum MantleGenerationError {
    /// The supplied geologic specification is invalid.
    #[error("invalid geologic specification: {0}")]
    InvalidSpec(#[from] GeologicSpecError),
    /// The requested hotspot cardinality exceeds the spatial allocation.
    #[error("hotspot count {hotspots} exceeds spatial cell count {cells}")]
    HotspotCountExceedsCells {
        /// The requested hotspot count.
        hotspots: u16,
        /// The available spatial cell count.
        cells: usize,
    },
    /// Generated mantle forcing failed its immutable domain contract.
    #[error("generated mantle snapshot is invalid: {0}")]
    InvalidSnapshot(MantleValidationError),
}
