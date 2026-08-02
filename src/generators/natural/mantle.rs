use rand::RngCore;
use thiserror::Error;

use super::random::{LabeledSubstreams, HOTSPOT_SEEDS_LABEL, HOTSPOT_STRENGTH_LABEL};
use super::topology::{farthest_point_seeds, multi_source_distance, NaturalTopologyIndex};
use crate::engine::StageRng;
use crate::world::natural::{
    GeologicSpec, GeologicSpecError, Hotspot, MantleActivity, MantleFormationBias, MantleSnapshot,
    MantleValidationError, MANTLE_SNAPSHOT_SCHEMA_V1, MAX_HOTSPOT_COUNT,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::{HotspotId, Meters};

const BACKGROUND_HEAT_FLOW: [f32; 3] = [45.0, 65.0, 85.0];
const HOTSPOT_ANOMALY_MAX: [f32; 3] = [160.0, 220.0, 280.0];
const HOTSPOT_RADIUS_SHORT_SIDE: [f64; 3] = [0.04, 0.055, 0.07];

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
        let (hotspot_count, mantle_activity) = match formation_bias {
            MantleFormationBias::Neutral => (spec.hotspot_count, spec.mantle_activity),
            MantleFormationBias::VolcanicIslands => (
                spec.hotspot_count.clamp(9, MAX_HOTSPOT_COUNT),
                MantleActivity::Active,
            ),
        };
        if usize::from(hotspot_count) > spatial.cell_count() {
            return Err(MantleGenerationError::HotspotCountExceedsCells {
                hotspots: hotspot_count,
                cells: spatial.cell_count(),
            });
        }

        let activity_index = activity_index(mantle_activity);
        let streams = LabeledSubstreams::capture(rng);
        let topology = NaturalTopologyIndex::new(spatial);
        let mut seed_rng = streams.stream(HOTSPOT_SEEDS_LABEL);
        let sources =
            farthest_point_seeds(&topology, usize::from(hotspot_count), seed_rng.next_u64());
        let mut strength_rng = streams.stream(HOTSPOT_STRENGTH_LABEL);
        let short_side_m = spatial
            .bounds()
            .width()
            .get()
            .min(spatial.bounds().height().get());

        let mut hotspots = Vec::with_capacity(sources.len());
        for (index, source_cell) in sources.into_iter().enumerate() {
            let strength_permille = 650 + (strength_rng.next_u32() % 351) as u16;
            let normalized_strength = f64::from(strength_permille - 650) / 350.0;
            let radius_multiplier = 0.8 + 0.4 * normalized_strength;
            let support_radius_m =
                short_side_m * HOTSPOT_RADIUS_SHORT_SIDE[activity_index] * radius_multiplier;
            hotspots.push(
                Hotspot::new(
                    HotspotId::from_raw(index as u32),
                    source_cell,
                    strength_permille,
                    Meters::new(support_radius_m)
                        .expect("validated spatial dimensions produce a finite radius"),
                )
                .map_err(MantleGenerationError::InvalidSnapshot)?,
            );
        }

        let mut volcanic_influence = vec![0.0_f32; spatial.cell_count()];
        for hotspot in &hotspots {
            let support_distance =
                topology.quantized_distance_for_meters(hotspot.support_radius_m().get());
            let distances =
                multi_source_distance(&topology, &[hotspot.source_cell()], Some(support_distance));
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
        let snapshot = MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            spatial.cell_count() as u32,
            hotspots,
            heat_flow_mw_m2,
            volcanic_influence,
        )
        .map_err(MantleGenerationError::InvalidSnapshot)?;
        snapshot
            .validate_against(spatial)
            .map_err(MantleGenerationError::InvalidSnapshot)?;
        Ok(snapshot)
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
