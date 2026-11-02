//! Area sampling of the endpoint P4 climate for one author-chosen region.
//!
//! Design truth: `docs/superpowers/specs/2026-10-26-site-climatology-export-design.md`.
//! Every function here is a pure, read-only query over published products.

use std::collections::VecDeque;
use std::f64::consts::PI;

use crate::world::natural::{
    saturation_specific_humidity_kg_kg, AnchorAssessment, ClimateCapabilityAvailability,
    ClimateCapabilityId, ClimateLayerRole, GlobalCirculationSnapshot, SiteAvailability,
    SiteCapabilityId, SiteCapabilityReason, SiteCapabilityStatus, SiteClimatology,
    SiteClimatologyError, SiteClimatologyMonth, SiteClimatologySource, SiteElevationSummary,
    SiteMethods, SiteRegion, SiteRegionSummary, SiteSubstitute, SurfaceWaterField,
    SurfaceWaterKind, ANCHOR_INLAND_MIN_CLIMATE_SPACINGS, ANCHOR_RECOMMENDATION_COUNT,
    CELSIUS_TO_KELVIN_OFFSET_K, CLIMATE_MONTH_COUNT, CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
    EADY_GROWTH_RATE_COEFFICIENT, EARTH_ROTATION_RATE_RAD_S,
    P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K, SITE_CLIMATOLOGY_SCHEMA_VERSION,
    STANDARD_GRAVITY_M_S2, WET_DAY_THRESHOLD_MM,
};
use crate::world::spatial::{
    canonical_east_north_basis, central_angle, gnomonic_forward, gnomonic_inverse,
    latitude_longitude_radians, SphericalSurfaceSnapshot, UnitVector3,
};
use crate::world::CellId;

const SECONDS_PER_DAY: f64 = 86_400.0;

/// Published products a site query reads; all share the authoritative cells.
#[derive(Debug, Clone, Copy)]
pub struct SiteClimatologyInputs<'a> {
    /// Authoritative surface geometry.
    pub surface: &'a SphericalSurfaceSnapshot,
    /// Endpoint P4 snapshot on the same surface.
    pub climate: &'a GlobalCirculationSnapshot,
    /// Published dry-land / ocean / lake classification.
    pub surface_water: &'a SurfaceWaterField,
    /// Current surface elevation, metres.
    pub elevation_m: &'a [f32],
    /// Cubed-sphere face resolution of the P4 work domain.
    pub climate_face_resolution: u16,
}

impl SiteClimatologyInputs<'_> {
    fn validate(&self) -> Result<(), SiteClimatologyError> {
        let expected = self.surface.cells().len();
        for (field, found) in [
            ("climate", self.climate.fields().cell_count()),
            ("surface_water", self.surface_water.len()),
            ("elevation_m", self.elevation_m.len()),
        ] {
            if found != expected {
                return Err(SiteClimatologyError::CellCountMismatch {
                    field,
                    found,
                    expected,
                });
            }
        }
        Ok(())
    }

    fn radius_m(&self) -> f64 {
        self.surface.radius().get()
    }

    fn kind(&self, cell: CellId) -> SurfaceWaterKind {
        self.surface_water
            .get(cell.raw() as usize)
            .expect("validated surface-water field covers every cell")
    }

    fn centroid(&self, cell: CellId) -> UnitVector3 {
        self.surface.cells()[cell.raw() as usize].centroid
    }

    fn neighbors(&self, cell: CellId) -> impl Iterator<Item = CellId> + '_ {
        self.surface
            .cell_edges(cell)
            .unwrap_or_default()
            .iter()
            .filter_map(move |edge| self.surface.opposite_cell(cell, *edge))
    }

    /// Largest site-to-site distance across any edge, as a unit-sphere angle.
    fn max_neighbor_angle(&self) -> f64 {
        let radius = self.radius_m();
        self.surface
            .edges()
            .iter()
            .map(|edge| edge.center_distance.get() / radius)
            .fold(0.0, f64::max)
    }
}

/// Mean cell spacing of the P4 cubed-sphere work grid, metres.
///
/// The grid tiles the sphere with `6 N^2` cells, so the square root of the
/// mean cell area is the representative spacing. Both `N` and the radius are
/// read from the production work domain; no constant is introduced.
pub fn climate_grid_spacing_m(radius_m: f64, climate_face_resolution: u16) -> f64 {
    let n = f64::from(climate_face_resolution);
    (4.0 * PI * radius_m * radius_m / (6.0 * n * n)).sqrt()
}

