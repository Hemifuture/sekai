//! Site climatology export contracts (spec 2026-10-26 §9.1).
//!
//! Synthetic P4 snapshots on one Draft-sized geodesic surface isolate the
//! export's own contracts — area weighting, anchor-basis wind projection,
//! footprint widening, explicit capability degradation, byte determinism and
//! anchor recommendation — without solving P4, which no contract here needs.

use std::sync::OnceLock;

use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{
    climate_grid_spacing_m, eady_growth_rate_per_day, global_circulation_model_fingerprint,
    recommend_site_anchors, sample_site_climatology, SiteClimatologyInputs,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    expected_global_circulation_dense_state_bytes, saturation_specific_humidity_kg_kg,
    site_climatology_fingerprint, ClimateBudgetReport, ClimateCapabilitySet, ClimateCheckpoint,
    ClimateLayerLayout, ClimateModelProfile, ClimateQuantizationId, ClimateRemapReport,
    ClimateSolveReport, GlobalCirculationFields, GlobalCirculationSnapshot, MonthlyScalarField,
    MonthlyVector3Field, NaturalQualityProfile, ProductionIntegratorId, SiteAvailability,
    SiteCapabilityId, SiteClimatology, SiteClimatologySource, SiteRegion, SiteSubstitute,
    SurfaceWaterField, SurfaceWaterKind, ANCHOR_INLAND_MIN_CLIMATE_SPACINGS,
    ANCHOR_RECOMMENDATION_COUNT, GLOBAL_CIRCULATION_SCHEMA_V2, WET_DAY_THRESHOLD_MM,
};
use sekai::world::spatial::{
    canonical_east_north_basis, central_angle, latitude_longitude_radians,
    SphericalSurfaceSnapshot, SurfaceRef,
};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

const FACE_RESOLUTION: u16 = 24;

fn surface() -> &'static SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: NaturalQualityProfile::Draft.authoritative_target_cell_count(),
        })
        .unwrap()
    })
}

fn scalar(count: usize, value: f32) -> MonthlyScalarField {
    MonthlyScalarField::from_values(vec![[value; 12]; count]).unwrap()
}

/// A per-cell tangent field `east_weight * east + north_weight * north`.
fn local_vectors(surface: &SphericalSurfaceSnapshot, east_north: [f64; 2]) -> MonthlyVector3Field {
    MonthlyVector3Field::from_values(
        surface
            .cells()
            .iter()
            .map(|cell| {
                let (east, north) = canonical_east_north_basis(cell.centroid);
                let vector: [f32; 3] = std::array::from_fn(|axis| {
                    (east_north[0] * east[axis] + east_north[1] * north[axis]) as f32
                });
                [vector; 12]
            })
            .collect(),
    )
    .unwrap()
}

/// Upper minus lower, component by component, as P4's shear identity requires.
fn exact_shear(upper: &MonthlyVector3Field, lower: &MonthlyVector3Field) -> MonthlyVector3Field {
    MonthlyVector3Field::from_values(
        upper
            .values()
            .iter()
            .zip(lower.values())
            .map(|(upper, lower)| {
                std::array::from_fn(|month| {
                    std::array::from_fn(|axis| upper[month][axis] - lower[month][axis])
                })
            })
            .collect(),
    )
    .unwrap()
}

