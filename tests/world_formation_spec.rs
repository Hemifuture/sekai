use sekai::world::natural::{
    MantleFormationBias, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    WorldFormationPreset, WorldFormationSpec, WorldFormationSpecError,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1, WORLD_FORMATION_SPEC_SCHEMA_V1,
};

const CONCRETE_PRESETS: [ResolvedWorldFormationPreset; 5] = [
    ResolvedWorldFormationPreset::Continents,
    ResolvedWorldFormationPreset::Archipelago,
    ResolvedWorldFormationPreset::Supercontinent,
    ResolvedWorldFormationPreset::GreatIsland,
    ResolvedWorldFormationPreset::VolcanicIslands,
];

#[test]
fn default_spec_requests_named_multi_continents() {
    let spec = WorldFormationSpec::default();

    assert_eq!(spec.schema_version, WORLD_FORMATION_SPEC_SCHEMA_V1);
    assert_eq!(spec.preset, WorldFormationPreset::Continents);
    spec.validate().unwrap();
}

#[test]
fn requested_and_resolved_presets_have_stable_json_names() {
    let requested = [
        (WorldFormationPreset::Random, "\"Random\""),
        (WorldFormationPreset::Continents, "\"Continents\""),
        (WorldFormationPreset::Archipelago, "\"Archipelago\""),
        (WorldFormationPreset::Supercontinent, "\"Supercontinent\""),
        (WorldFormationPreset::GreatIsland, "\"GreatIsland\""),
        (WorldFormationPreset::VolcanicIslands, "\"VolcanicIslands\""),
    ];
    for (preset, expected) in requested {
        assert_eq!(serde_json::to_string(&preset).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<WorldFormationPreset>(expected).unwrap(),
            preset
        );
    }

    for preset in CONCRETE_PRESETS {
        let encoded = serde_json::to_string(&preset).unwrap();
        assert_eq!(
            serde_json::from_str::<ResolvedWorldFormationPreset>(&encoded).unwrap(),
            preset
        );
    }
}

#[test]
fn unsupported_spec_schema_is_rejected_during_validation_and_deserialization() {
    let invalid = WorldFormationSpec {
        schema_version: WORLD_FORMATION_SPEC_SCHEMA_V1 + 1,
        preset: WorldFormationPreset::Continents,
    };
    assert!(matches!(
        invalid.validate(),
        Err(WorldFormationSpecError::UnsupportedSpecSchema { .. })
    ));

    let encoded = serde_json::to_value(invalid).unwrap();
    assert!(serde_json::from_value::<WorldFormationSpec>(encoded).is_err());
}

#[test]
fn resolved_formation_round_trips_and_rejects_invalid_wire_data() {
    for resolved in CONCRETE_PRESETS {
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Random,
            resolved,
        )
        .unwrap();
        let encoded = serde_json::to_string(&formation).unwrap();
        assert_eq!(
            serde_json::from_str::<ResolvedWorldFormation>(&encoded).unwrap(),
            formation
        );
    }

    let invalid_schema = serde_json::json!({
        "schema_version": RESOLVED_WORLD_FORMATION_SCHEMA_V1 + 1,
        "requested": "Continents",
        "resolved": "Continents"
    });
    assert!(serde_json::from_value::<ResolvedWorldFormation>(invalid_schema).is_err());

    let resolved_random = serde_json::json!({
        "schema_version": RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        "requested": "Random",
        "resolved": "Random"
    });
    assert!(serde_json::from_value::<ResolvedWorldFormation>(resolved_random).is_err());
}

#[test]
fn resolved_profiles_expose_literal_recommendations_and_narrow_mantle_biases() {
    let cases = [
        (ResolvedWorldFormationPreset::Continents, 0.38),
        (ResolvedWorldFormationPreset::Archipelago, 0.26),
        (ResolvedWorldFormationPreset::Supercontinent, 0.42),
        (ResolvedWorldFormationPreset::GreatIsland, 0.28),
        (ResolvedWorldFormationPreset::VolcanicIslands, 0.16),
    ];

    for (resolved, expected_fraction) in cases {
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Random,
            resolved,
        )
        .unwrap();
        assert_eq!(formation.requested(), WorldFormationPreset::Random);
        assert_eq!(formation.resolved(), resolved);
        assert_eq!(
            formation.recommended_continental_crust_fraction(),
            expected_fraction
        );
        assert_eq!(
            formation.mantle_bias(),
            if resolved == ResolvedWorldFormationPreset::VolcanicIslands {
                MantleFormationBias::VolcanicIslands
            } else {
                MantleFormationBias::Neutral
            }
        );
    }
}
