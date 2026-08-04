use sekai::generators::natural::{
    legacy_planar_natural_foundation_graph, natural_foundation_graph,
};

#[test]
fn legacy_alias_preserves_the_exact_planar_graph() {
    let legacy = legacy_planar_natural_foundation_graph().unwrap();
    let compatibility = natural_foundation_graph().unwrap();

    assert_eq!(legacy.stage_ids(), compatibility.stage_ids());
    assert_eq!(legacy.descriptors(), compatibility.descriptors());
    assert!(legacy
        .descriptors()
        .iter()
        .all(|descriptor| !descriptor.output().as_str().contains("spherical")));
}

#[test]
fn old_application_state_defaults_without_adding_a_geometry_selector() {
    let restored: sekai::TemplateApp =
        serde_json::from_value(serde_json::json!({ "world_seed": 7 })).unwrap();
    let encoded = serde_json::to_value(restored).unwrap();

    assert_eq!(encoded["world_seed"], 7);
    assert!(encoded.get("spherical_mode").is_none());
    assert!(encoded.get("geometry_mode").is_none());
}
