#![cfg_attr(not(test), allow(dead_code))]

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::engine::StageRng;

pub(super) const PLATE_SEEDS_LABEL: &str = "plate-seeds-v1";
pub(super) const PLATE_MOTION_LABEL: &str = "plate-motion-v1";
pub(super) const CRUST_SEEDS_LABEL: &str = "crust-seeds-v1";
pub(super) const CRUST_SHAPE_LABEL: &str = "crust-shape-v1";
pub(super) const CRUST_THICKNESS_LABEL: &str = "crust-thickness-v1";
pub(super) const RELIEF_REGIONAL_LABEL: &str = "relief-regional-v1";
pub(super) const RELIEF_TECTONIC_DETAIL_LABEL: &str = "relief-tectonic-detail-v1";
pub(super) const RELIEF_HOTSPOT_MORPHOLOGY_LABEL: &str = "relief-hotspot-morphology-v1";
pub(super) const RELIEF_ISLAND_ARC_LABEL: &str = "relief-island-arc-v1";
pub(super) const HOTSPOT_SEEDS_LABEL: &str = "hotspot-seeds-v1";
pub(super) const HOTSPOT_STRENGTH_LABEL: &str = "hotspot-strength-v1";
pub(super) const BEDROCK_PROVINCE_LABEL: &str = "bedrock-province-v1";
pub(super) const PLATE_TARGET_AREA_LABEL: &str = "plate-target-area-v2";
pub(super) const PLATE_SEED_PLACEMENT_LABEL: &str = "plate-seed-placement-v2";
pub(super) const PLATE_RESISTANCE_FIELD_LABEL: &str = "plate-resistance-field-v2";
pub(super) const PLATE_FABRIC_FIELD_LABEL: &str = "plate-fabric-field-v2";
pub(super) const CRUST_ANCHOR_LAYOUT_LABEL: &str = "crust-anchor-layout-v2";
pub(super) const CRUST_AFFINITY_FIELD_LABEL: &str = "crust-affinity-field-v2";
pub(super) const CRUST_THICKNESS_FIELD_LABEL: &str = "crust-thickness-field-v2";
pub(super) const INITIAL_PLATES_V3_LABEL: &str = "initial-plates-v3";
pub(super) const INITIAL_CRUST_V3_LABEL: &str = "initial-crust-v3";
pub(super) const PLATE_MOTION_V3_LABEL: &str = "plate-motion-v3";
pub(super) const RIFT_EVENTS_V3_LABEL: &str = "rift-events-v3";
pub(super) const PROCESS_VARIATION_V3_LABEL: &str = "process-variation-v3";
pub(super) const OROGENIC_DETAIL_V3_LABEL: &str = "orogenic-detail-v3";
pub(super) const OCEANIC_DETAIL_V3_LABEL: &str = "oceanic-detail-v3";
pub(super) const INITIAL_DOMAINS_V5_LABEL: &str = "initial-domains-v5";
pub(super) const MECHANICAL_FRAGMENTATION_V5_LABEL: &str = "mechanical-fragmentation-v5";
pub(super) const SPHERICAL_MORPHOLOGY_LABELS: [&str; 7] = [
    PLATE_TARGET_AREA_LABEL,
    PLATE_SEED_PLACEMENT_LABEL,
    PLATE_RESISTANCE_FIELD_LABEL,
    PLATE_FABRIC_FIELD_LABEL,
    CRUST_ANCHOR_LAYOUT_LABEL,
    CRUST_AFFINITY_FIELD_LABEL,
    CRUST_THICKNESS_FIELD_LABEL,
];
pub(super) const SPHERICAL_TECTONIC_V3_LABELS: [&str; 7] = [
    INITIAL_PLATES_V3_LABEL,
    INITIAL_CRUST_V3_LABEL,
    PLATE_MOTION_V3_LABEL,
    RIFT_EVENTS_V3_LABEL,
    PROCESS_VARIATION_V3_LABEL,
    OROGENIC_DETAIL_V3_LABEL,
    OCEANIC_DETAIL_V3_LABEL,
];

pub(super) struct LabeledSubstreams {
    root: [u8; 32],
}

impl LabeledSubstreams {
    pub(super) fn capture(stage_rng: &mut StageRng) -> Self {
        let mut root = [0_u8; 32];
        stage_rng.fill_bytes(&mut root);
        Self { root }
    }

    pub(super) fn stream(&self, label: &'static str) -> ChaCha8Rng {
        debug_assert!(
            !label.is_empty()
                && label.is_ascii()
                && label.bytes().all(|byte| byte.is_ascii_graphic()),
            "natural RNG labels must be non-empty printable ASCII constants"
        );
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai-natural-substream-v1\0");
        hasher.update(&self.root);
        hasher.update(&(label.len() as u64).to_le_bytes());
        hasher.update(label.as_bytes());
        ChaCha8Rng::from_seed(*hasher.finalize().as_bytes())
    }

