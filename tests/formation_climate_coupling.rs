mod support;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    GlobalCirculationGenerator, GlobalClimateForcingBuilder, SELECTED_PRODUCTION_INTEGRATOR,
};
use sekai::world::natural::{ClimateModelProfile, ClimateSpec};

use support::global_circulation::global_circulation_fixture;

#[test]
fn public_p4_boundary_stays_relief_bound_deterministic_and_selected() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let repeated = GlobalClimateForcingBuilder::build(
        surface,
        &fixture.relief,
        &ClimateSpec::default(),
        &fixture.domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(repeated, fixture.forcing);

    let snapshot = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &repeated,
        ClimateModelProfile::C2LayeredV1,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(snapshot.integrator(), SELECTED_PRODUCTION_INTEGRATOR);
    assert_eq!(snapshot.profile(), ClimateModelProfile::C2LayeredV1);
    assert_eq!(
        snapshot.checkpoint().forcing_fingerprint(),
        repeated.fingerprint()
    );
    assert_ne!(snapshot.checkpoint().input_fingerprint(), &[0; 32]);
}
