use rand::RngCore;

use crate::generators::natural::fractal::FractalProfile;
use crate::generators::natural::morphology::noise::{GaborKernel, SphericalNoise3d};
use crate::generators::natural::random::{
    LabeledSubstreams, OCEANIC_DETAIL_V3_LABEL, OROGENIC_DETAIL_V3_LABEL,
};
use crate::world::natural::{CrustKind, SphericalOrogenyKind};
use crate::world::spatial::{canonical_east_north_basis, UnitVector3};

const CONTINENTAL_BROAD_AMPLITUDE_M: f64 = 120.0;
const OCEANIC_BROAD_AMPLITUDE_M: f64 = 180.0;
const OCEANIC_FABRIC_AMPLITUDE_M: f64 = 140.0;
const ANDEAN_DETAIL_AMPLITUDE_M: f64 = 760.0;
const HIMALAYAN_DETAIL_AMPLITUDE_M: f64 = 1_100.0;
const OROGENY_DETAIL_HALF_LIFE_MYR: f64 = 80.0;
const OCEANIC_FABRIC_HALF_LIFE_MYR: f64 = 120.0;
const MIN_GABOR_ENVELOPE_RAD: f64 = 0.12;
const MAX_GABOR_ENVELOPE_RAD: f64 = 0.90;
const MIN_GABOR_CYCLES_PER_RAD: f64 = 0.80;
const MAX_GABOR_CYCLES_PER_RAD: f64 = 6.0;

const BROAD_PROFILE: FractalProfile = FractalProfile {
    octaves: 5,
    frequency: 1.25,
    lacunarity: 2.03,
    persistence: 0.50,
};

/// Two independent, labeled detail sources used after tectonic construction.
///
/// This type knows only how to shape bounded surface detail. It cannot change
/// plate ownership, crust state, or the coarse tectonic height field.
pub(super) struct DirectedDetailNoise {
    orogenic: SphericalNoise3d,
    oceanic: SphericalNoise3d,
    broad_profile: FractalProfile,
    gabor_kernel: GaborKernel,
}

impl DirectedDetailNoise {
    pub(super) fn from_streams(
        streams: &LabeledSubstreams,
        radius_m: f64,
        sample_spacing_m: f64,
    ) -> Self {
        let mut orogenic_rng = streams.stream(OROGENIC_DETAIL_V3_LABEL);
        let mut oceanic_rng = streams.stream(OCEANIC_DETAIL_V3_LABEL);
        let angular_spacing = (sample_spacing_m / radius_m).clamp(f64::EPSILON, 1.0);
        let envelope_scale_rad =
            (4.0 * angular_spacing).clamp(MIN_GABOR_ENVELOPE_RAD, MAX_GABOR_ENVELOPE_RAD);
        let carrier_frequency =
            (0.25 / angular_spacing).clamp(MIN_GABOR_CYCLES_PER_RAD, MAX_GABOR_CYCLES_PER_RAD);
        Self {
            orogenic: SphericalNoise3d::new(orogenic_rng.next_u32()),
            oceanic: SphericalNoise3d::new(oceanic_rng.next_u32()),
            broad_profile: BROAD_PROFILE.limited_to_resolution(radius_m, sample_spacing_m),
            gabor_kernel: GaborKernel {
                envelope_scale_rad,
                carrier_frequency,
                impulse_count: 48,
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn sample_m(
        &self,
        radial: UnitVector3,
        crust_kind: CrustKind,
        crust_age_myr: f32,
        lineation_east: f32,
        lineation_north: f32,
        orogeny_kind: SphericalOrogenyKind,
        orogeny_age_myr: f32,
    ) -> f64 {
        let lineation = tangent_from_components(radial, lineation_east, lineation_north);
        let detail = match crust_kind {
            CrustKind::Continental => {
                let broad =
                    self.orogenic.fbm(radial, self.broad_profile) * CONTINENTAL_BROAD_AMPLITUDE_M;
                broad
                    + orogenic_ridges_m(
                        &self.orogenic,
                        radial,
                        lineation,
                        orogeny_kind,
                        orogeny_age_myr,
                        self.gabor_kernel,
                    )
            }
            CrustKind::Oceanic => {
                let broad =
                    self.oceanic.fbm(radial, self.broad_profile) * OCEANIC_BROAD_AMPLITUDE_M;
                let fabric = lineation.map_or(0.0, |tangent| {
                    let age_decay = half_life_decay(crust_age_myr, OCEANIC_FABRIC_HALF_LIFE_MYR);
                    self.oceanic
                        .sparse_gabor(radial, tangent, self.gabor_kernel)
                        * OCEANIC_FABRIC_AMPLITUDE_M
                        * age_decay
                });
                broad + fabric
            }
        };
        detail
    }
}

fn orogenic_ridges_m(
    noise: &SphericalNoise3d,
    radial: UnitVector3,
    lineation: Option<[f64; 3]>,
    kind: SphericalOrogenyKind,
    age_myr: f32,
    kernel: GaborKernel,
) -> f64 {
    let amplitude = match kind {
        SphericalOrogenyKind::None => return 0.0,
        SphericalOrogenyKind::Andean => ANDEAN_DETAIL_AMPLITUDE_M,
        SphericalOrogenyKind::Himalayan => HIMALAYAN_DETAIL_AMPLITUDE_M,
    };
    let Some(tangent) = lineation else {
        return 0.0;
    };
    let signal = noise.sparse_gabor(radial, tangent, kernel);
    // A positive ridge envelope adds relief without letting detail noise invert
    // the uplift already recorded by the tectonic construction state.
    let ridge_envelope = 0.35 + 0.65 * (signal + 1.0) * 0.5;
    amplitude * ridge_envelope * half_life_decay(age_myr, OROGENY_DETAIL_HALF_LIFE_MYR)
}

fn tangent_from_components(
    radial: UnitVector3,
    east_component: f32,
    north_component: f32,
) -> Option<[f64; 3]> {
    if east_component == 0.0 && north_component == 0.0 {
        return None;
    }
    let (east, north) = canonical_east_north_basis(radial);
    Some(std::array::from_fn(|axis| {
        east[axis] * f64::from(east_component) + north[axis] * f64::from(north_component)
    }))
}

fn half_life_decay(age_myr: f32, half_life_myr: f64) -> f64 {
    2.0_f64.powf(-f64::from(age_myr) / half_life_myr)
}