    /// Returns a deterministic event value without consuming any mutable stream.
    pub(super) fn counter_u64(&self, label: &'static str, coordinates: &[u64]) -> u64 {
        debug_assert!(
            !label.is_empty()
                && label.is_ascii()
                && label.bytes().all(|byte| byte.is_ascii_graphic()),
            "natural RNG labels must be non-empty printable ASCII constants"
        );
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai-natural-counter-v1\0");
        hasher.update(&self.root);
        hasher.update(&(label.len() as u64).to_le_bytes());
        hasher.update(label.as_bytes());
        hasher.update(&(coordinates.len() as u64).to_le_bytes());
        for coordinate in coordinates {
            hasher.update(&coordinate.to_le_bytes());
        }
        u64::from_le_bytes(hasher.finalize().as_bytes()[..8].try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use rand::RngCore;

    use super::{
        LabeledSubstreams, BEDROCK_PROVINCE_LABEL, CRUST_SEEDS_LABEL, HOTSPOT_SEEDS_LABEL,
        HOTSPOT_STRENGTH_LABEL, INITIAL_CRUST_V3_LABEL, INITIAL_PLATES_V3_LABEL,
        OCEANIC_DETAIL_V3_LABEL, OROGENIC_DETAIL_V3_LABEL, PLATE_MOTION_LABEL,
        PLATE_MOTION_V3_LABEL, PLATE_SEEDS_LABEL, PROCESS_VARIATION_V3_LABEL,
        RELIEF_HOTSPOT_MORPHOLOGY_LABEL, RELIEF_ISLAND_ARC_LABEL, RELIEF_REGIONAL_LABEL,
        RELIEF_TECTONIC_DETAIL_LABEL, RIFT_EVENTS_V3_LABEL, SPHERICAL_MORPHOLOGY_LABELS,
        SPHERICAL_TECTONIC_V3_LABELS,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::world::RootSeed;

    fn stage_rng() -> StageRng {
        StageRng::from_seed(derive_stage_seed(
            RootSeed::new(71),
            StageIdentity::new("natural-test", 1, "sekai.test"),
        ))
    }

    #[test]
    fn labeled_substreams_repeat_exactly() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut first = streams.stream(PLATE_SEEDS_LABEL);
        let mut second = streams.stream(PLATE_SEEDS_LABEL);

        assert_eq!(
            (0..8).map(|_| first.next_u64()).collect::<Vec<_>>(),
            (0..8).map(|_| second.next_u64()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn consuming_one_label_does_not_perturb_another() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut plate = streams.stream(PLATE_SEEDS_LABEL);
        let mut crust_after_plate = streams.stream(CRUST_SEEDS_LABEL);
        for _ in 0..100 {
            plate.next_u64();
        }

        let pristine = LabeledSubstreams::capture(&mut stage_rng());
        let mut pristine_crust = pristine.stream(CRUST_SEEDS_LABEL);
        assert_eq!(
            (0..8)
                .map(|_| crust_after_plate.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_crust.next_u64())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn capture_consumes_exactly_one_32_byte_seed() {
        let mut actual = stage_rng();
        LabeledSubstreams::capture(&mut actual);

        let mut expected = stage_rng();
        let mut seed = [0_u8; 32];
        expected.fill_bytes(&mut seed);

        assert_eq!(actual.next_u64(), expected.next_u64());
    }

    #[test]
    fn hotspot_seed_consumption_cannot_perturb_strengths() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut seeds = streams.stream(HOTSPOT_SEEDS_LABEL);
        let mut strengths_after_seeds = streams.stream(HOTSPOT_STRENGTH_LABEL);
        for _ in 0..100 {
            seeds.next_u64();
        }

        let pristine = LabeledSubstreams::capture(&mut stage_rng());
        let mut pristine_strengths = pristine.stream(HOTSPOT_STRENGTH_LABEL);
        assert_eq!(
            (0..8)
                .map(|_| strengths_after_seeds.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_strengths.next_u64())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mantle_substreams_cannot_perturb_bedrock_provinces() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut hotspots = streams.stream(HOTSPOT_SEEDS_LABEL);
        let mut provinces_after_hotspots = streams.stream(BEDROCK_PROVINCE_LABEL);
        for _ in 0..100 {
            hotspots.next_u64();
        }

        let pristine = LabeledSubstreams::capture(&mut stage_rng());
        let mut pristine_provinces = pristine.stream(BEDROCK_PROVINCE_LABEL);
        assert_eq!(
            (0..8)
                .map(|_| provinces_after_hotspots.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_provinces.next_u64())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn hotspot_morphology_cannot_perturb_existing_relief_streams() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut morphology = streams.stream(RELIEF_HOTSPOT_MORPHOLOGY_LABEL);
        let mut regional_after_morphology = streams.stream(RELIEF_REGIONAL_LABEL);
        let mut detail_after_morphology = streams.stream(RELIEF_TECTONIC_DETAIL_LABEL);
        for _ in 0..100 {
            morphology.next_u64();
        }

        let pristine = LabeledSubstreams::capture(&mut stage_rng());
        let mut pristine_regional = pristine.stream(RELIEF_REGIONAL_LABEL);
        let mut pristine_detail = pristine.stream(RELIEF_TECTONIC_DETAIL_LABEL);
        assert_eq!(
            (0..8)
                .map(|_| regional_after_morphology.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_regional.next_u64())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            (0..8)
                .map(|_| detail_after_morphology.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_detail.next_u64())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn island_arc_morphology_cannot_perturb_existing_relief_streams() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut island_arcs = streams.stream(RELIEF_ISLAND_ARC_LABEL);
        let mut regional_after_arcs = streams.stream(RELIEF_REGIONAL_LABEL);
        let mut detail_after_arcs = streams.stream(RELIEF_TECTONIC_DETAIL_LABEL);
        for _ in 0..100 {
            island_arcs.next_u64();
        }

        let pristine = LabeledSubstreams::capture(&mut stage_rng());
        let mut pristine_regional = pristine.stream(RELIEF_REGIONAL_LABEL);
        let mut pristine_detail = pristine.stream(RELIEF_TECTONIC_DETAIL_LABEL);
        assert_eq!(
            (0..8)
                .map(|_| regional_after_arcs.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_regional.next_u64())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            (0..8)
                .map(|_| detail_after_arcs.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_detail.next_u64())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn spherical_morphology_substreams_are_pairwise_orthogonal() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let expected = {
            let mut motion = streams.stream(PLATE_MOTION_LABEL);
            (0..8).map(|_| motion.next_u64()).collect::<Vec<_>>()
        };

        for label in SPHERICAL_MORPHOLOGY_LABELS {
            let mut morphology = streams.stream(label);
            for _ in 0..100 {
                morphology.next_u64();
            }
        }

        let mut motion = streams.stream(PLATE_MOTION_LABEL);
        assert_eq!(
            expected,
            (0..8).map(|_| motion.next_u64()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn evolved_tectonic_substreams_are_pairwise_orthogonal() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let expected = SPHERICAL_TECTONIC_V3_LABELS.map(|label| {
            let mut stream = streams.stream(label);
            (0..8).map(|_| stream.next_u64()).collect::<Vec<_>>()
        });

        for consumed in SPHERICAL_TECTONIC_V3_LABELS {
            let mut stream = streams.stream(consumed);
            for _ in 0..100 {
                stream.next_u64();
            }
        }

        for (label, expected) in SPHERICAL_TECTONIC_V3_LABELS.into_iter().zip(expected) {
            let mut stream = streams.stream(label);
            assert_eq!(
                expected,
                (0..8).map(|_| stream.next_u64()).collect::<Vec<_>>()
            );
        }

        assert_eq!(
            SPHERICAL_TECTONIC_V3_LABELS,
            [
                INITIAL_PLATES_V3_LABEL,
                INITIAL_CRUST_V3_LABEL,
                PLATE_MOTION_V3_LABEL,
                RIFT_EVENTS_V3_LABEL,
                PROCESS_VARIATION_V3_LABEL,
                OROGENIC_DETAIL_V3_LABEL,
                OCEANIC_DETAIL_V3_LABEL,
            ]
        );
    }

    #[test]
    fn counter_values_repeat_without_consuming_or_aliasing_coordinates() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let before = {
            let mut stream = streams.stream(PLATE_MOTION_V3_LABEL);
            (0..8).map(|_| stream.next_u64()).collect::<Vec<_>>()
        };
        let value = streams.counter_u64(RIFT_EVENTS_V3_LABEL, &[17, 3, 9]);

        assert_eq!(
            value,
            streams.counter_u64(RIFT_EVENTS_V3_LABEL, &[17, 3, 9])
        );
        assert_ne!(
            value,
            streams.counter_u64(RIFT_EVENTS_V3_LABEL, &[17, 3, 10])
        );
        assert_ne!(
            value,
            streams.counter_u64(PROCESS_VARIATION_V3_LABEL, &[17, 3, 9])
        );
        let mut after = streams.stream(PLATE_MOTION_V3_LABEL);
        assert_eq!(before, (0..8).map(|_| after.next_u64()).collect::<Vec<_>>());
    }
}
