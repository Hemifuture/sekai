//! Whittaker–Ricklefs potential dry-land vegetation, using Navarro et al. (2025).
mod polygons;

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Serialize;

use super::{climatological_annual_total_mm, climatological_monthly_mean, GlobalCirculationFields};
use polygons::POLYGONS;

struct ClimatePolygon {
    key: &'static str,
    // Closed ring in the source chart's degrees Celsius and centimetres per year.
    ring: &'static [[f64; 2]],
}

pub(super) fn category_labels() -> BTreeMap<u32, String> {
    POLYGONS
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                i as u32,
                format!("field.sekai.core.natural.potential_biome.{}", p.key),
            )
        })
        .collect()
}

// Lower bounds on polygon distance allow the nearest-region search to skip whole rings.
static BOUNDS: LazyLock<Vec<[[f64; 2]; 2]>> = LazyLock::new(|| {
    POLYGONS
        .iter()
        .map(|polygon| {
            let mut bounds = [polygon.ring[0]; 2];
            for point in polygon.ring {
                for (axis, &value) in point.iter().enumerate() {
                    bounds[0][axis] = bounds[0][axis].min(value);
                    bounds[1][axis] = bounds[1][axis].max(value);
                }
            }
            bounds
        })
        .collect()
});

fn classify(x: f64, annual_mm: f64) -> (u32, bool) {
    let y = annual_mm / 10.0;
    let mut best = f64::INFINITY;
    let mut category = 0;
    for (index, (poly, &[lo, hi])) in POLYGONS.iter().zip(BOUNDS.iter()).enumerate() {
        let dx = x - x.clamp(lo[0], hi[0]);
        let dy = y - y.clamp(lo[1], hi[1]);
        if dx * dx + dy * dy >= best {
            continue;
        }
        let mut inside = false;
        let mut nearest = f64::INFINITY;
        for edge in poly.ring.windows(2) {
            let [ax, ay] = edge[0];
            let [bx, by] = edge[1];
            let vx = bx - ax;
            let vy = by - ay;
            if (ay > y) != (by > y) && x < ax + (y - ay) * vx / vy {
                inside = !inside;
            }
            let length_sq = vx * vx + vy * vy;
            if length_sq == 0.0 {
                continue;
            }
            let t = (((x - ax) * vx + (y - ay) * vy) / length_sq).clamp(0.0, 1.0);
            let dx = (x - ax) - t * vx;
            let dy = (y - ay) - t * vy;
            nearest = nearest.min(dx * dx + dy * dy);
        }
        if inside || nearest == 0.0 {
            return (index as u32, false);
        }
        if nearest < best {
            best = nearest;
            category = index as u32;
        }
    }
    (category, true)
}

/// One candidate per source cell; only a dry fine-bed sample realizes the candidate.
#[derive(Debug, Serialize)]
pub(crate) struct PotentialVegetation {
    pub(crate) categories: Vec<u32>,
    pub(crate) extrapolated: Vec<bool>,
}

impl PotentialVegetation {
    pub(crate) fn from_climate(climate: &GlobalCirculationFields) -> Self {
        let (categories, extrapolated) = climate
            .monthly_air_temperature_c()
            .values()
            .iter()
            .zip(climate.monthly_precipitation_mm_day().values())
            .map(|(temp, rain)| {
                classify(
                    f64::from(climatological_monthly_mean(temp)),
                    climatological_annual_total_mm(rain),
                )
            })
            .unzip();
        Self {
            categories,
            extrapolated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_chart_interiors_and_nearest_extrapolation() {
        let [temp, cm] = POLYGONS[0].ring[0];
        assert!(POLYGONS[2].ring.contains(&[temp, cm]));
        assert_eq!(classify(temp, cm * 10.0), (0, false));
        for (temp, mm, key, outside) in [
            (-10.0, 200.0, "tundra", false),
            (0.0, 800.0, "boreal_forest", false),
            (10.0, 200.0, "temperate_grassland_desert", false),
            (15.0, 650.0, "woodland_shrubland", false),
            (10.0, 1200.0, "temperate_seasonal_forest", false),
            (12.0, 2300.0, "temperate_rain_forest", false),
            (25.0, 1600.0, "tropical_seasonal_forest_savanna", false),
            (25.0, 100.0, "subtropical_desert", false),
            (25.0, 3500.0, "tropical_rain_forest", false),
            (25.0, 20000.0, "tropical_rain_forest", true),
        ] {
            let (id, extrapolated) = classify(temp, mm);
            assert_eq!((POLYGONS[id as usize].key, extrapolated), (key, outside));
        }
    }
}