fn snapshot(
    surface: &SphericalSurfaceSnapshot,
    profile: ClimateModelProfile,
) -> GlobalCirculationSnapshot {
    let count = surface.cells().len();
    let fields = match profile {
        ClimateModelProfile::C2LayeredV1 => GlobalCirculationFields::new_c2(
            local_vectors(surface, [1.0, 0.0]),
            local_vectors(surface, [1.0, 2.0]),
            exact_shear(
                &local_vectors(surface, [1.0, 2.0]),
                &local_vectors(surface, [1.0, 0.0]),
            ),
            local_vectors(surface, [0.0, 0.0]),
            scalar(count, 12.0),
            scalar(count, 15.0),
            vec![0.1; count],
            scalar(count, 240.0),
            scalar(count, 240.0),
            scalar(count, 8.0),
            scalar(count, 900.0),
            scalar(count, 0.008),
            scalar(count, 0.0),
            scalar(count, 2.0),
            scalar(count, 0.5),
            scalar(count, 0.0),
            scalar(count, 0.0),
            scalar(count, 0.0),
            scalar(count, 0.0),
            scalar(count, 4.0),
        ),
        ClimateModelProfile::C1SingleLayerV1 => GlobalCirculationFields::new_c1(
            local_vectors(surface, [1.0, 0.0]),
            local_vectors(surface, [0.0, 0.0]),
            scalar(count, 12.0),
            scalar(count, 15.0),
            vec![0.1; count],
            scalar(count, 240.0),
            scalar(count, 240.0),
            scalar(count, 0.008),
            scalar(count, 0.0),
            scalar(count, 2.0),
            scalar(count, 0.5),
            scalar(count, 0.0),
            scalar(count, 0.0),
        ),
    }
    .unwrap();
    let checkpoint = ClimateCheckpoint::new(
        NaturalQualityProfile::Draft,
        profile,
        ProductionIntegratorId::SplitExplicitRk3V1,
        *CubedSphereGrid::new(FACE_RESOLUTION, surface.radius().get())
            .unwrap()
            .fingerprint(),
        [2; 32],
        global_circulation_model_fingerprint(profile),
        [3; 32],
        ClimateQuantizationId::DeterministicF64V1,
        24,
        fields.fingerprint(),
    )
    .unwrap();
    GlobalCirculationSnapshot::new(
        GLOBAL_CIRCULATION_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        ClimateLayerLayout::for_profile(profile),
        ProductionIntegratorId::SplitExplicitRk3V1,
        ClimateCapabilitySet::for_profile(profile),
        checkpoint,
        ClimateSolveReport::new(
            2,
            24,
            192,
            0,
            1.0,
            1.0e-5,
            0.41,
            expected_global_circulation_dense_state_bytes(
                NaturalQualityProfile::Draft,
                profile,
                count as u32,
            )
            .unwrap(),
        )
        .unwrap(),
        ClimateBudgetReport::new(1.0e-10, 2.0e-10, 3.0e-10, 4.0e-9, 5.0e-10).unwrap(),
        ClimateRemapReport::new(1.0e-13, 2.0e-13, 3.0e-13, 4.0e-13, 5.0e-8, 100, 100).unwrap(),
        fields,
    )
    .unwrap()
}

/// Ocean south of 20°S, a lake cluster near 60°N, dry land elsewhere; a
/// smooth deterministic relief so recommendations have something to rank.
fn surface_water(surface: &SphericalSurfaceSnapshot) -> SurfaceWaterField {
    SurfaceWaterField::from_kinds(
        surface
            .cells()
            .iter()
            .map(|cell| {
                let (latitude, longitude) = latitude_longitude_radians(cell.centroid);
                if latitude < (-20.0_f64).to_radians() {
                    SurfaceWaterKind::Ocean
                } else if (latitude - 60.0_f64.to_radians()).abs() < 0.05 && longitude.abs() < 0.1 {
                    SurfaceWaterKind::Lake
                } else {
                    SurfaceWaterKind::DryLand
                }
            })
            .collect(),
    )
}

fn elevation(surface: &SphericalSurfaceSnapshot) -> Vec<f32> {
    surface
        .cells()
        .iter()
        .map(|cell| {
            let [x, y, z] = cell.centroid.components();
            (1_500.0 * (7.0 * x).sin() * (5.0 * y).cos() + 800.0 * (9.0 * z).sin()) as f32
        })
        .collect()
}

fn source() -> SiteClimatologySource {
    SiteClimatologySource {
        world_seed: 42,
        surface_fingerprint: "surface".to_owned(),
        build_result_hash: "build".to_owned(),
        graph_contract_version: 1,
        global_circulation_fingerprint: "p4".to_owned(),
        global_circulation_schema: GLOBAL_CIRCULATION_SCHEMA_V2,
        climate_profile: ClimateModelProfile::C2LayeredV1,
        quality_profile: NaturalQualityProfile::Draft,
        sekai_package_version: "test".to_owned(),
    }
}

/// The cell whose centroid is nearest to 45°N, 30°E.
fn midlatitude_anchor(surface: &SphericalSurfaceSnapshot) -> CellId {
    let target = sekai::world::spatial::UnitVector3::new(
        45.0_f64.to_radians().cos() * 30.0_f64.to_radians().cos(),
        45.0_f64.to_radians().cos() * 30.0_f64.to_radians().sin(),
        45.0_f64.to_radians().sin(),
    )
    .unwrap();
    surface
        .cells()
        .iter()
        .min_by(|a, b| {
            central_angle(a.centroid, target).total_cmp(&central_angle(b.centroid, target))
        })
        .unwrap()
        .id
}

fn export(profile: ClimateModelProfile) -> SiteClimatology {
    let surface = surface();
    let climate = snapshot(surface, profile);
    let water = surface_water(surface);
    let elevation = elevation(surface);
    let inputs = SiteClimatologyInputs {
        surface,
        climate: &climate,
        surface_water: &water,
        elevation_m: &elevation,
        climate_face_resolution: FACE_RESOLUTION,
    };
    let region = SiteRegion::new(midlatitude_anchor(surface), 0.0, 1.0, 8_192.0).unwrap();
    sample_site_climatology(&inputs, &region, source()).unwrap()
}

