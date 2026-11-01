#![cfg(not(debug_assertions))]

use sekai as coast_probe_crate;
use sekai::generators::natural::causal_natural_formation_graph;
use sekai::world::natural::{NaturalQualityProfile, ReliefSpec};

#[path = "support/procedural_coast.rs"]
mod procedural_coast;

#[test]
#[ignore = "offline full-chain Draft procedural coast preview; run in Release"]
fn procedural_coast_publishes_a_valid_final_bundle() {
    procedural_coast::generate_procedural_coast_preview(
        NaturalQualityProfile::Draft,
        ReliefSpec::default(),
        "full-chain.json",
        causal_natural_formation_graph().unwrap(),
    );
}

#[test]
#[ignore = "offline user-accepted 25-percent target-land preview; run in Release"]
fn procedural_coast_target25_has_coherent_fine_shorelines() {
    procedural_coast::generate_procedural_coast_preview(
        NaturalQualityProfile::Draft,
        procedural_coast::target25_relief(),
        "full-chain-target25.json",
        causal_natural_formation_graph().unwrap(),
    );
}