/// The sampled square: a rotated square in the gnomonic tangent plane at the
/// anchor (Snyder 1987, §22), widened to the P4 grid spacing when smaller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiteFootprint {
    anchor: UnitVector3,
    radius_m: f64,
    heading_rad: f64,
    requested_side_m: f64,
    climate_spacing_m: f64,
}

impl SiteFootprint {
    /// Resolves the footprint of a region on the given surface.
    pub fn new(
        inputs: &SiteClimatologyInputs<'_>,
        region: &SiteRegion,
    ) -> Result<Self, SiteClimatologyError> {
        let anchor = inputs
            .surface
            .cell(region.anchor_cell())
            .ok_or(SiteClimatologyError::UnknownAnchor)?
            .centroid;
        Ok(Self {
            anchor,
            radius_m: inputs.radius_m(),
            heading_rad: region.heading_deg().to_radians(),
            requested_side_m: 2.0 * region.scale() * region.half_extent_m(),
            climate_spacing_m: climate_grid_spacing_m(
                inputs.radius_m(),
                inputs.climate_face_resolution,
            ),
        })
    }

    /// Anchor direction.
    pub const fn anchor(&self) -> UnitVector3 {
        self.anchor
    }

    /// Requested side length on the planet, metres.
    pub const fn requested_side_m(&self) -> f64 {
        self.requested_side_m
    }

    /// Mean P4 grid spacing, metres.
    pub const fn climate_spacing_m(&self) -> f64 {
        self.climate_spacing_m
    }

    /// Sampled side length, metres.
    pub fn effective_side_m(&self) -> f64 {
        self.requested_side_m.max(self.climate_spacing_m)
    }

    /// Whether the footprint was widened to the climate grid spacing.
    pub fn expanded(&self) -> bool {
        self.requested_side_m < self.climate_spacing_m
    }

    /// Game-frame coordinates `[x, z]` of a direction, metres on the tangent plane.
    fn game_coordinates_m(&self, direction: UnitVector3) -> Option<[f64; 2]> {
        let [east, north] = gnomonic_forward(self.anchor, direction)?;
        let (sin, cos) = self.heading_rad.sin_cos();
        Some([
            self.radius_m * (east * cos - north * sin),
            self.radius_m * (-east * sin - north * cos),
        ])
    }

    /// Whether a direction lies inside the effective footprint.
    pub fn contains(&self, direction: UnitVector3) -> bool {
        let half = 0.5 * self.effective_side_m();
        self.game_coordinates_m(direction)
            .is_some_and(|[x, z]| x.abs() <= half && z.abs() <= half)
    }

    /// Closed outline of a centred square of the given side, sampled along
    /// each side, for presentation overlays.
    pub fn outline(&self, side_m: f64, samples_per_side: usize) -> Vec<UnitVector3> {
        let half = 0.5 * side_m / self.radius_m;
        let corners = [[-half, -half], [half, -half], [half, half], [-half, half]];
        let (sin, cos) = self.heading_rad.sin_cos();
        let mut points = Vec::with_capacity(4 * samples_per_side + 1);
        for side in 0..4 {
            let start = corners[side];
            let end = corners[(side + 1) % 4];
            for step in 0..samples_per_side {
                let t = step as f64 / samples_per_side as f64;
                let x = start[0] + (end[0] - start[0]) * t;
                let z = start[1] + (end[1] - start[1]) * t;
                // The game rotation is an involution: the same matrix maps back.
                points.push(gnomonic_inverse(
                    self.anchor,
                    [x * cos - z * sin, -x * sin - z * cos],
                ));
            }
        }
        points.push(points[0]);
        points
    }

    /// Angular radius of the circle circumscribing the effective footprint.
    fn circumradius_angle(&self) -> f64 {
        (0.5 * self.effective_side_m() * std::f64::consts::SQRT_2 / self.radius_m).atan()
    }
}