/// Contracts: area-weighted means of uniform fields reproduce the field;
/// per-cell tangent winds project onto the anchor's east/north basis;
/// relative humidity and Eady reuse the production formulas; the Qinglan
/// mapping widens to the P4 grid; wet-day statistics stay explicitly
/// unavailable; and identical inputs serialize to identical bytes.
#[test]
fn c2_export_samples_area_weighted_anchor_projected_climate() {
    let site = export(ClimateModelProfile::C2LayeredV1);
    let region = &site.region;
    assert!(region.footprint_expanded);
    assert_eq!(region.requested_footprint_side_m, 16_384.0);
    assert_eq!(
        region.effective_footprint_side_m,
        climate_grid_spacing_m(6_371_000.0, FACE_RESOLUTION)
    );
    assert!(region.cell_count >= 2, "{}", region.cell_count);

    let latitude = region.anchor_lat_deg.to_radians();
    for month in &site.months {
        assert!((month.air_temperature_c - 12.0).abs() < 1.0e-9);
        assert!((month.precipitation_mm_day - 2.0).abs() < 1.0e-9);
        let expected_rh = f64::from(0.008_f32) / saturation_specific_humidity_kg_kg(12.0);
        assert!((month.relative_humidity - expected_rh).abs() < 1.0e-9);
        // Neighbouring cells' bases rotate slightly; the projection stays
        // within a few hundredths of the uniform local wind.
        let [east, north] = month.near_surface_wind_en_m_s;
        assert!(
            (east - 1.0).abs() < 0.02 && north.abs() < 0.05,
            "{east} {north}"
        );
        let [east, north] = month.steering_wind_en_m_s.unwrap();
        assert!(
            (east - 1.0).abs() < 0.1 && (north - 2.0).abs() < 0.1,
            "{east} {north}"
        );
        // atan2(1, 2) = 26.57° east of north.
        let bearing = month.steering_flow_toward_bearing_deg;
        assert!((bearing - 26.57).abs() < 3.0, "{bearing}");
        let shear = month.wind_shear_en_m_s.unwrap();
        let expected = eady_growth_rate_per_day(latitude, shear[0].hypot(shear[1]), 5_000.0, 12.0);
        assert_eq!(month.eady_growth_rate_per_day, Some(expected));
        assert!(month.wet_day_fraction.is_none() && month.p_wet_given_wet.is_none());
    }
    assert_eq!(site.wet_day_threshold_mm, WET_DAY_THRESHOLD_MM);
    assert_eq!(
        site.availability(SiteCapabilityId::WetDayStatistics),
        SiteAvailability::Unavailable
    );
    assert_eq!(
        site.availability(SiteCapabilityId::SteeringWind),
        SiteAvailability::Available
    );

    let again = export(ClimateModelProfile::C2LayeredV1);
    assert_eq!(site.to_json_bytes(), again.to_json_bytes());
    assert_eq!(
        site_climatology_fingerprint(&site.to_json_bytes()),
        site_climatology_fingerprint(&again.to_json_bytes())
    );
}

/// Contract (§3): without an upper layer the export says so explicitly and
/// names the near-surface substitute instead of leaving nulls to interpret.
#[test]
fn c1_export_degrades_vertical_quantities_explicitly() {
    let site = export(ClimateModelProfile::C1SingleLayerV1);
    for id in [
        SiteCapabilityId::SteeringWind,
        SiteCapabilityId::VerticalWindShear,
        SiteCapabilityId::EadyGrowthRate,
    ] {
        assert_eq!(site.availability(id), SiteAvailability::Unavailable);
    }
    let steering = site
        .capabilities
        .iter()
        .find(|status| status.id == SiteCapabilityId::SteeringWind)
        .unwrap();
    assert_eq!(steering.substitute, Some(SiteSubstitute::NearSurfaceWind));
    for month in &site.months {
        assert!(month.steering_wind_en_m_s.is_none());
        assert!(month.eady_growth_rate_per_day.is_none());
        let bearing = month.steering_flow_toward_bearing_deg;
        assert!((bearing - 90.0).abs() < 3.0, "{bearing}");
    }
}

