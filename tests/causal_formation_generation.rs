mod support;

use sekai::engine::Artifact;
use sekai::generators::natural::{causal_natural_formation_graph, NaturalFormationBundleArtifact};

use support::causal_formation::causal_formation_fixture;

#[test]
fn production_graph_has_one_atomic_formation_publication() {
    assert_eq!(
        NaturalFormationBundleArtifact::KEY.as_str(),
        "world.natural-formation-bundle"
    );
    assert_eq!(
        causal_natural_formation_graph().unwrap().stage_ids(),
        vec!["natural.climate-work-domain", "natural.causal-formation"]
    );
}

#[test]
fn published_bundle_binds_final_siblings_without_solver_history() {
    // Cross-layer atomic identity needs one representative full-chain case;
    // Draft/42 is the sole ordinary-test corpus member for this contract.
    let fixture = causal_formation_fixture();
    let artifact = fixture.artifact.as_ref();
    artifact.validate().unwrap();
    let bundle = artifact.bundle();

    assert_eq!(
        bundle.surface_ref().cell_count() as usize,
        fixture.surface.cells().len()
    );
    assert_eq!(
        bundle
            .surface_formation()
            .checkpoint()
            .upstream()
            .formation_climate_checkpoint_fingerprint(),
        bundle.climate().checkpoint().fingerprint()
    );
    assert_eq!(
        bundle.climate_quality().subject_fingerprint(),
        Some(bundle.climate().checkpoint().fingerprint())
    );

    let json = serde_json::to_value(bundle).unwrap();
    assert!(json["surface_formation"].get("formation_climate").is_none());
    assert_no_solver_history(&json);
}

fn assert_no_solver_history(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, child) in fields {
                assert!(
                    !["history", "checkpoints", "pseudo_time", "rejected_steps"]
                        .contains(&key.as_str()),
                    "forbidden solver-history key: {key}"
                );
                assert_no_solver_history(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                assert_no_solver_history(item);
            }
        }
        _ => {}
    }
}
