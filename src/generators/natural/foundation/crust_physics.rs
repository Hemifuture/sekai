//! Shared present-day crust-to-height approximations.
//!
//! The tectonic initializer and relief projection use the same Airy-style
//! continental freeboard and square-root oceanic plate-cooling laws. Keeping
//! these recipes here prevents the current crust state and its heightmap from
//! encoding two different physical baselines.

const CONTINENTAL_REFERENCE_THICKNESS_KM: f32 = 35.0;
const CONTINENTAL_REFERENCE_FREEBOARD_M: f32 = 300.0;
// A conservative effective Airy response after mantle/crust density contrast.
const CONTINENTAL_ISOSTATIC_RESPONSE_M_PER_KM: f32 = 135.0;
const OCEANIC_RIDGE_DEPTH_M: f32 = -2_600.0;
// Parsons-Sclater-style square-root plate cooling, deliberately bounded later
// by the public crust/relief contracts rather than by this pure recipe.
const OCEANIC_COOLING_M_PER_SQRT_MYR: f32 = 350.0;
const OCEANIC_REFERENCE_THICKNESS_KM: f32 = 7.0;
const OCEANIC_THICKNESS_RESPONSE_M_PER_KM: f32 = 90.0;

pub(in crate::generators::natural) fn continental_isostatic_elevation_m(thickness_km: f32) -> f32 {
    CONTINENTAL_REFERENCE_FREEBOARD_M
        + (thickness_km - CONTINENTAL_REFERENCE_THICKNESS_KM)
            * CONTINENTAL_ISOSTATIC_RESPONSE_M_PER_KM
}

pub(in crate::generators::natural) fn oceanic_plate_cooling_elevation_m(
    age_myr: f32,
    thickness_km: f32,
) -> f32 {
    OCEANIC_RIDGE_DEPTH_M - OCEANIC_COOLING_M_PER_SQRT_MYR * age_myr.sqrt()
        + (thickness_km - OCEANIC_REFERENCE_THICKNESS_KM) * OCEANIC_THICKNESS_RESPONSE_M_PER_KM
}

#[cfg(test)]
mod tests {
    use super::{continental_isostatic_elevation_m, oceanic_plate_cooling_elevation_m};

    #[test]
    fn shared_crust_physics_is_monotone_in_thickness_and_ocean_age() {
        assert!(continental_isostatic_elevation_m(24.0) < 0.0);
        assert!(continental_isostatic_elevation_m(52.0) > continental_isostatic_elevation_m(35.0));
        assert!(
            oceanic_plate_cooling_elevation_m(0.0, 7.0)
                > oceanic_plate_cooling_elevation_m(100.0, 7.0)
        );
        assert!(
            oceanic_plate_cooling_elevation_m(100.0, 10.0)
                > oceanic_plate_cooling_elevation_m(100.0, 5.0)
        );
    }
}
