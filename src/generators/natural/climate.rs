use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use thiserror::Error;

use crate::world::natural::{
    ClimateSpec, ClimateSpecError, ClimateValidationError, MonthlyScalarField, MonthlyVectorField,
    PreliminaryClimateSnapshot, ReliefSnapshot, ReliefValidationError, CLIMATE_MONTH_COUNT,
    PRELIMINARY_CLIMATE_SCHEMA_V1,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::CellId;

const MIN_CLIMATE_GRID_CELLS: usize = 16;
const MAX_CLIMATE_GRID_CELLS: usize = 4_096;
const MIN_GRID_AXIS: usize = 4;
const WATER_VAPOR_TRANSPORT_STEPS: usize = 48;
const ENVIRONMENTAL_LAPSE_RATE_C_PER_M: f32 = 0.0065;

/// Deterministic bounded solver for preliminary monthly climate forcing.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClimateGenerator;

impl ClimateGenerator {
    /// Generates current-slice monthly climate from only spatial, relief, and resolved forcing.
    pub fn generate(
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
        spec: &ClimateSpec,
    ) -> Result<PreliminaryClimateSnapshot, ClimateGenerationError> {
        spec.validate()?;
        relief.validate_against(spatial)?;
        let cell_count = u32::try_from(spatial.cell_count()).map_err(|_| {
            ClimateGenerationError::CellCountOverflow {
                found: spatial.cell_count(),
            }
        })?;
        if cell_count == 0 {
            return Err(ClimateGenerationError::EmptySpatialSnapshot);
        }

        let grid = ClimateGrid::aggregate(spatial, relief);
        let maritime = grid.maritime_influence();
        let grid_climate = solve_grid_climate(&grid, &maritime, spec);
        let projected = project_to_cells(spatial, relief, spec, &grid, &maritime, &grid_climate);

        let snapshot = PreliminaryClimateSnapshot::new(
            PRELIMINARY_CLIMATE_SCHEMA_V1,
            cell_count,
            projected.latitude,
            projected.maritime,
            MonthlyScalarField::from_values(projected.temperature)?,
            MonthlyScalarField::from_values(projected.precipitation)?,
            MonthlyVectorField::from_values(projected.wind)?,
            projected.mean_temperature,
            projected.temperature_seasonality,
            projected.annual_precipitation,
            projected.prevailing_wind,
        )?;
        snapshot.validate_against(spatial, relief)?;
        Ok(snapshot)
    }
}

#[derive(Debug)]
struct ClimateGrid {
    cols: usize,
    rows: usize,
    min_x: f32,
    min_y: f32,
    width: f32,
    height: f32,
    elevation_m: Vec<f32>,
    land_fraction: Vec<f32>,
}

impl ClimateGrid {
    fn aggregate(spatial: &SpatialSnapshot, relief: &ReliefSnapshot) -> Self {
        let bounds = spatial.bounds();
        let min_x = bounds.min().x().get() as f32;
        let min_y = bounds.min().y().get() as f32;
        let width = bounds.width().get() as f32;
        let height = bounds.height().get() as f32;
        let (cols, rows) = climate_grid_dimensions(spatial.cell_count(), width, height);
        let grid_count = cols * rows;
        let mut total_area = vec![0.0_f64; grid_count];
        let mut land_area = vec![0.0_f64; grid_count];
        let mut elevation_area = vec![0.0_f64; grid_count];

        for index in 0..spatial.cell_count() {
            let cell = spatial
                .cell(CellId::from_raw(index as u32))
                .expect("validated spatial IDs are dense");
            let bin = grid_index_for_point(
                cell.centroid.x().get() as f32,
                cell.centroid.y().get() as f32,
                min_x,
                min_y,
                width,
                height,
                cols,
                rows,
            );
            let area = cell.area.get();
            let elevation = f64::from(relief.elevation_m().values()[index]);
            total_area[bin] += area;
            elevation_area[bin] += elevation * area;
            if relief.land_ocean().raw_values()[index] == 1 {
                land_area[bin] += area;
            }
        }

        let mut elevation_m = vec![0.0; grid_count];
        let mut land_fraction = vec![0.0; grid_count];
        let mut populated = vec![false; grid_count];
        for index in 0..grid_count {
            if total_area[index] > 0.0 {
                populated[index] = true;
                elevation_m[index] = (elevation_area[index] / total_area[index]) as f32;
                land_fraction[index] = (land_area[index] / total_area[index]) as f32;
            }
        }
        fill_empty_bins(
            cols,
            rows,
            &mut populated,
            &mut elevation_m,
            &mut land_fraction,
        );

        Self {
            cols,
            rows,
            min_x,
            min_y,
            width,
            height,
            elevation_m,
            land_fraction,
        }
    }

    fn len(&self) -> usize {
        self.cols * self.rows
    }

    fn cell_width_m(&self) -> f32 {
        self.width / self.cols as f32
    }

    fn cell_height_m(&self) -> f32 {
        self.height / self.rows as f32
    }

    fn latitude_for_row(&self, row: usize, spec: &ClimateSpec) -> f32 {
        let normalized_y = (row as f32 + 0.5) / self.rows as f32;
        latitude_for_normalized_y(normalized_y, spec)
    }

    fn sample(&self, values: &[f32], world_x: f32, world_y: f32) -> f32 {
        let grid_x = (world_x - self.min_x) / self.width * self.cols as f32 - 0.5;
        let grid_y = (world_y - self.min_y) / self.height * self.rows as f32 - 0.5;
        bilinear_sample(values, self.cols, self.rows, grid_x, grid_y)
    }

    fn sample_monthly_scalar(
        &self,
        values: &[[f32; CLIMATE_MONTH_COUNT]],
        month: usize,
        world_x: f32,
        world_y: f32,
    ) -> f32 {
        let grid_x = (world_x - self.min_x) / self.width * self.cols as f32 - 0.5;
        let grid_y = (world_y - self.min_y) / self.height * self.rows as f32 - 0.5;
        bilinear_sample_by(self.cols, self.rows, grid_x, grid_y, |index| {
            values[index][month]
        })
    }

    fn sample_monthly_vector_component(
        &self,
        values: &[[[f32; 2]; CLIMATE_MONTH_COUNT]],
        month: usize,
        component: usize,
        world_x: f32,
        world_y: f32,
    ) -> f32 {
        let grid_x = (world_x - self.min_x) / self.width * self.cols as f32 - 0.5;
        let grid_y = (world_y - self.min_y) / self.height * self.rows as f32 - 0.5;
        bilinear_sample_by(self.cols, self.rows, grid_x, grid_y, |index| {
            values[index][month][component]
        })
    }

    fn maritime_influence(&self) -> Vec<f32> {
        let ocean = self
            .land_fraction
            .iter()
            .map(|&land_fraction| land_fraction < 0.5)
            .collect::<Vec<_>>();
        let ocean_count = ocean.iter().filter(|&&is_ocean| is_ocean).count();
        if ocean_count == 0 {
            return vec![0.0; self.len()];
        }
        if ocean_count == self.len() {
            return vec![1.0; self.len()];
        }

        let mut distance_steps = vec![usize::MAX; self.len()];
        let mut frontier = VecDeque::new();
        for (index, &is_ocean) in ocean.iter().enumerate() {
            if is_ocean {
                distance_steps[index] = 0;
                frontier.push_back(index);
            }
        }
        while let Some(index) = frontier.pop_front() {
            let next_distance = distance_steps[index] + 1;
            for neighbor in grid_neighbors(index, self.cols, self.rows) {
                if distance_steps[neighbor] == usize::MAX {
                    distance_steps[neighbor] = next_distance;
                    frontier.push_back(neighbor);
                }
            }
        }

        let representative_step = self.cell_width_m().min(self.cell_height_m());
        let decay_distance = (self.width.min(self.height) * 0.16).max(representative_step);
        distance_steps
            .into_iter()
            .map(|steps| {
                (-(steps as f32 * representative_step) / decay_distance)
                    .exp()
                    .clamp(0.0, 1.0)
            })
            .collect()
    }
}

#[derive(Debug)]
struct GridClimate {
    temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    precipitation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    wind: Vec<[[f32; 2]; CLIMATE_MONTH_COUNT]>,
}

#[derive(Debug)]
struct ProjectedClimate {
    latitude: Vec<f32>,
    maritime: Vec<f32>,
    temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    precipitation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    wind: Vec<[[f32; 2]; CLIMATE_MONTH_COUNT]>,
    mean_temperature: Vec<f32>,
    temperature_seasonality: Vec<f32>,
    annual_precipitation: Vec<f32>,
    prevailing_wind: Vec<[f32; 2]>,
}

fn climate_grid_dimensions(cell_count: usize, width: f32, height: f32) -> (usize, usize) {
    let target = (cell_count / 2).clamp(MIN_CLIMATE_GRID_CELLS, MAX_CLIMATE_GRID_CELLS);
    let aspect = (width / height).clamp(0.05, 20.0);
    let mut rows = ((target as f32 / aspect).sqrt().round() as usize).max(MIN_GRID_AXIS);
    let mut cols = ((target as f32 / rows as f32).round() as usize).max(MIN_GRID_AXIS);

    while cols * rows > MAX_CLIMATE_GRID_CELLS {
        if cols >= rows && cols > MIN_GRID_AXIS {
            cols -= 1;
        } else if rows > MIN_GRID_AXIS {
            rows -= 1;
        } else {
            break;
        }
    }
    while cols * rows < MIN_CLIMATE_GRID_CELLS {
        if cols <= rows {
            cols += 1;
        } else {
            rows += 1;
        }
    }
    (cols, rows)
}

#[allow(clippy::too_many_arguments)]
fn grid_index_for_point(
    x: f32,
    y: f32,
    min_x: f32,
    min_y: f32,
    width: f32,
    height: f32,
    cols: usize,
    rows: usize,
) -> usize {
    let col = (((x - min_x) / width) * cols as f32)
        .floor()
        .clamp(0.0, (cols - 1) as f32) as usize;
    let row = (((y - min_y) / height) * rows as f32)
        .floor()
        .clamp(0.0, (rows - 1) as f32) as usize;
    row * cols + col
}

fn fill_empty_bins(
    cols: usize,
    rows: usize,
    populated: &mut [bool],
    elevation_m: &mut [f32],
    land_fraction: &mut [f32],
) {
    let mut frontier = VecDeque::new();
    for (index, &is_populated) in populated.iter().enumerate() {
        if is_populated {
            frontier.push_back(index);
        }
    }
    debug_assert!(!frontier.is_empty());

    while let Some(index) = frontier.pop_front() {
        for neighbor in grid_neighbors(index, cols, rows) {
            if !populated[neighbor] {
                populated[neighbor] = true;
                elevation_m[neighbor] = elevation_m[index];
                land_fraction[neighbor] = land_fraction[index];
                frontier.push_back(neighbor);
            }
        }
    }
}

fn grid_neighbors(index: usize, cols: usize, rows: usize) -> Vec<usize> {
    let col = index % cols;
    let row = index / cols;
    let mut neighbors = Vec::with_capacity(4);
    if col > 0 {
        neighbors.push(index - 1);
    }
    if col + 1 < cols {
        neighbors.push(index + 1);
    }
    if row > 0 {
        neighbors.push(index - cols);
    }
    if row + 1 < rows {
        neighbors.push(index + cols);
    }
    neighbors
}

fn solve_grid_climate(grid: &ClimateGrid, maritime: &[f32], spec: &ClimateSpec) -> GridClimate {
    let mut temperature = vec![[0.0; CLIMATE_MONTH_COUNT]; grid.len()];
    let mut precipitation = vec![[0.0; CLIMATE_MONTH_COUNT]; grid.len()];
    let mut wind = vec![[[0.0; 2]; CLIMATE_MONTH_COUNT]; grid.len()];
    let declinations = std::array::from_fn::<_, CLIMATE_MONTH_COUNT, _>(|month| {
        monthly_declination_degrees(month, spec.axial_tilt_degrees())
    });

    let row_insolation = (0..grid.rows)
        .map(|row| {
            let latitude = grid.latitude_for_row(row, spec);
            std::array::from_fn::<_, CLIMATE_MONTH_COUNT, _>(|month| {
                daily_mean_insolation(latitude, declinations[month])
            })
        })
        .collect::<Vec<_>>();

    for (row, monthly_insolation) in row_insolation.iter().enumerate() {
        let latitude = grid.latitude_for_row(row, spec);
        let annual_insolation = monthly_insolation.iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32;
        for col in 0..grid.cols {
            let index = row * grid.cols + col;
            for month in 0..CLIMATE_MONTH_COUNT {
                let anomaly = if annual_insolation > 1.0e-6 {
                    (monthly_insolation[month] / annual_insolation - 1.0).clamp(-1.4, 1.4)
                } else {
                    0.0
                };
                let sea_level_annual =
                    annual_sea_level_temperature(latitude) + spec.temperature_offset_c();
                let seasonal_response = 18.0 * (0.30 + 0.70 * (1.0 - maritime[index]));
                let lapse_c = grid.elevation_m[index].max(0.0) * ENVIRONMENTAL_LAPSE_RATE_C_PER_M;
                temperature[index][month] =
                    (sea_level_annual + anomaly * seasonal_response - lapse_c).clamp(-100.0, 70.0);
                wind[index][month] =
                    circulation_wind(latitude, declinations[month], maritime[index]);
            }
        }
    }

    for month in 0..CLIMATE_MONTH_COUNT {
        let monthly_temperature = temperature
            .iter()
            .map(|months| months[month])
            .collect::<Vec<_>>();
        let monthly_wind = wind.iter().map(|months| months[month]).collect::<Vec<_>>();
        let monthly_precipitation = solve_monthly_precipitation(
            grid,
            &monthly_temperature,
            &monthly_wind,
            spec.moisture_scale(),
            month,
            spec,
        );
        for index in 0..grid.len() {
            precipitation[index][month] = monthly_precipitation[index];
        }
    }

    GridClimate {
        temperature,
        precipitation,
        wind,
    }
}

fn monthly_declination_degrees(month: usize, axial_tilt_degrees: f32) -> f32 {
    let phase = TAU * (month as f32 - 2.0) / CLIMATE_MONTH_COUNT as f32;
    axial_tilt_degrees * phase.sin()
}

fn daily_mean_insolation(latitude_degrees: f32, declination_degrees: f32) -> f32 {
    let latitude = latitude_degrees.to_radians();
    let declination = declination_degrees.to_radians();
    let hour_angle_argument = -latitude.tan() * declination.tan();
    let sunset_hour_angle = if hour_angle_argument >= 1.0 {
        0.0
    } else if hour_angle_argument <= -1.0 {
        PI
    } else {
        hour_angle_argument.acos()
    };
    ((sunset_hour_angle * latitude.sin() * declination.sin())
        + latitude.cos() * declination.cos() * sunset_hour_angle.sin())
    .max(0.0)
        / PI
}

fn annual_sea_level_temperature(latitude_degrees: f32) -> f32 {
    let latitude_factor = latitude_degrees.to_radians().sin().abs().powf(1.18);
    29.0 - 50.0 * latitude_factor
}

fn circulation_wind(latitude_degrees: f32, declination_degrees: f32, maritime: f32) -> [f32; 2] {
    let effective_latitude = latitude_degrees - declination_degrees * 0.18;
    let absolute_latitude = effective_latitude.abs();
    let zonal = if absolute_latitude <= 20.0 {
        -6.5
    } else if absolute_latitude < 35.0 {
        lerp(-6.5, 7.5, smoothstep((absolute_latitude - 20.0) / 15.0))
    } else if absolute_latitude <= 55.0 {
        7.5
    } else if absolute_latitude < 70.0 {
        lerp(7.5, -4.5, smoothstep((absolute_latitude - 55.0) / 15.0))
    } else {
        -4.5
    };
    let hemisphere = effective_latitude.signum();
    let meridional = if absolute_latitude < 30.0 {
        -1.6 * hemisphere
    } else if absolute_latitude < 60.0 {
        1.1 * hemisphere
    } else {
        -0.8 * hemisphere
    };
    let ocean_acceleration = 0.92 + maritime * 0.16;
    [zonal * ocean_acceleration, meridional * ocean_acceleration]
}

fn solve_monthly_precipitation(
    grid: &ClimateGrid,
    temperature: &[f32],
    wind: &[[f32; 2]],
    moisture_scale: f32,
    month: usize,
    spec: &ClimateSpec,
) -> Vec<f32> {
    let mut vapor = (0..grid.len())
        .map(|index| {
            let ocean_fraction = 1.0 - grid.land_fraction[index];
            let warmth = evaporation_warmth(temperature[index]);
            moisture_scale
                * (ocean_fraction * (0.72 + 0.48 * warmth) + (1.0 - ocean_fraction) * 0.035)
        })
        .collect::<Vec<_>>();
    let mut next = vec![0.0; grid.len()];

    for _ in 0..WATER_VAPOR_TRANSPORT_STEPS {
        for row in 0..grid.rows {
            for col in 0..grid.cols {
                let index = row * grid.cols + col;
                let (upstream_x, upstream_y) =
                    upstream_grid_position(col, row, wind[index], grid.cols, grid.rows);
                let incoming =
                    bilinear_sample(&vapor, grid.cols, grid.rows, upstream_x, upstream_y);
                let upstream_elevation = bilinear_sample(
                    &grid.elevation_m,
                    grid.cols,
                    grid.rows,
                    upstream_x,
                    upstream_y,
                );
                let condensation = condensation_fraction(
                    grid.elevation_m[index],
                    upstream_elevation,
                    temperature[index],
                    grid.latitude_for_row(row, spec),
                    month,
                );
                let ocean_fraction = 1.0 - grid.land_fraction[index];
                let warmth = evaporation_warmth(temperature[index]);
                let equilibrium = moisture_scale * ocean_fraction * (0.72 + 0.48 * warmth);
                let land_recycling =
                    moisture_scale * (1.0 - ocean_fraction) * (0.004 + 0.008 * warmth);
                let advected = incoming * (1.0 - condensation) * 0.992;
                next[index] = if ocean_fraction >= 0.5 {
                    advected.max(equilibrium)
                } else {
                    advected + land_recycling
                };
            }
        }
        std::mem::swap(&mut vapor, &mut next);
    }

    let mut result = vec![0.0; grid.len()];
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let index = row * grid.cols + col;
            let (upstream_x, upstream_y) =
                upstream_grid_position(col, row, wind[index], grid.cols, grid.rows);
            let incoming = bilinear_sample(&vapor, grid.cols, grid.rows, upstream_x, upstream_y);
            let upstream_elevation = bilinear_sample(
                &grid.elevation_m,
                grid.cols,
                grid.rows,
                upstream_x,
                upstream_y,
            );
            let condensation = condensation_fraction(
                grid.elevation_m[index],
                upstream_elevation,
                temperature[index],
                grid.latitude_for_row(row, spec),
                month,
            );
            let ocean_fraction = 1.0 - grid.land_fraction[index];
            let local_recycling = moisture_scale
                * (0.006 + 0.010 * evaporation_warmth(temperature[index]) + 0.010 * ocean_fraction);
            result[index] =
                ((incoming * condensation + local_recycling) * 520.0).clamp(0.0, 4_000.0);
        }
    }
    result
}

