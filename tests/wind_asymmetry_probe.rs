//! Zonal structure of the annual-mean near-surface wind on Draft seed 42:
//! the non-zonal variance share plus a 5° band profile with its mirror, so a
//! change to the P4 boundary physics can be compared before and after
//! (`docs/superpowers/specs/2026-09-02-p4-zonal-asymmetry-design.md` §1).

mod support;

use sekai::world::spatial::canonical_east_north_basis;
use support::causal_formation::causal_formation_fixture;

#[test]
#[ignore = "release-only Draft seed 42 wind-structure probe"]
fn near_surface_wind_zonal_structure() {
    let fixture = causal_formation_fixture();
    let bundle = fixture.artifact.bundle();
    let metric = bundle
        .climate_quality()
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "near-surface-wind-non-zonal-variance-fraction")
        .expect("the non-zonal wind metric is recorded");
    eprintln!(
        "[wind] non-zonal variance fraction = {:.4}",
        metric.value().unwrap_or(f64::NAN)
    );
    for name in [
        "precipitation-global-mean-mm-day",
        "evaporation-global-mean-mm-day",
        "toa-net-radiation-global-mean-w-m2",
        "orographic-rain-shadow-leeward-drying",
        "orographic-uplift-enrichment-ratio",
        "orographic-precipitation-response",
    ] {
        let value = bundle
            .climate_quality()
            .metrics()
            .iter()
            .find(|metric| metric.id().name() == name)
            .and_then(|metric| metric.value())
            .unwrap_or(f64::NAN);
        eprintln!("[wind] {name} = {value:.4}");
    }

    {
        let fields = bundle.climate().fields();
        let land_mask = bundle
            .surface_formation()
            .terrain_fields()
            .land_ocean()
            .raw_values();
        let mut sums = [[0.0_f64; 3]; 2];
        for (index, cell) in fixture.surface.cells().iter().enumerate() {
            let area = cell.area.get();
            let evaporation = fields.monthly_evaporation_mm_day().values()[index]
                .iter()
                .map(|value| f64::from(*value))
                .sum::<f64>()
                / 12.0;
            let precipitation = fields.monthly_precipitation_mm_day().values()[index]
                .iter()
                .map(|value| f64::from(*value))
                .sum::<f64>()
                / 12.0;
            let bucket = usize::from(land_mask[index] == 1);
            sums[bucket][0] += area;
            sums[bucket][1] += area * evaporation;
            sums[bucket][2] += area * precipitation;
        }
        for (name, [area, evaporation, precipitation]) in [("ocean", sums[0]), ("land", sums[1])] {
            eprintln!(
                "[wind] {name}: area share {:.3}, E {:.3} mm/day, P {:.3} mm/day",
                area / (sums[0][0] + sums[1][0]),
                evaporation / area,
                precipitation / area
            );
        }
    }
    let wind = bundle.climate().fields().near_surface_wind_m_s().values();
    let land = bundle
        .surface_formation()
        .terrain_fields()
        .land_ocean()
        .raw_values();
    let cells = fixture.surface.cells();
    const BANDS: usize = 36;
    let mut sum = vec![[0.0_f64; 2]; BANDS];
    let mut count = vec![0_usize; BANDS];
    let mut samples = Vec::with_capacity(cells.len());
    for (index, cell) in cells.iter().enumerate() {
        let (east, north) = canonical_east_north_basis(cell.centroid);
        let mut annual = [0.0_f64; 3];
        for month in &wind[index] {
            for (component, value) in annual.iter_mut().zip(month) {
                *component += f64::from(*value) / 12.0;
            }
        }
        let zonal = annual[0] * east[0] + annual[1] * east[1] + annual[2] * east[2];
        let meridional = annual[0] * north[0] + annual[1] * north[1] + annual[2] * north[2];
        let latitude = cell.centroid.components()[2].asin().to_degrees();
        let band = (((latitude + 90.0) / 5.0).floor() as usize).min(BANDS - 1);
        sum[band][0] += zonal;
        sum[band][1] += meridional;
        count[band] += 1;
        samples.push((band, zonal, meridional, land[index] == 1));
    }
    let mean = |band: usize| {
        if count[band] == 0 {
            [0.0, 0.0]
        } else {
            [
                sum[band][0] / count[band] as f64,
                sum[band][1] / count[band] as f64,
            ]
        }
    };
    let mut land_deviation = (0.0_f64, 0_usize);
    let mut sea_deviation = (0.0_f64, 0_usize);
    for &(band, zonal, meridional, is_land) in &samples {
        let [mean_zonal, mean_meridional] = mean(band);
        let deviation = (zonal - mean_zonal).powi(2) + (meridional - mean_meridional).powi(2);
        let target = if is_land {
            &mut land_deviation
        } else {
            &mut sea_deviation
        };
        target.0 += deviation;
        target.1 += 1;
    }
    eprintln!(
        "[wind] rms deviation from band mean: land {:.2} m/s, sea {:.2} m/s",
        (land_deviation.0 / land_deviation.1.max(1) as f64).sqrt(),
        (sea_deviation.0 / sea_deviation.1.max(1) as f64).sqrt()
    );
    for band in BANDS / 2..BANDS {
        let mirror = BANDS - 1 - band;
        let latitude = -90.0 + 5.0 * band as f64 + 2.5;
        let [u, v] = mean(band);
        let [mu, mv] = mean(mirror);
        eprintln!("[wind] {latitude:>5.1}  u={u:+6.2} v={v:+6.2}   mirror u={mu:+6.2} v={mv:+6.2}");
    }
}
