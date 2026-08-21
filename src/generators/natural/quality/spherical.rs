use std::fmt::Display;

use super::{
    area_weighted_fraction, jaccard_fraction, MetricAccumulator, MetricObservation,
    NaturalQualityReportBuilder, QualityBuildError,
};
use crate::world::natural::{
    CrustKind, LandOceanKind, NaturalQualityReport, QualityMetricId, ReliefSpec,
    ResolvedWorldFormation, SphericalHydroErosionSnapshot, SphericalReliefSnapshot,
    SphericalTectonicSnapshot, SurfaceWaterKind,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

const CONTINENTAL_AREA_MIN: f64 = 0.30;
const CONTINENTAL_AREA_MAX: f64 = 0.45;
const CONTINENTAL_RETENTION_MIN: f64 = 0.75;
const CONTINENTAL_RETENTION_MAX: f64 = 1.15;
const LAND_CRUST_JACCARD_MIN: f64 = 0.75;
const OCEANIC_EMERGENT_MAX: f64 = 0.10;
const OUTLET_AREA_COVERAGE_MIN: f64 = 0.999_999;

/// Evaluates the current authoritative spherical foundation without reading presentation state.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_spherical_foundation_quality(
    surface: &SphericalSurfaceSnapshot,
    formation: &ResolvedWorldFormation,
    relief_spec: &ReliefSpec,
    tectonic: &SphericalTectonicSnapshot,
    relief: &SphericalReliefSnapshot,
    hydro_erosion: &SphericalHydroErosionSnapshot,
) -> Result<NaturalQualityReport, QualityBuildError> {
    validate_inputs(
        surface,
        formation,
        relief_spec,
        tectonic,
        relief,
        hydro_erosion,
    )?;
    evaluate_spherical_foundation_quality_from_validated(
        surface,
        formation,
        relief_spec,
        tectonic,
        relief,
        hydro_erosion,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_spherical_foundation_quality_from_validated(
    surface: &SphericalSurfaceSnapshot,
    formation: &ResolvedWorldFormation,
    relief_spec: &ReliefSpec,
    tectonic: &SphericalTectonicSnapshot,
    relief: &SphericalReliefSnapshot,
    hydro_erosion: &SphericalHydroErosionSnapshot,
) -> Result<NaturalQualityReport, QualityBuildError> {
    let surface_ref = SurfaceRef::from_validated_spherical(surface)
        .map_err(|error| invalid_input("surface identity", error))?;
    let areas = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    let cell_count = u32::try_from(areas.len()).map_err(|_| QualityBuildError::CountOverflow {
        field: "surface cells",
        found: areas.len(),
    })?;
    let total_area = surface.total_cell_area().get();
    let maximum_cell_area_fraction = areas
        .iter()
        .copied()
        .max_by(f64::total_cmp)
        .expect("validated spherical surfaces contain cells")
        / total_area;

    let continental = tectonic
        .crust_kinds()
        .raw_values()
        .iter()
        .map(|&kind| kind == CrustKind::Continental.raw())
        .collect::<Vec<_>>();
    let final_land = hydro_erosion
        .surface()
        .surface_elevation_m()
        .values()
        .iter()
        .map(|&elevation| {
            LandOceanKind::classify(elevation, relief.sea_level_m()) == LandOceanKind::Land
        })
        .collect::<Vec<_>>();

    let continental_fraction = area_weighted_fraction(&continental, &areas)?;
    let expected_continental_fraction =
        f64::from(formation.recommended_continental_crust_fraction());
    let continental_retention =
        divide_observation(continental_fraction.clone(), expected_continental_fraction);
    let actual_land_fraction = area_weighted_fraction(&final_land, &areas)?;
    let land_crust_jaccard = jaccard_fraction(&final_land, &continental, &areas)?;
    let oceanic = continental
        .iter()
        .map(|continental| !continental)
        .collect::<Vec<_>>();
    let oceanic_emergent = subset_fraction(&final_land, &oceanic, &areas)?;

    let hydrology = hydro_erosion.hydrology();
    let non_ocean = hydrology
        .surface_water()
        .raw_values()
        .iter()
        .map(|&kind| kind != SurfaceWaterKind::Ocean.raw())
        .collect::<Vec<_>>();
    let has_basin = hydrology
        .basin_id()
        .iter()
        .map(Option::is_some)
        .collect::<Vec<_>>();
    let outlet_coverage = subset_fraction(&has_basin, &non_ocean, &areas)?;

    let mut builder = NaturalQualityReportBuilder::new(surface_ref);
    builder.record_observation_between(
        metric_id("tectonics", "continental-area-fraction")?,
        continental_fraction,
        CONTINENTAL_AREA_MIN,
        CONTINENTAL_AREA_MAX,
    )?;
    builder.record_observation_between(
        metric_id("tectonics", "continental-retention")?,
        continental_retention,
        CONTINENTAL_RETENTION_MIN,
        CONTINENTAL_RETENTION_MAX,
    )?;

    let requested_land = f64::from(relief_spec.target_land_fraction);
    let land_min = requested_land - maximum_cell_area_fraction;
    let land_max = requested_land + maximum_cell_area_fraction;
    builder.record_between(
        metric_id("relief", "requested-land-area-fraction")?,
        requested_land,
        1,
        land_min,
        land_max,
    )?;
    builder.record_observation_between(
        metric_id("relief", "actual-land-area-fraction")?,
        actual_land_fraction,
        land_min,
        land_max,
    )?;
    builder.record_observation_at_least(
        metric_id("relief", "land-crust-jaccard")?,
        land_crust_jaccard,
        LAND_CRUST_JACCARD_MIN,
    )?;
    builder.record_observation_at_most(
        metric_id("relief", "oceanic-emergent-area-fraction")?,
        oceanic_emergent,
        OCEANIC_EMERGENT_MAX,
    )?;
    builder.record_observation_at_least(
        metric_id("hydrology", "outlet-area-coverage")?,
        outlet_coverage,
        OUTLET_AREA_COVERAGE_MIN,
    )?;
    builder.record_unbounded(
        metric_id("hydrology", "river-segment-count")?,
        hydrology.river_segments().len() as f64,
        1,
    )?;

    let non_finite = count_non_finite_values(
        surface,
        formation,
        relief_spec,
        tectonic,
        relief,
        hydro_erosion,
    )?;
    builder.record_at_most(
        metric_id("quality", "non-finite-value-count")?,
        non_finite.count as f64,
        non_finite.sample_count,
        0.0,
    )?;
    debug_assert_eq!(cell_count, hydro_erosion.cell_count());
    builder.finish()
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    formation: &ResolvedWorldFormation,
    relief_spec: &ReliefSpec,
    tectonic: &SphericalTectonicSnapshot,
    relief: &SphericalReliefSnapshot,
    hydro_erosion: &SphericalHydroErosionSnapshot,
) -> Result<(), QualityBuildError> {
    validate_input("surface", surface.validate())?;
    validate_input("formation", formation.validate())?;
    validate_input("relief spec", relief_spec.validate())?;
    validate_input("tectonics", tectonic.validate_against(surface))?;
    validate_input("relief", relief.validate())?;
    validate_input("hydro-erosion", hydro_erosion.validate())?;
    validate_input(
        "final surface process",
        hydro_erosion.surface().validate_against(surface, relief),
    )?;
    validate_input(
        "final hydrology",
        hydro_erosion.hydrology().validate_against(surface),
    )?;

    validate_spherical_quality_input_identities(
        surface,
        formation,
        relief_spec,
        tectonic,
        relief,
        hydro_erosion,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_spherical_quality_input_identities(
    surface: &SphericalSurfaceSnapshot,
    formation: &ResolvedWorldFormation,
    relief_spec: &ReliefSpec,
    tectonic: &SphericalTectonicSnapshot,
    relief: &SphericalReliefSnapshot,
    hydro_erosion: &SphericalHydroErosionSnapshot,
) -> Result<(), QualityBuildError> {
    validate_input("formation", formation.validate())?;
    validate_input("relief spec", relief_spec.validate())?;
    let authoritative = SurfaceRef::from_validated_spherical(surface)
        .map_err(|error| invalid_input("surface identity", error))?;
    for (input, found) in [
        ("tectonics", tectonic.surface_ref()),
        ("relief", relief.surface_ref()),
        ("hydro-erosion", hydro_erosion.surface_ref()),
    ] {
        if found != authoritative {
            return Err(QualityBuildError::SurfaceMismatch {
                input,
                found,
                expected: authoritative,
            });
        }
    }
    Ok(())
}

fn validate_input<E: Display>(
    input: &'static str,
    result: Result<(), E>,
) -> Result<(), QualityBuildError> {
    result.map_err(|error| invalid_input(input, error))
}

fn invalid_input(input: &'static str, error: impl Display) -> QualityBuildError {
    QualityBuildError::InvalidInput {
        input,
        reason: error.to_string(),
    }
}

fn metric_id(namespace: &str, name: &str) -> Result<QualityMetricId, QualityBuildError> {
    Ok(QualityMetricId::new(namespace, name, 1)?)
}

fn divide_observation(observation: MetricObservation, denominator: f64) -> MetricObservation {
    match observation {
        MetricObservation::Available {
            value,
            sample_count,
        } => MetricObservation::Available {
            value: value / denominator,
            sample_count,
        },
        unavailable @ MetricObservation::Unavailable { .. } => unavailable,
    }
}

fn subset_fraction(
    included: &[bool],
    eligible: &[bool],
    weights: &[f64],
) -> Result<MetricObservation, QualityBuildError> {
    if included.len() != eligible.len() {
        return Err(QualityBuildError::LengthMismatch {
            field: "included",
            found: included.len(),
            expected: eligible.len(),
        });
    }
    if eligible.len() != weights.len() {
        return Err(QualityBuildError::LengthMismatch {
            field: "weights",
            found: weights.len(),
            expected: eligible.len(),
        });
    }
    let mut accumulator = MetricAccumulator::new();
    for ((&included, &eligible), &weight) in included.iter().zip(eligible).zip(weights) {
        accumulator.push(
            if included && eligible { 1.0 } else { 0.0 },
            if eligible { weight } else { 0.0 },
        )?;
    }
    accumulator.finish()
}

#[derive(Debug, Clone, Copy)]
struct NonFiniteSummary {
    count: u64,
    sample_count: u32,
}

#[derive(Default)]
struct NonFiniteCounter {
    count: u64,
    sample_count: u64,
}

impl NonFiniteCounter {
    fn f64(&mut self, value: f64) {
        self.sample_count += 1;
        self.count += u64::from(!value.is_finite());
    }

    fn f32(&mut self, value: f32) {
        self.f64(f64::from(value));
    }

    fn f64s(&mut self, values: &[f64]) {
        values.iter().for_each(|&value| self.f64(value));
    }

    fn f32s(&mut self, values: &[f32]) {
        values.iter().for_each(|&value| self.f32(value));
    }

    fn finish(self) -> Result<NonFiniteSummary, QualityBuildError> {
        let sample_count =
            u32::try_from(self.sample_count).map_err(|_| QualityBuildError::CountOverflow {
                field: "finite-value samples",
                found: usize::try_from(self.sample_count).unwrap_or(usize::MAX),
            })?;
        Ok(NonFiniteSummary {
            count: self.count,
            sample_count,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn count_non_finite_values(
    surface: &SphericalSurfaceSnapshot,
    formation: &ResolvedWorldFormation,
    relief_spec: &ReliefSpec,
    tectonic: &SphericalTectonicSnapshot,
    relief: &SphericalReliefSnapshot,
    hydro_erosion: &SphericalHydroErosionSnapshot,
) -> Result<NonFiniteSummary, QualityBuildError> {
    let mut counter = NonFiniteCounter::default();
    counter.f64(surface.radius().get());
    for vertex in surface.vertices() {
        counter.f64s(&vertex.position.components());
    }
    for cell in surface.cells() {
        counter.f64s(&cell.site.components());
        counter.f64s(&cell.centroid.components());
        counter.f64(cell.area.get());
    }
    for edge in surface.edges() {
        counter.f64s(&edge.midpoint.components());
        counter.f64(edge.length.get());
        counter.f64(edge.center_distance.get());
        counter.f64s(&edge.center_distances_to_midpoint.map(|value| value.get()));
        counter.f64s(&edge.normal_from_first.components());
    }

    counter.f32(formation.recommended_continental_crust_fraction());
    counter.f32(formation.recommended_land_fraction());
    counter.f32(relief_spec.target_land_fraction);
    for plate in tectonic.plates() {
        counter.f64s(&plate.rotation().pole().components());
        counter.f64(plate.rotation().angular_rate_rad_per_year());
    }
    counter.f32s(tectonic.crust_thickness_km());
    counter.f32s(tectonic.crust_age_myr());
    counter.f32s(tectonic.tectonic_elevation_m());
    counter.f32s(tectonic.lineation_east());
    counter.f32s(tectonic.lineation_north());
    counter.f32s(tectonic.orogeny_age_myr());
    for boundary in tectonic.boundaries() {
        counter.f32(boundary.strength);
    }
    for segment in tectonic.boundary_segments() {
        counter.f32(segment.mean_strength());
    }

    counter.f32(relief.sea_level_m());
    for field in [
        relief.crust_base_elevation_m(),
        relief.tectonic_offset_m(),
        relief.volcanic_offset_m(),
        relief.regional_offset_m(),
        relief.elevation_m(),
    ] {
        counter.f32s(field.values());
    }

    let surface_process = hydro_erosion.surface();
    counter.f32s(surface_process.erosion_depth_m());
    counter.f32s(surface_process.deposition_thickness_m());
    counter.f32s(surface_process.surface_elevation_m().values());
    counter.f64s(surface_process.sediment_throughput_m3());
    counter.f64(surface_process.sediment_ocean_delivery_m3());
    counter.f64(surface_process.sediment_endorheic_storage_m3());
    counter.f64(surface_process.sediment_terminal_transfer_m3());

    let hydrology = hydro_erosion.hydrology();
    counter.f32(hydrology.river_discharge_threshold_m3_s());
    counter.f32(hydrology.minimum_lake_depth_m());
    for months in hydrology.monthly_local_runoff_mm() {
        counter.f32s(months);
    }
    for months in hydrology.monthly_discharge_m3_s() {
        counter.f32s(months);
    }
    counter.f32s(hydrology.annual_local_runoff_mm());
    counter.f32s(hydrology.mean_annual_discharge_m3_s());
    counter.f32s(hydrology.drainage_area_km2());
    counter.f32s(hydrology.drainage_surface_elevation_m().values());
    counter.f32s(hydrology.lake_depth_m());
    counter.f64s(hydrology.river_segment_length_m());
    for basin in hydrology.basins() {
        counter.f64(basin.area_km2());
        counter.f32(basin.mean_discharge_m3_s());
    }
    for lake in hydrology.lakes() {
        counter.f32(lake.surface_elevation_m());
        counter.f64(lake.area_km2());
        counter.f64(lake.volume_m3());
    }
    for river in hydrology.river_segments() {
        counter.f32(river.mean_discharge_m3_s());
    }
    counter.finish()
}