fn upstream_grid_position(
    col: usize,
    row: usize,
    wind: [f32; 2],
    cols: usize,
    rows: usize,
) -> (f32, f32) {
    let magnitude = wind[0].hypot(wind[1]).max(1.0e-6);
    let step = 0.95;
    (
        (col as f32 - wind[0] / magnitude * step).clamp(0.0, (cols - 1) as f32),
        (row as f32 - wind[1] / magnitude * step).clamp(0.0, (rows - 1) as f32),
    )
}

fn condensation_fraction(
    elevation_m: f32,
    upstream_elevation_m: f32,
    temperature_c: f32,
    latitude_degrees: f32,
    month: usize,
) -> f32 {
    let uplift = ((elevation_m - upstream_elevation_m).max(0.0) / 1_200.0).min(1.5);
    let warmth = evaporation_warmth(temperature_c);
    let tropical_convergence = (-latitude_degrees.abs() / 15.0).exp();
    let seasonal_pulse = 0.9 + 0.1 * (TAU * month as f32 / CLIMATE_MONTH_COUNT as f32).cos();
    ((0.018 + 0.055 * warmth + 0.075 * tropical_convergence) * seasonal_pulse + 0.42 * uplift)
        .clamp(0.012, 0.78)
}

fn evaporation_warmth(temperature_c: f32) -> f32 {
    ((temperature_c + 12.0) / 42.0).clamp(0.0, 1.0)
}

