pub(super) mod arrival;
pub(super) mod metric;
pub(super) mod noise;

#[cfg(test)]
mod tests {
    use super::noise::{GaborKernel, SphericalNoise3d};
    use crate::generators::natural::fractal::FractalProfile;
    use crate::world::spatial::UnitVector3;

    const PROFILE: FractalProfile = FractalProfile {
        octaves: 5,
        frequency: 1.25,
        lacunarity: 2.03,
        persistence: 0.5,
    };

    fn unit(x: f64, y: f64, z: f64) -> UnitVector3 {
        UnitVector3::new(x, y, z).unwrap()
    }

    #[test]
    fn shared_spherical_noise_is_seeded_bounded_and_coordinate_seam_free() {
        let first = SphericalNoise3d::new(71);
        let repeated = SphericalNoise3d::new(71);
        let changed = SphericalNoise3d::new(72);
        let points = [
            unit(1.0, 0.0, 0.0),
            unit(-1.0, 1.0e-12, 0.0),
            unit(-1.0, -1.0e-12, 0.0),
            unit(0.0, 0.0, 1.0),
            unit(0.0, 0.0, -1.0),
        ];
        let actual = points.map(|point| first.fbm(point, PROFILE));

        assert_eq!(actual, points.map(|point| repeated.fbm(point, PROFILE)));
        assert_ne!(actual, points.map(|point| changed.fbm(point, PROFILE)));
        assert!(actual
            .iter()
            .all(|value| value.is_finite() && (-1.0..=1.0).contains(value)));
        assert!((actual[1] - actual[2]).abs() < 1.0e-9);
        assert!(points
            .map(|point| first.ridged(point, PROFILE))
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
    }

    #[test]
    fn sparse_gabor_is_bounded_deterministic_and_varies_more_across_its_ridge() {
        let first = SphericalNoise3d::new(91);
        let repeated = SphericalNoise3d::new(91);
        let changed = SphericalNoise3d::new(92);
        let kernel = GaborKernel {
            envelope_scale_rad: 0.8,
            carrier_frequency: 16.0,
            impulse_count: 64,
        };
        let tangent = [0.0, 0.0, 1.0];
        let mut along_variation = 0.0;
        let mut across_variation = 0.0;

        for index in 0..16 {
            let longitude = std::f64::consts::TAU * index as f64 / 16.0;
            let base = unit(longitude.cos(), longitude.sin(), 0.0);
            let along = unit(longitude.cos(), longitude.sin(), 0.02);
            let across = unit((longitude + 0.02).cos(), (longitude + 0.02).sin(), 0.0);
            let base_value = first.sparse_gabor(base, tangent, kernel);
            let along_value = first.sparse_gabor(along, tangent, kernel);
            let across_value = first.sparse_gabor(across, tangent, kernel);

            assert!(base_value.is_finite() && (-1.0..=1.0).contains(&base_value));
            assert_eq!(base_value, repeated.sparse_gabor(base, tangent, kernel));
            along_variation += (along_value - base_value).abs();
            across_variation += (across_value - base_value).abs();
        }

        assert!(
            across_variation > along_variation * 1.1,
            "expected ridge-aligned Gabor response, along={along_variation}, across={across_variation}"
        );

        let seam_north = unit(-1.0, 1.0e-12, 0.0);
        let seam_south = unit(-1.0, -1.0e-12, 0.0);
        let seam_value = first.sparse_gabor(seam_north, tangent, kernel);
        assert!((seam_value - first.sparse_gabor(seam_south, tangent, kernel)).abs() < 1.0e-9);
        assert_ne!(
            seam_value,
            changed.sparse_gabor(seam_north, tangent, kernel)
        );
        for (pole, pole_tangent) in [
            (unit(0.0, 0.0, 1.0), [1.0, 0.0, 0.0]),
            (unit(0.0, 0.0, -1.0), [1.0, 0.0, 0.0]),
        ] {
            let value = first.sparse_gabor(pole, pole_tangent, kernel);
            assert!(value.is_finite() && (-1.0..=1.0).contains(&value));
        }
    }
}
