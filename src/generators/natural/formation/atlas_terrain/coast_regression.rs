use std::f64::consts::TAU;

use civ_atlas_terrain::TerrainParams;

use super::{AtlasTerrainGenerator, AtlasTerrainRequest};
use crate::engine::BuildCancellation;
use crate::world::natural::{AtlasTerrainSnapshot, NaturalQualityProfile, SurfaceWaterKind};
use crate::world::spatial::{add, central_angle, cross, gnomonic_inverse, scale, UnitVector3};

fn unit([x, y, z]: [f64; 3]) -> UnitVector3 {
    UnitVector3::new(x, y, z).expect("the observed arc is finite and nondegenerate")
}

fn chord_point(start: UnitVector3, end: UnitVector3, t: f64) -> UnitVector3 {
    unit(add(
        scale(start.components(), 1.0 - t),
        scale(end.components(), t),
    ))
}

fn check_enclosed_depression(snapshot: &AtlasTerrainSnapshot, edge_site: UnitVector3, step: f64) {
    let radius_m = snapshot.surface().radius().get();
    // Measured at 28.42331° S, 60.29940° E in this same world.
    let floor = gnomonic_inverse(edge_site, [-40_000.0 / radius_m, 107_000.0 / radius_m]);
    let rim_angle = 16_000.0 / radius_m;
    let segments = (TAU * rim_angle / step).ceil() as usize;
    let rim = (0..segments)
        .map(|i| {
            let (sin, cos) = (TAU * i as f64 / segments as f64).sin_cos();
            gnomonic_inverse(floor, [cos * rim_angle.tan(), sin * rim_angle.tan()]).components()
        })
        .collect::<Vec<_>>();
    let rim_beds = snapshot
        .fine_terrain()
        .sample_surface_batch(&rim, &|| false)
        .unwrap();
    let lowest_rim_m = rim_beds
        .iter()
        .map(|p| p.bed_elevation_m)
        .fold(f64::INFINITY, f64::min);
    assert!(
        lowest_rim_m > 0.0,
        "the basin rim must stand above sea level"
    );
    let nearest_site = snapshot
        .surface()
        .cells()
        .iter()
        .map(|cell| central_angle(cell.site, floor))
        .fold(f64::INFINITY, f64::min);
    assert!(
        nearest_site > rim_angle,
        "the enclosed basin must contain no source water seed"
    );
    let point = snapshot.sample_surface(floor.components()).unwrap();
    log::debug!(
        "Atlas coast depression floor_m={:.2} lowest_rim_m={lowest_rim_m:.2} \
        nearest_site_km={:.2} rim_samples={segments} water={:?}",
        point.bed_elevation_m,
        nearest_site * radius_m / 1000.0,
        point.water
    );
    assert!(point.bed_elevation_m < 0.0);
    assert_eq!(
        point.water,
        SurfaceWaterKind::DryLand,
        "enclosed unseeded depression"
    );
}

#[test]
#[ignore = "actual Standard Atlas publication for a reported shoreline; run in Release"]
fn reported_atlas_coast_crosses_source_edge_continuously() {
    let _ = env_logger::Builder::new()
        .filter_module(
            "sekai::generators::natural::formation::atlas_terrain",
            log::LevelFilter::Debug,
        )
        .is_test(true)
        .try_init();
    let request = AtlasTerrainRequest {
        author: TerrainParams {
            seed: 27_276_201,
            ..Default::default()
        },
        quality: NaturalQualityProfile::Standard,
        ..Default::default()
    };
    let artifact = AtlasTerrainGenerator::generate(&request, &BuildCancellation::new()).unwrap();
    let snapshot = artifact.snapshot();
    let site = |i: usize| snapshot.surface().cells()[i].site;
    let (start, end) = (site(11711), site(11869));
    let normal = unit(cross(start.components(), end.components())).components();
    let offset = scale(normal, 20.0 / snapshot.surface().radius().get());

    // The recorded 55.5 km segment ran from 29.7180° S, 60.2532° E to
    // 30.0370° S, 59.8109° E. These are its original chord sample indices.
    let edge_steps = 449.0;
    let centres = (188..=370)
        .map(|i| chord_point(start, end, (f64::from(i) + 0.5) / edge_steps))
        .collect::<Vec<_>>();
    let step = central_angle(start, end) / edge_steps;
    check_enclosed_depression(snapshot, start, step);

    // A submerged transect from the centreline to an actual upstream ocean site
    // establishes which water body reaches both sides, independently of the classifier.
    let fine = snapshot.fine_terrain();
    let ocean_site = 11868;
    let source_class = fine.coarse_terrain().water_classification()[ocean_site];
    assert_eq!(u32::from(source_class), SurfaceWaterKind::Ocean.raw());
    let mid = centres[centres.len() / 2];
    let ocean = site(ocean_site);
    let segments = (central_angle(mid, ocean) / step).ceil() as usize;
    let transect = (0..=segments)
        .map(|i| chord_point(mid, ocean, i as f64 / segments as f64))
        .chain(centres.iter().copied())
        .map(UnitVector3::components)
        .collect::<Vec<_>>();
    let beds = fine.sample_surface_batch(&transect, &|| false).unwrap();
    assert!(
        beds.iter().all(|point| point.bed_elevation_m < 0.0),
        "the path to the ocean site must be submerged"
    );

    let directions = centres
        .iter()
        .flat_map(|centre| {
            [-1.0, 1.0].map(|side| unit(add(centre.components(), scale(offset, side))).components())
        })
        .collect::<Vec<_>>();
    let samples = snapshot
        .sample_surface_batch(&directions, &|| false)
        .unwrap();
    assert!(samples.iter().all(|sample| sample.bed_elevation_m < 0.0));
    let missed_pairs = samples
        .chunks_exact(2)
        .filter(|pair| pair.iter().any(|p| p.water != SurfaceWaterKind::Ocean))
        .count();
    let highest_path_m = beds[..=segments]
        .iter()
        .map(|p| p.bed_elevation_m)
        .fold(f64::NEG_INFINITY, f64::max);
    log::debug!(
        "Atlas coast ocean_site={ocean_site} path_km={:.2} highest_path_m={highest_path_m:.2} \
        pairs={} missing_ocean_pairs={missed_pairs}",
        central_angle(mid, ocean) * snapshot.surface().radius().get() / 1000.0,
        centres.len()
    );
    assert_eq!(
        missed_pairs, 0,
        "pairs missing ocean on the submerged source edge"
    );
}