fn project_to_cells(
    spatial: &SpatialSnapshot,
    relief: &ReliefSnapshot,
    spec: &ClimateSpec,
    grid: &ClimateGrid,
    maritime_grid: &[f32],
    climate_grid: &GridClimate,
) -> ProjectedClimate {
    let count = spatial.cell_count();
    let mut latitude = Vec::with_capacity(count);
    let mut maritime = Vec::with_capacity(count);
    let mut temperature = Vec::with_capacity(count);
    let mut precipitation = Vec::with_capacity(count);
    let mut wind = Vec::with_capacity(count);

    for index in 0..count {
        let cell = spatial
            .cell(CellId::from_raw(index as u32))
            .expect("validated spatial IDs are dense");
        let x = cell.site.x().get() as f32;
        let y = cell.site.y().get() as f32;
        let normalized_y = ((y - grid.min_y) / grid.height).clamp(0.0, 1.0);
        latitude.push(latitude_for_normalized_y(normalized_y, spec));
        maritime.push(grid.sample(maritime_grid, x, y).clamp(0.0, 1.0));
        let sampled_grid_elevation = grid.sample(&grid.elevation_m, x, y).max(0.0);
        let local_elevation = relief.elevation_m().values()[index].max(0.0);
        let local_lapse_adjustment =
            -(local_elevation - sampled_grid_elevation) * ENVIRONMENTAL_LAPSE_RATE_C_PER_M;

        let mut cell_temperature = [0.0; CLIMATE_MONTH_COUNT];
        let mut cell_precipitation = [0.0; CLIMATE_MONTH_COUNT];
        let mut cell_wind = [[0.0; 2]; CLIMATE_MONTH_COUNT];
        for month in 0..CLIMATE_MONTH_COUNT {
            cell_temperature[month] =
                (grid.sample_monthly_scalar(&climate_grid.temperature, month, x, y)
                    + local_lapse_adjustment)
                    .clamp(-100.0, 70.0);
            cell_precipitation[month] = grid
                .sample_monthly_scalar(&climate_grid.precipitation, month, x, y)
                .clamp(0.0, 4_000.0);
            cell_wind[month] = [
                grid.sample_monthly_vector_component(&climate_grid.wind, month, 0, x, y)
                    .clamp(-80.0, 80.0),
                grid.sample_monthly_vector_component(&climate_grid.wind, month, 1, x, y)
                    .clamp(-80.0, 80.0),
            ];
        }
        let annual_precipitation = cell_precipitation.iter().sum::<f32>();
        if annual_precipitation > 20_000.0 {
            let scale = 20_000.0 / annual_precipitation;
            for value in &mut cell_precipitation {
                *value *= scale;
            }
        }
        temperature.push(cell_temperature);
        precipitation.push(cell_precipitation);
        wind.push(cell_wind);
    }

    let mean_temperature = temperature
        .iter()
        .map(|months| months.iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32)
        .collect();
    let temperature_seasonality = temperature
        .iter()
        .map(|months| {
            months.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - months.iter().copied().fold(f32::INFINITY, f32::min)
        })
        .collect();
    let annual_precipitation = precipitation
        .iter()
        .map(|months| months.iter().sum())
        .collect();
    let prevailing_wind = wind
        .iter()
        .map(|months| {
            let sum = months.iter().fold([0.0_f32; 2], |sum, value| {
                [sum[0] + value[0], sum[1] + value[1]]
            });
            [
                sum[0] / CLIMATE_MONTH_COUNT as f32,
                sum[1] / CLIMATE_MONTH_COUNT as f32,
            ]
        })
        .collect();

    ProjectedClimate {
        latitude,
        maritime,
        temperature,
        precipitation,
        wind,
        mean_temperature,
        temperature_seasonality,
        annual_precipitation,
        prevailing_wind,
    }
}

