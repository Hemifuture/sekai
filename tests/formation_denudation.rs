//! Land denudation budget of the published P5 formation horizon.
//!
//! The nine retained elevation components are a mass ledger, so the area
//! weighted sum of the erosive ones minus the depositional ones over the
//! frozen horizon is the model's land denudation rate. That rate is directly
//! observable on Earth, which makes it the right quantity to pin the P5
//! erosion constants against: cosmogenic `10Be` basin averages give a global
//! median of `54 m/Myr` (Portenga & Bierman 2011, *GSA Today* 21(8), 4-10, DOI
//! `10.1130/G111A.1`) and an area-weighted global mean of about the same
//! (Willenbring, Codilean & McElroy 2013, *Geology* 41(3), 343-346, DOI
//! `10.1130/G33918.1`); the global suspended flux to the ocean implies the same
//! order (Milliman & Farnsworth 2011, *River Discharge to the Coastal Ocean*).
//!
//! Run explicitly:
//! `cargo test --release --test formation_denudation -- --ignored --nocapture`

mod support;

use sekai::world::natural::{
    LandOceanKind, NaturalQualityProfile, SURFACE_FORMATION_HORIZON_YEARS,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::Meters;

use support::causal_formation::build_causal_formation;

/// Seeds are the smallest set that still shows cross-world spread; the full
/// 17-seed corpus lives in `surface_formation_evidence`.
const SEEDS: [u64; 2] = [42, 3];
/// Admissible area-weighted land denudation, in metres per million years.
///
/// Centered on the `54 m/Myr` global `10Be` median with the order-of-magnitude
/// spread the same compilations report between shield interiors and active
/// orogens.
const DENUDATION_MIN_M_PER_MYR: f64 = 20.0;
const DENUDATION_MAX_M_PER_MYR: f64 = 200.0;

struct LandBudget {
    fluvial_erosion_m: f64,
    hillslope_erosion_m: f64,
    coastal_erosion_m: f64,
    hillslope_deposition_m: f64,
    routed_deposition_m: f64,
    coastal_deposition_m: f64,
}

impl LandBudget {
    fn net_denudation_m(&self) -> f64 {
        self.fluvial_erosion_m + self.hillslope_erosion_m + self.coastal_erosion_m
            - self.hillslope_deposition_m
            - self.routed_deposition_m
            - self.coastal_deposition_m
    }

    fn denudation_m_per_myr(&self) -> f64 {
        self.net_denudation_m() / SURFACE_FORMATION_HORIZON_YEARS * 1.0e6
    }
}

#[test]
#[ignore = "release-only land denudation probe; run with --ignored --nocapture"]
fn published_land_denudation_matches_the_observed_global_rate() {
    let surface = ProfileSurface::draft();
    let mut failures = Vec::new();
    for seed in SEEDS {
        let budget = land_budget(&surface.0, seed);
        let rate = budget.denudation_m_per_myr();
        println!(
            "seed={seed} denudation={rate:.1} m/Myr fluvial={:.1} hillslope={:.1} coastal={:.3} \
             routed_dep={:.1} hillslope_dep={:.1} coastal_dep={:.3} (metres over horizon)",
            budget.fluvial_erosion_m,
            budget.hillslope_erosion_m,
            budget.coastal_erosion_m,
            budget.routed_deposition_m,
            budget.hillslope_deposition_m,
            budget.coastal_deposition_m,
        );
        if !(DENUDATION_MIN_M_PER_MYR..=DENUDATION_MAX_M_PER_MYR).contains(&rate) {
            failures.push(format!("seed {seed}: {rate:.1} m/Myr"));
        }
    }
    assert!(
        failures.is_empty(),
        "land denudation outside the observed \
         {DENUDATION_MIN_M_PER_MYR}..={DENUDATION_MAX_M_PER_MYR} m/Myr band: {failures:?}"
    );
}

struct ProfileSurface(SphericalSurfaceSnapshot);

impl ProfileSurface {
    fn draft() -> Self {
        Self(
            sekai::generators::spatial::ProfileSurfaceBuilder::build(
                NaturalQualityProfile::Draft,
                Meters::new(6_371_000.0).unwrap(),
                &sekai::engine::BuildCancellation::new(),
            )
            .unwrap()
            .authoritative_surface()
            .clone(),
        )
    }
}

fn land_budget(surface: &SphericalSurfaceSnapshot, seed: u64) -> LandBudget {
    let artifact = build_causal_formation(surface, NaturalQualityProfile::Draft, seed);
    let snapshot = artifact.bundle().surface_formation();
    let terrain = snapshot.terrain_fields();
    let components = terrain.elevation_components();
    let land_ocean = terrain.land_ocean();
    let mut land_area_m2 = 0.0_f64;
    let mut weighted = [0.0_f64; 6];
    for (index, cell) in surface.cells().iter().enumerate() {
        if land_ocean.get(index) != Some(LandOceanKind::Land) {
            continue;
        }
        let area_m2 = cell.area.get();
        land_area_m2 += area_m2;
        for (slot, values) in weighted.iter_mut().zip([
            components.fluvial_erosion_m(),
            components.hillslope_erosion_m(),
            components.coastal_erosion_m(),
            components.hillslope_deposition_m(),
            components.routed_sediment_deposition_m(),
            components.coastal_deposition_m(),
        ]) {
            *slot += area_m2 * f64::from(values[index]);
        }
    }
    assert!(land_area_m2 > 0.0, "seed {seed} published no land");
    let mean = weighted.map(|value| value / land_area_m2);
    LandBudget {
        fluvial_erosion_m: mean[0],
        hillslope_erosion_m: mean[1],
        coastal_erosion_m: mean[2],
        hillslope_deposition_m: mean[3],
        routed_deposition_m: mean[4],
        coastal_deposition_m: mean[5],
    }
}