/// Cells whose centroid lies in the footprint, by breadth-first search from
/// the anchor cell. Expansion continues through every cell within the
/// circumscribed circle plus one neighbor spacing, which reaches every cell
/// that can intersect the footprint.
fn footprint_cells(
    inputs: &SiteClimatologyInputs<'_>,
    footprint: &SiteFootprint,
    anchor_cell: CellId,
    max_neighbor_angle: f64,
) -> Vec<CellId> {
    let bound = footprint.circumradius_angle() + max_neighbor_angle;
    let mut visited = std::collections::HashSet::from([anchor_cell]);
    let mut queue = VecDeque::from([anchor_cell]);
    let mut inside = Vec::new();
    while let Some(cell) = queue.pop_front() {
        let centroid = inputs.centroid(cell);
        if footprint.contains(centroid) {
            inside.push(cell);
        }
        if central_angle(footprint.anchor(), centroid) > bound {
            continue;
        }
        for neighbor in inputs.neighbors(cell) {
            if visited.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    inside.sort_unstable();
    inside
}

/// Ocean cells adjacent to any non-ocean cell.
fn coastal_ocean_cells(inputs: &SiteClimatologyInputs<'_>) -> Vec<CellId> {
    inputs
        .surface
        .cells()
        .iter()
        .map(|cell| cell.id)
        .filter(|cell| {
            inputs.kind(*cell) == SurfaceWaterKind::Ocean
                && inputs
                    .neighbors(*cell)
                    .any(|neighbor| inputs.kind(neighbor) != SurfaceWaterKind::Ocean)
        })
        .collect()
}

/// Great-circle distance to the nearest coastal ocean centroid, metres;
/// `None` on a planet without ocean.
fn coast_distance_m(
    inputs: &SiteClimatologyInputs<'_>,
    coastal: &[CellId],
    direction: UnitVector3,
) -> Option<f64> {
    coastal
        .iter()
        .map(|cell| central_angle(direction, inputs.centroid(*cell)))
        .reduce(f64::min)
        .map(|angle| angle * inputs.radius_m())
}

struct FootprintStatistics {
    cells: Vec<CellId>,
    total_area: f64,
    land_fraction: f64,
    lake_fraction: f64,
    lake_cells: u32,
    elevation: SiteElevationSummary,
}

fn footprint_statistics(
    inputs: &SiteClimatologyInputs<'_>,
    cells: Vec<CellId>,
) -> Result<FootprintStatistics, SiteClimatologyError> {
    if cells.is_empty() {
        return Err(SiteClimatologyError::EmptyFootprint);
    }
    let mut total_area = 0.0;
    let mut land_area = 0.0;
    let mut lake_area = 0.0;
    let mut lake_cells = 0;
    let mut weighted_elevation = 0.0;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for cell in &cells {
        let area = inputs.surface.cells()[cell.raw() as usize].area.get();
        let elevation = f64::from(inputs.elevation_m[cell.raw() as usize]);
        total_area += area;
        weighted_elevation += area * elevation;
        min = min.min(elevation);
        max = max.max(elevation);
        match inputs.kind(*cell) {
            SurfaceWaterKind::DryLand => land_area += area,
            SurfaceWaterKind::Lake => {
                lake_area += area;
                lake_cells += 1;
            }
            SurfaceWaterKind::Ocean => {}
        }
    }
    Ok(FootprintStatistics {
        cells,
        total_area,
        land_fraction: land_area / total_area,
        lake_fraction: lake_area / total_area,
        lake_cells,
        elevation: SiteElevationSummary {
            mean: weighted_elevation / total_area,
            min,
            max,
        },
    })
}

fn assess_anchor(
    inputs: &SiteClimatologyInputs<'_>,
    footprint: &SiteFootprint,
    anchor_cell: CellId,
    statistics: &FootprintStatistics,
    coast_distance_m: Option<f64>,
) -> AnchorAssessment {
    let inland_threshold_m = ANCHOR_INLAND_MIN_CLIMATE_SPACINGS * footprint.climate_spacing_m();
    let anchor_is_land = inputs.kind(anchor_cell) == SurfaceWaterKind::DryLand;
    let inland = coast_distance_m.is_none_or(|distance| distance >= inland_threshold_m);
    let footprint_dry_land_only = statistics
        .cells
        .iter()
        .all(|cell| inputs.kind(*cell) == SurfaceWaterKind::DryLand);
    AnchorAssessment {
        coast_distance_m,
        inland_threshold_m,
        anchor_is_land,
        inland,
        footprint_dry_land_only,
        footprint_lake_cells: statistics.lake_cells,
        footprint_relief_m: statistics.elevation.max - statistics.elevation.min,
        meets_criteria: anchor_is_land && inland && footprint_dry_land_only,
    }
}

/// Vertical distance between the reference mid-heights of the lower and
/// upper atmosphere layers, metres; `None` without an upper layer.
fn layer_midpoint_separation_m(climate: &GlobalCirculationSnapshot) -> Option<f64> {
    let thickness = |role| {
        climate
            .layout()
            .layers()
            .iter()
            .find(|layer| layer.role() == role)
            .map(|layer| layer.reference_thickness_m())
    };
    let lower = thickness(ClimateLayerRole::LowerAtmosphere)?;
    let upper = thickness(ClimateLayerRole::UpperAtmosphere)?;
    Some(0.5 * (lower + upper))
}

/// Dry Brunt–Väisälä frequency squared, `N^2 = (g/T)(g/c_p - Gamma)`
/// (Holton & Hakim 2013, §2.7.3), with the U.S. Standard Atmosphere 1976
/// environmental lapse rate the P4 forcing already uses.
fn dry_buoyancy_frequency_squared(air_temperature_c: f64) -> f64 {
    STANDARD_GRAVITY_M_S2 / (air_temperature_c + CELSIUS_TO_KELVIN_OFFSET_K)
        * (STANDARD_GRAVITY_M_S2 / P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K
            - CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M)
}

/// Maximum Eady growth rate per day,
/// `sigma = 0.31 |f| |dU/dz| / N` (Lindzen & Farrell 1980).
pub fn eady_growth_rate_per_day(
    latitude_rad: f64,
    shear_m_s: f64,
    layer_separation_m: f64,
    air_temperature_c: f64,
) -> f64 {
    let coriolis = 2.0 * EARTH_ROTATION_RATE_RAD_S * latitude_rad.sin().abs();
    let buoyancy = dry_buoyancy_frequency_squared(air_temperature_c).sqrt();
    EADY_GROWTH_RATE_COEFFICIENT * coriolis * (shear_m_s / layer_separation_m) / buoyancy
        * SECONDS_PER_DAY
}

/// Geographic bearing a flow `[east, north]` blows toward, degrees in `[0, 360)`.
pub fn toward_bearing_deg(east_north: [f64; 2]) -> f64 {
    east_north[0]
        .atan2(east_north[1])
        .to_degrees()
        .rem_euclid(360.0)
}

fn capabilities(climate: &GlobalCirculationSnapshot) -> Vec<SiteCapabilityStatus> {
    let available = |id| SiteCapabilityStatus {
        id,
        availability: SiteAvailability::Available,
        reason: None,
        substitute: None,
    };
    let unavailable = |id, reason| SiteCapabilityStatus {
        id,
        availability: SiteAvailability::Unavailable,
        reason: Some(reason),
        substitute: None,
    };
    let layered = climate.fields().upper_wind_m_s().is_some()
        && climate.fields().vertical_wind_shear_m_s().is_some();
    let vertical = |id| {
        if layered {
            available(id)
        } else {
            unavailable(id, SiteCapabilityReason::NoUpperLayer)
        }
    };
    let steering = if layered {
        available(SiteCapabilityId::SteeringWind)
    } else {
        SiteCapabilityStatus {
            substitute: Some(SiteSubstitute::NearSurfaceWind),
            ..unavailable(
                SiteCapabilityId::SteeringWind,
                SiteCapabilityReason::NoUpperLayer,
            )
        }
    };
    let tropical_cyclones = match climate
        .capabilities()
        .availability(ClimateCapabilityId::TropicalCycloneClimatologyV1)
    {
        ClimateCapabilityAvailability::Available => {
            available(SiteCapabilityId::TropicalCycloneClimatology)
        }
        ClimateCapabilityAvailability::EvaluatedNotApplicable => SiteCapabilityStatus {
            id: SiteCapabilityId::TropicalCycloneClimatology,
            availability: SiteAvailability::EvaluatedNotApplicable,
            reason: None,
            substitute: None,
        },
        ClimateCapabilityAvailability::Unavailable => unavailable(
            SiteCapabilityId::TropicalCycloneClimatology,
            SiteCapabilityReason::P4CapabilityUnavailable,
        ),
    };
    vec![
        steering,
        vertical(SiteCapabilityId::VerticalWindShear),
        vertical(SiteCapabilityId::EadyGrowthRate),
        available(SiteCapabilityId::RelativeHumidity),
        unavailable(
            SiteCapabilityId::WetDayStatistics,
            SiteCapabilityReason::NoVerifiedWetDayRelationship,
        ),
        unavailable(
            SiteCapabilityId::ConvectivePrecipitationShare,
            SiteCapabilityReason::NoPrecipitationTypeDiagnostic,
        ),
        unavailable(
            SiteCapabilityId::DiurnalTemperatureRange,
            SiteCapabilityReason::NoDiurnalCycle,
        ),
        unavailable(
            SiteCapabilityId::WindVariability,
            SiteCapabilityReason::NoTransientStatistics,
        ),
        unavailable(
            SiteCapabilityId::CloudCover,
            SiteCapabilityReason::CloudsNotImplemented,
        ),
        tropical_cyclones,
    ]
}

/// Samples the region and assembles the complete export document.
pub fn sample_site_climatology(
    inputs: &SiteClimatologyInputs<'_>,
    region: &SiteRegion,
    source: SiteClimatologySource,
) -> Result<SiteClimatology, SiteClimatologyError> {
    inputs.validate()?;
    let footprint = SiteFootprint::new(inputs, region)?;
    let cells = footprint_cells(
        inputs,
        &footprint,
        region.anchor_cell(),
        inputs.max_neighbor_angle(),
    );
    let statistics = footprint_statistics(inputs, cells)?;
    let coastal = coastal_ocean_cells(inputs);
    let coast = coast_distance_m(inputs, &coastal, footprint.anchor());
    let assessment = assess_anchor(inputs, &footprint, region.anchor_cell(), &statistics, coast);

    let anchor = footprint.anchor();
    let (latitude, longitude) = latitude_longitude_radians(anchor);
    let (east, north) = canonical_east_north_basis(anchor);
    let project = |vector: [f32; 3]| {
        let vector = vector.map(f64::from);
        [
            vector[0] * east[0] + vector[1] * east[1] + vector[2] * east[2],
            vector[0] * north[0] + vector[1] * north[1] + vector[2] * north[2],
        ]
    };
    let fields = inputs.climate.fields();
    let separation = layer_midpoint_separation_m(inputs.climate);
    let months = (0..CLIMATE_MONTH_COUNT)
        .map(|month| {
            let mut sums = [0.0_f64; 5];
            let mut near = [0.0_f64; 2];
            let mut upper = [0.0_f64; 2];
            let mut shear = [0.0_f64; 2];
            for cell in &statistics.cells {
                let index = cell.raw() as usize;
                let weight = inputs.surface.cells()[index].area.get() / statistics.total_area;
                let temperature =
                    f64::from(fields.monthly_air_temperature_c().values()[index][month]);
                let humidity = f64::from(fields.monthly_specific_humidity().values()[index][month]);
                sums[0] += weight * temperature;
                sums[1] += weight
                    * f64::from(fields.monthly_precipitation_mm_day().values()[index][month]);
                sums[2] += weight
                    * f64::from(
                        fields.monthly_orographic_precipitation_mm_day().values()[index][month],
                    );
                sums[3] += weight * humidity;
                sums[4] += weight * humidity / saturation_specific_humidity_kg_kg(temperature);
                let accumulate = |target: &mut [f64; 2], vector: [f32; 3]| {
                    let [e, n] = project(vector);
                    target[0] += weight * e;
                    target[1] += weight * n;
                };
                accumulate(
                    &mut near,
                    fields.near_surface_wind_m_s().values()[index][month],
                );
                if let Some(field) = fields.upper_wind_m_s() {
                    accumulate(&mut upper, field.values()[index][month]);
                }
                if let Some(field) = fields.vertical_wind_shear_m_s() {
                    accumulate(&mut shear, field.values()[index][month]);
                }
            }
            let steering = fields.upper_wind_m_s().map(|_| upper);
            let shear = fields.vertical_wind_shear_m_s().map(|_| shear);
            let eady = shear.zip(separation).map(|(shear, separation)| {
                eady_growth_rate_per_day(latitude, shear[0].hypot(shear[1]), separation, sums[0])
            });
            SiteClimatologyMonth {
                month: (month + 1) as u8,
                air_temperature_c: sums[0],
                precipitation_mm_day: sums[1],
                orographic_precipitation_mm_day: sums[2],
                specific_humidity_kg_kg: sums[3],
                relative_humidity: sums[4],
                near_surface_wind_en_m_s: near,
                steering_wind_en_m_s: steering,
                wind_shear_en_m_s: shear,
                steering_flow_toward_bearing_deg: toward_bearing_deg(steering.unwrap_or(near)),
                eady_growth_rate_per_day: eady,
                wet_day_fraction: None,
                p_wet_given_dry: None,
                p_wet_given_wet: None,
                mean_wet_day_amount_mm: None,
            }
        })
        .collect();

    Ok(SiteClimatology {
        schema_version: SITE_CLIMATOLOGY_SCHEMA_VERSION,
        source,
        region: SiteRegionSummary {
            anchor_cell: region.anchor_cell().raw(),
            anchor_lat_deg: latitude.to_degrees(),
            anchor_lon_deg: longitude.to_degrees(),
            heading_deg: region.heading_deg(),
            scale: region.scale(),
            half_extent_m: region.half_extent_m(),
            requested_footprint_side_m: footprint.requested_side_m(),
            climate_grid_spacing_m: footprint.climate_spacing_m(),
            effective_footprint_side_m: footprint.effective_side_m(),
            footprint_expanded: footprint.expanded(),
            cell_count: statistics.cells.len() as u32,
            land_fraction: statistics.land_fraction,
            lake_fraction: statistics.lake_fraction,
            elevation_m: statistics.elevation,
        },
        anchor_assessment: assessment,
        wet_day_threshold_mm: WET_DAY_THRESHOLD_MM,
        capabilities: capabilities(inputs.climate),
        months,
        methods: SiteMethods {
            aggregation: "area_weighted_centroids_in_rotated_gnomonic_square_v1",
            wind_projection: "tangent_vector_onto_anchor_east_north_v1",
            relative_humidity: "cell_q_over_p4_bolton_saturation_area_mean_v1",
            steering_flow_bearing: "toward_bearing_atan2_east_north_v1",
            eady_growth_rate: "lindzen_farrell_1980_dry_standard_lapse_v1",
            wet_day_threshold: "new_et_al_2002_cru_cl_2_0_gt_0_1_mm",
        },
    })
}

/// One recommended anchor with the assessment that qualified it.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteAnchorCandidate {
    /// Candidate anchor cell.
    pub cell: CellId,
    /// Criteria evaluation recorded for the candidate.
    pub assessment: AnchorAssessment,
}

/// Hop distance from the nearest coastal ocean cell over cell adjacency.
fn coastal_hops(inputs: &SiteClimatologyInputs<'_>, coastal: &[CellId]) -> Vec<u32> {
    let mut hops = vec![u32::MAX; inputs.surface.cells().len()];
    let mut queue = VecDeque::new();
    for cell in coastal {
        hops[cell.raw() as usize] = 0;
        queue.push_back(*cell);
    }
    while let Some(cell) = queue.pop_front() {
        let next = hops[cell.raw() as usize] + 1;
        for neighbor in inputs.neighbors(cell) {
            if hops[neighbor.raw() as usize] == u32::MAX {
                hops[neighbor.raw() as usize] = next;
                queue.push_back(neighbor);
            }
        }
    }
    hops
}

/// Recommends inland, dry-land-only, high-relief anchors for the region's
/// mapping (Voxim `Docs/Weather.md` §7 item 4), ranked by footprint relief.
///
/// A hop count `h` from the coast bounds the great-circle distance by
/// `h` times the largest neighbor spacing, so cells that cannot reach the
/// inland threshold are rejected before the exact distance is computed.
/// Accepted candidates are at least one effective footprint side apart.
pub fn recommend_site_anchors(
    inputs: &SiteClimatologyInputs<'_>,
    region: &SiteRegion,
) -> Result<Vec<SiteAnchorCandidate>, SiteClimatologyError> {
    inputs.validate()?;
    let max_neighbor_angle = inputs.max_neighbor_angle();
    let coastal = coastal_ocean_cells(inputs);
    let hops = coastal_hops(inputs, &coastal);
    let spacing = climate_grid_spacing_m(inputs.radius_m(), inputs.climate_face_resolution);
    let threshold_m = ANCHOR_INLAND_MIN_CLIMATE_SPACINGS * spacing;
    let max_neighbor_m = max_neighbor_angle * inputs.radius_m();

    let mut qualified = Vec::new();
    for cell in inputs.surface.cells().iter().map(|cell| cell.id) {
        if inputs.kind(cell) != SurfaceWaterKind::DryLand
            || (hops[cell.raw() as usize] != u32::MAX
                && f64::from(hops[cell.raw() as usize]) * max_neighbor_m < threshold_m)
        {
            continue;
        }
        let candidate_region = region.with_anchor(cell);
        let footprint = SiteFootprint::new(inputs, &candidate_region)?;
        let coast = coast_distance_m(inputs, &coastal, footprint.anchor());
        if coast.is_some_and(|distance| distance < threshold_m) {
            continue;
        }
        let cells = footprint_cells(inputs, &footprint, cell, max_neighbor_angle);
        let statistics = footprint_statistics(inputs, cells)?;
        let assessment = assess_anchor(inputs, &footprint, cell, &statistics, coast);
        if assessment.meets_criteria {
            qualified.push((footprint, SiteAnchorCandidate { cell, assessment }));
        }
    }
    qualified.sort_by(|(_, a), (_, b)| {
        b.assessment
            .footprint_relief_m
            .total_cmp(&a.assessment.footprint_relief_m)
            .then(a.cell.cmp(&b.cell))
    });

    let mut chosen: Vec<(SiteFootprint, SiteAnchorCandidate)> = Vec::new();
    for (footprint, candidate) in qualified {
        if chosen.len() == ANCHOR_RECOMMENDATION_COUNT {
            break;
        }
        let separated = chosen.iter().all(|(other, _)| {
            central_angle(other.anchor(), footprint.anchor()) * inputs.radius_m()
                >= footprint.effective_side_m()
        });
        if separated {
            chosen.push((footprint, candidate));
        }
    }
    Ok(chosen.into_iter().map(|(_, candidate)| candidate).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eady_growth_rate_matches_a_hand_computed_midlatitude_value() {
        // 45°N, 20 m/s shear over 5 km, 0 °C: f = 1.0313e-4 s^-1,
        // N = 1.0831e-2 s^-1, so sigma = 0.31 f (4e-3) / N = 1.0201 day^-1.
        let sigma = eady_growth_rate_per_day(45.0_f64.to_radians(), 20.0, 5_000.0, 0.0);
        assert!((sigma - 1.020_072_781).abs() < 1.0e-6, "{sigma}");
    }

    #[test]
    fn toward_bearing_follows_the_geographic_compass() {
        assert_eq!(toward_bearing_deg([0.0, 1.0]), 0.0);
        assert!((toward_bearing_deg([1.0, 0.0]) - 90.0).abs() < 1.0e-12);
        assert!((toward_bearing_deg([0.0, -1.0]) - 180.0).abs() < 1.0e-12);
        assert!((toward_bearing_deg([-1.0, 0.0]) - 270.0).abs() < 1.0e-12);
    }

    #[test]
    fn climate_spacing_is_the_root_mean_cubed_sphere_cell_area() {
        // 4 pi R^2 / (6 * 24^2) at R = 6371 km.
        let spacing = climate_grid_spacing_m(6_371_000.0, 24);
        assert!((spacing - 384_171.966).abs() < 1.0e-2, "{spacing}");
    }

    #[test]
    fn heading_zero_maps_game_x_east_and_z_south() {
        let anchor = UnitVector3::new(1.0, 0.0, 1.0).unwrap();
        let footprint = |heading_deg: f64| SiteFootprint {
            anchor,
            radius_m: 1.0,
            heading_rad: heading_deg.to_radians(),
            requested_side_m: 0.2,
            climate_spacing_m: 0.0,
        };
        let east_point = gnomonic_inverse(anchor, [0.05, 0.0]);
        let south_point = gnomonic_inverse(anchor, [0.0, -0.05]);
        let [x, z] = footprint(0.0).game_coordinates_m(east_point).unwrap();
        assert!((x - 0.05).abs() < 1.0e-12 && z.abs() < 1.0e-12);
        let [x, z] = footprint(0.0).game_coordinates_m(south_point).unwrap();
        assert!(x.abs() < 1.0e-12 && (z - 0.05).abs() < 1.0e-12);
        // Rotating +X clockwise by 90° points it south.
        let [x, z] = footprint(90.0).game_coordinates_m(south_point).unwrap();
        assert!((x - 0.05).abs() < 1.0e-12 && z.abs() < 1.0e-12);
        // Outline corners land exactly on the footprint boundary.
        let rotated = footprint(30.0);
        for point in rotated.outline(0.2, 4) {
            let [x, z] = rotated.game_coordinates_m(point).unwrap();
            assert!((x.abs().max(z.abs()) - 0.1).abs() < 1.0e-12);
        }
    }
}