fn latitude_for_normalized_y(normalized_y: f32, spec: &ClimateSpec) -> f32 {
    lerp(
        spec.south_latitude_degrees(),
        spec.north_latitude_degrees(),
        normalized_y.clamp(0.0, 1.0),
    )
}

fn bilinear_sample(values: &[f32], cols: usize, rows: usize, grid_x: f32, grid_y: f32) -> f32 {
    bilinear_sample_by(cols, rows, grid_x, grid_y, |index| values[index])
}

fn bilinear_sample_by(
    cols: usize,
    rows: usize,
    grid_x: f32,
    grid_y: f32,
    value: impl Fn(usize) -> f32,
) -> f32 {
    let x = grid_x.clamp(0.0, (cols - 1) as f32);
    let y = grid_y.clamp(0.0, (rows - 1) as f32);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(cols - 1);
    let y1 = (y0 + 1).min(rows - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let lower = lerp(value(y0 * cols + x0), value(y0 * cols + x1), tx);
    let upper = lerp(value(y1 * cols + x0), value(y1 * cols + x1), tx);
    lerp(lower, upper, ty)
}

fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn lerp(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount
}

/// Failures from preliminary-climate generation or its validated boundaries.
#[derive(Debug, Error)]
pub enum ClimateGenerationError {
    /// The resolved climate forcing is invalid.
    #[error("invalid preliminary-climate specification: {0}")]
    InvalidSpec(#[from] ClimateSpecError),
    /// Relief is invalid or not aligned to the spatial topology.
    #[error("invalid climate relief input: {0}")]
    InvalidRelief(#[from] ReliefValidationError),
    /// Generated climate violated its formal snapshot contract.
    #[error("generated preliminary climate is invalid: {0}")]
    InvalidSnapshot(#[from] ClimateValidationError),
    /// Valid climate generation requires at least one spatial cell.
    #[error("cannot generate preliminary climate for an empty spatial snapshot")]
    EmptySpatialSnapshot,
    /// Dense stable IDs cannot represent more than `u32::MAX` cells.
    #[error("spatial cell count {found} exceeds the preliminary-climate index range")]
    CellCountOverflow {
        /// The rejected spatial cardinality.
        found: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_budget_is_bounded_for_all_supported_world_scales() {
        for count in [1, 16, 128, 20_000, u32::MAX as usize] {
            for (width, height) in [(1.0, 1.0), (20.0, 1.0), (1.0, 20.0)] {
                let (cols, rows) = climate_grid_dimensions(count, width, height);
                assert!((MIN_CLIMATE_GRID_CELLS..=MAX_CLIMATE_GRID_CELLS).contains(&(cols * rows)));
                assert!(cols >= MIN_GRID_AXIS);
                assert!(rows >= MIN_GRID_AXIS);
            }
        }
    }

    #[test]
    fn insolation_reverses_between_hemispheres() {
        let june = monthly_declination_degrees(5, 23.4);
        let december = monthly_declination_degrees(11, 23.4);
        assert!(daily_mean_insolation(55.0, june) > daily_mean_insolation(55.0, december));
        assert!(daily_mean_insolation(-55.0, june) < daily_mean_insolation(-55.0, december));
    }
}