/// Contract (Voxim `Docs/Weather.md` §7 item 4): every recommended anchor is
/// dry land, at least two P4 spacings from the coast, has no lake or ocean in
/// its footprint, candidates are separated and ranked by footprint relief.
#[test]
fn recommended_anchors_meet_every_inland_criterion() {
    let surface = surface();
    let climate = snapshot(surface, ClimateModelProfile::C2LayeredV1);
    let water = surface_water(surface);
    let elevation = elevation(surface);
    let inputs = SiteClimatologyInputs {
        surface,
        climate: &climate,
        surface_water: &water,
        elevation_m: &elevation,
        climate_face_resolution: FACE_RESOLUTION,
    };
    let region = SiteRegion::new(CellId::from_raw(0), 0.0, 1.0, 8_192.0).unwrap();
    let candidates = recommend_site_anchors(&inputs, &region).unwrap();
    assert_eq!(candidates.len(), ANCHOR_RECOMMENDATION_COUNT);

    let spacing = climate_grid_spacing_m(6_371_000.0, FACE_RESOLUTION);
    for pair in candidates.windows(2) {
        assert!(pair[0].assessment.footprint_relief_m >= pair[1].assessment.footprint_relief_m);
    }
    for (index, candidate) in candidates.iter().enumerate() {
        let assessment = &candidate.assessment;
        assert!(assessment.meets_criteria && assessment.footprint_dry_land_only);
        assert_eq!(assessment.footprint_lake_cells, 0);
        assert!(
            assessment.coast_distance_m.unwrap() >= ANCHOR_INLAND_MIN_CLIMATE_SPACINGS * spacing
        );
        let centroid = surface.cells()[candidate.cell.raw() as usize].centroid;
        for other in &candidates[..index] {
            let other_centroid = surface.cells()[other.cell.raw() as usize].centroid;
            assert!(central_angle(centroid, other_centroid) * 6_371_000.0 >= spacing);
        }
    }
}

mod support;

/// Spec §9.0 Stage 0 measurement on the ordinary Draft/seed-42 corpus member
/// (Release, offline): P4 spacing, sampled-cell counts across every dry-land
/// anchor for the Qinglan mapping, and the recommended anchors. Prints only;
/// numbers are recorded in the spec, not pinned here.
#[test]
#[ignore = "Release-only offline Stage 0 measurement"]
fn stage0_measures_draft_footprints_and_recommendations() {
    let fixture = support::causal_formation::causal_formation_fixture();
    let bundle = fixture.artifact.bundle();
    let formation = bundle.surface_formation();
    let inputs = SiteClimatologyInputs {
        surface: &fixture.surface,
        climate: bundle.climate(),
        surface_water: formation.hydrology().surface_water(),
        elevation_m: formation.terrain_fields().current_elevation_m(),
        climate_face_resolution: NaturalQualityProfile::Draft.climate_face_resolution(),
    };
    println!(
        "P4 spacing: {:.1} km",
        climate_grid_spacing_m(6_371_000.0, inputs.climate_face_resolution) / 1_000.0
    );
    let template = SiteRegion::new(CellId::from_raw(0), 0.0, 1.0, 8_192.0).unwrap();
    let mut counts = Vec::new();
    let mut source_template = source();
    source_template.climate_profile = bundle.climate().profile();
    for cell in fixture.surface.cells() {
        if inputs.surface_water.get(cell.id.raw() as usize) != Some(SurfaceWaterKind::DryLand) {
            continue;
        }
        let site = sample_site_climatology(
            &inputs,
            &template.with_anchor(cell.id),
            source_template.clone(),
        )
        .unwrap();
        counts.push(site.region.cell_count);
    }
    counts.sort_unstable();
    println!(
        "dry-land anchors {}: cells min {} p10 {} median {} p90 {} max {}",
        counts.len(),
        counts[0],
        counts[counts.len() / 10],
        counts[counts.len() / 2],
        counts[counts.len() * 9 / 10],
        counts[counts.len() - 1]
    );
    let started = std::time::Instant::now();
    let candidates = recommend_site_anchors(&inputs, &template).unwrap();
    println!(
        "recommendation: {} candidates in {:.2} s",
        candidates.len(),
        started.elapsed().as_secs_f64()
    );
    for candidate in &candidates {
        let site = sample_site_climatology(
            &inputs,
            &template.with_anchor(candidate.cell),
            source_template.clone(),
        )
        .unwrap();
        let january = &site.months[0];
        let july = &site.months[6];
        println!(
            "cell {} at {:+.1}, {:+.1}: relief {:.0} m, coast {:.0} km, cells {}, \
             T jan/jul {:.1}/{:.1} C, P jan/jul {:.2}/{:.2} mm/d, bearing jan {:.0}, eady jan {:?}",
            candidate.cell.raw(),
            site.region.anchor_lat_deg,
            site.region.anchor_lon_deg,
            candidate.assessment.footprint_relief_m,
            candidate.assessment.coast_distance_m.unwrap_or(f64::NAN) / 1_000.0,
            site.region.cell_count,
            january.air_temperature_c,
            july.air_temperature_c,
            january.precipitation_mm_day,
            july.precipitation_mm_day,
            january.steering_flow_toward_bearing_deg,
            january.eady_growth_rate_per_day,
        );
    }
}
