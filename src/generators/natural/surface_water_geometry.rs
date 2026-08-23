use crate::engine::BuildCancellation;
use crate::world::natural::{
    surface_elevation_fingerprint, LandOceanField, LandOceanKind, SurfaceWaterGeometry,
    WaterVolumeSolution, WaterVolumeSolveError,
};
use crate::world::spatial::{
    central_angle, spherical_triangle_area_unit, SphericalSurfaceSnapshot, SurfaceRef,
};

/// Builds the one authoritative sub-cell interpretation of a published sea level.
pub fn build_surface_water_geometry(
    surface: &SphericalSurfaceSnapshot,
    elevation_m: &[f32],
    sea_level_m: f32,
    cancellation: &BuildCancellation,
) -> Result<SurfaceWaterGeometry, WaterVolumeSolveError> {
    if !sea_level_m.is_finite() {
        return Err(WaterVolumeSolveError::InvalidSeaLevel { found: sea_level_m });
    }
    SurfaceWaterReconstruction::new(surface, elevation_m, cancellation)?
        .build(sea_level_m, cancellation)
}

/// Recomputes water volume through the same P1 geometry used for every other coast field.
pub fn water_volume_at_sea_level_m3(
    surface: &SphericalSurfaceSnapshot,
    elevation_m: &[f32],
    sea_level_m: f32,
) -> Result<f64, WaterVolumeSolveError> {
    Ok(
        build_surface_water_geometry(surface, elevation_m, sea_level_m, &BuildCancellation::new())?
            .total_water_volume_m3(),
    )
}

/// Solves the continuous P1 water-volume equation and publishes the same geometry payload.
pub fn solve_physical_sea_level(
    surface: &SphericalSurfaceSnapshot,
    elevation_m: &[f32],
    water_inventory_m3: f64,
) -> Result<WaterVolumeSolution, WaterVolumeSolveError> {
    solve_physical_sea_level_cancellable(
        surface,
        elevation_m,
        water_inventory_m3,
        &BuildCancellation::new(),
    )
}

/// Cancellation-aware form of the continuous P1 water-volume solve.
pub fn solve_physical_sea_level_cancellable(
    surface: &SphericalSurfaceSnapshot,
    elevation_m: &[f32],
    water_inventory_m3: f64,
    cancellation: &BuildCancellation,
) -> Result<WaterVolumeSolution, WaterVolumeSolveError> {
    check_cancelled(cancellation)?;
    if !water_inventory_m3.is_finite() || water_inventory_m3 < 0.0 {
        return Err(WaterVolumeSolveError::InvalidInventory {
            found: water_inventory_m3,
        });
    }
    let reconstruction = SurfaceWaterReconstruction::new(surface, elevation_m, cancellation)?;
    let minimum = elevation_m
        .iter()
        .copied()
        .min_by(f32::total_cmp)
        .ok_or(WaterVolumeSolveError::EmptySurface)?;
    if water_inventory_m3 == 0.0 {
        let geometry = reconstruction.build(minimum, cancellation)?;
        return WaterVolumeSolution::from_geometry(geometry, water_inventory_m3);
    }

    let maximum = reconstruction
        .maximum_node_elevation_m()
        .max(f64::from(minimum));
    let total_area_m2 = surface.total_cell_area().get();
    let mut lower = f64::from(minimum);
    let mut upper = maximum + water_inventory_m3 / total_area_m2;
    if !upper.is_finite() || upper > f64::from(f32::MAX) {
        return Err(WaterVolumeSolveError::NonFiniteSolution { found: upper });
    }
    let upper_volume = reconstruction.total_volume_m3(upper, cancellation)?;
    if upper_volume < water_inventory_m3 {
        return Err(WaterVolumeSolveError::NonFiniteSolution { found: upper });
    }

    loop {
        check_cancelled(cancellation)?;
        let midpoint = lower + 0.5 * (upper - lower);
        if midpoint == lower || midpoint == upper {
            break;
        }
        let volume = reconstruction.total_volume_m3(midpoint, cancellation)?;
        if volume < water_inventory_m3 {
            lower = midpoint;
        } else if volume > water_inventory_m3 {
            upper = midpoint;
        } else {
            lower = midpoint;
            upper = midpoint;
            break;
        }
    }

    let quantized = (lower + 0.5 * (upper - lower)) as f32;
    let mut best_level = quantized;
    let mut best_error = f64::INFINITY;
    for candidate in [next_down_f32(quantized), quantized, next_up_f32(quantized)] {
        if !candidate.is_finite() {
            continue;
        }
        let volume = reconstruction.total_volume_m3(f64::from(candidate), cancellation)?;
        let error = (volume - water_inventory_m3).abs();
        if error < best_error
            || (error.to_bits() == best_error.to_bits() && candidate.total_cmp(&best_level).is_lt())
        {
            best_level = candidate;
            best_error = error;
        }
    }
    let geometry = reconstruction.build(best_level, cancellation)?;
    WaterVolumeSolution::from_geometry(geometry, water_inventory_m3)
}

struct SurfaceWaterReconstruction<'surface> {
    surface: &'surface SphericalSurfaceSnapshot,
    elevation_m: &'surface [f32],
    vertex_elevation_m: Vec<f64>,
    fan_area_m2: Vec<f64>,
}

impl<'surface> SurfaceWaterReconstruction<'surface> {
    fn new(
        surface: &'surface SphericalSurfaceSnapshot,
        elevation_m: &'surface [f32],
        cancellation: &BuildCancellation,
    ) -> Result<Self, WaterVolumeSolveError> {
        check_cancelled(cancellation)?;
        surface.validate()?;
        if elevation_m.is_empty() {
            return Err(WaterVolumeSolveError::EmptySurface);
        }
        if elevation_m.len() != surface.cells().len() {
            return Err(WaterVolumeSolveError::LengthMismatch {
                elevations: elevation_m.len(),
                areas: surface.cells().len(),
            });
        }
        for (index, &found) in elevation_m.iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            if !found.is_finite() {
                return Err(WaterVolumeSolveError::InvalidElevation { index, found });
            }
        }

        let vertex_elevation_m = reconstruct_vertex_elevations(surface, elevation_m, cancellation)?;
        let fan_area_m2 = normalized_fan_areas(surface, cancellation)?;
        check_cancelled(cancellation)?;
        Ok(Self {
            surface,
            elevation_m,
            vertex_elevation_m,
            fan_area_m2,
        })
    }

    fn maximum_node_elevation_m(&self) -> f64 {
        self.elevation_m
            .iter()
            .map(|value| f64::from(*value))
            .chain(self.vertex_elevation_m.iter().copied())
            .fold(f64::NEG_INFINITY, f64::max)
    }

    fn total_volume_m3(
        &self,
        sea_level_m: f64,
        cancellation: &BuildCancellation,
    ) -> Result<f64, WaterVolumeSolveError> {
        let mut total = CompensatedSum::default();
        let mut fan = 0_usize;
        for (index, cell) in self.surface.cells().iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            let center_depth_m = sea_level_m - f64::from(self.elevation_m[index]);
            for side in 0..cell.boundary_vertices.len() {
                let first = cell.boundary_vertices[side].raw() as usize;
                let second = cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()].raw()
                    as usize;
                let depths = [
                    center_depth_m,
                    sea_level_m - self.vertex_elevation_m[first],
                    sea_level_m - self.vertex_elevation_m[second],
                ];
                let (_, integrated_depth_m) = integrate_positive_linear_triangle(depths);
                total.add(self.fan_area_m2[fan] * integrated_depth_m);
                fan += 1;
            }
        }
        check_cancelled(cancellation)?;
        Ok(total.total())
    }

    fn build(
        &self,
        sea_level_m: f32,
        cancellation: &BuildCancellation,
    ) -> Result<SurfaceWaterGeometry, WaterVolumeSolveError> {
        check_cancelled(cancellation)?;
        let sea_level = f64::from(sea_level_m);
        let mut ocean_area_fraction = Vec::with_capacity(self.surface.cells().len());
        let mut cell_water_volume_m3 = Vec::with_capacity(self.surface.cells().len());
        let mut land_ocean = Vec::with_capacity(self.surface.cells().len());
        let mut fan = 0_usize;
        for (index, cell) in self.surface.cells().iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            let center_depth_m = sea_level - f64::from(self.elevation_m[index]);
            let mut wet_area = CompensatedSum::default();
            let mut volume = CompensatedSum::default();
            let mut all_non_negative = center_depth_m >= 0.0;
            let mut any_positive = center_depth_m > 0.0;
            for side in 0..cell.boundary_vertices.len() {
                let first = cell.boundary_vertices[side].raw() as usize;
                let second = cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()].raw()
                    as usize;
                let first_depth = sea_level - self.vertex_elevation_m[first];
                let second_depth = sea_level - self.vertex_elevation_m[second];
                all_non_negative &= first_depth >= 0.0 && second_depth >= 0.0;
                any_positive |= first_depth > 0.0 || second_depth > 0.0;
                let (wet_fraction, integrated_depth_m) =
                    integrate_positive_linear_triangle([center_depth_m, first_depth, second_depth]);
                let area = self.fan_area_m2[fan];
                wet_area.add(area * wet_fraction);
                volume.add(area * integrated_depth_m);
                fan += 1;
            }
            let fraction = if !any_positive {
                0.0
            } else if all_non_negative {
                1.0
            } else {
                (wet_area.total() / cell.area.get()).clamp(0.0, 1.0)
            } as f32;
            ocean_area_fraction.push(fraction);
            cell_water_volume_m3.push(volume.total().max(0.0));
            land_ocean.push(LandOceanKind::classify(
                self.elevation_m[index],
                sea_level_m,
            ));
        }

        let mut wet_edge_fraction = Vec::with_capacity(self.surface.edges().len());
        for (index, edge) in self.surface.edges().iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            let [first, second] = edge
                .vertices
                .map(|vertex| sea_level - self.vertex_elevation_m[vertex.raw() as usize]);
            wet_edge_fraction.push(wet_line_fraction(first, second) as f32);
        }
        check_cancelled(cancellation)?;
        let geometry = SurfaceWaterGeometry::new_cancellable(
            SurfaceRef::for_spherical(self.surface),
            surface_elevation_fingerprint(self.elevation_m),
            sea_level_m,
            ocean_area_fraction,
            wet_edge_fraction,
            cell_water_volume_m3,
            LandOceanField::from_kinds(land_ocean),
            &|| cancellation.is_cancelled(),
        )?;
        geometry.validate_against(self.surface, self.elevation_m)?;
        Ok(geometry)
    }
}

fn reconstruct_vertex_elevations(
    surface: &SphericalSurfaceSnapshot,
    elevation_m: &[f32],
    cancellation: &BuildCancellation,
) -> Result<Vec<f64>, WaterVolumeSolveError> {
    let mut weighted_delta = vec![CompensatedSum::default(); surface.vertices().len()];
    let mut weights = vec![CompensatedSum::default(); surface.vertices().len()];
    let mut anchor_elevation_m = vec![None; surface.vertices().len()];
    let mut exact = vec![None; surface.vertices().len()];
    for (index, cell) in surface.cells().iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        for vertex_id in &cell.boundary_vertices {
            let vertex = surface
                .vertex(*vertex_id)
                .expect("validated surface contains boundary vertex");
            let distance = central_angle(cell.centroid, vertex.position);
            let vertex_index = vertex_id.raw() as usize;
            if distance == 0.0 {
                exact[vertex_index] = Some(f64::from(elevation_m[index]));
            } else {
                // Every sample shares one sphere radius, so inverse squared
                // central angle produces the exact Shepard distance ratios.
                let weight = 1.0 / (distance * distance);
                let elevation = f64::from(elevation_m[index]);
                let anchor = *anchor_elevation_m[vertex_index].get_or_insert(elevation);
                weighted_delta[vertex_index].add(weight * (elevation - anchor));
                weights[vertex_index].add(weight);
            }
        }
    }
    let mut result = Vec::with_capacity(surface.vertices().len());
    for index in 0..surface.vertices().len() {
        poll_cancelled(index, cancellation)?;
        let value = if let Some(value) = exact[index] {
            value
        } else {
            anchor_elevation_m[index].unwrap_or(f64::NAN)
                + weighted_delta[index].total() / weights[index].total()
        };
        if !value.is_finite() {
            return Err(WaterVolumeSolveError::NonFiniteSolution { found: value });
        }
        result.push(value);
    }
    Ok(result)
}

fn normalized_fan_areas(
    surface: &SphericalSurfaceSnapshot,
    cancellation: &BuildCancellation,
) -> Result<Vec<f64>, WaterVolumeSolveError> {
    let mut result = Vec::with_capacity(
        surface
            .cells()
            .iter()
            .map(|cell| cell.boundary_vertices.len())
            .sum(),
    );
    for (index, cell) in surface.cells().iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        let mut unit_areas = Vec::with_capacity(cell.boundary_vertices.len());
        let mut total = CompensatedSum::default();
        for side in 0..cell.boundary_vertices.len() {
            let first = surface
                .vertex(cell.boundary_vertices[side])
                .expect("validated boundary vertex")
                .position;
            let second = surface
                .vertex(cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()])
                .expect("validated boundary vertex")
                .position;
            let area = spherical_triangle_area_unit(cell.centroid, first, second);
            if !area.is_finite() || area <= 0.0 {
                return Err(WaterVolumeSolveError::InvalidFanTriangle {
                    cell: cell.id,
                    side,
                });
            }
            unit_areas.push(area);
            total.add(area);
        }
        let scale = cell.area.get() / total.total();
        let start = result.len();
        for area in unit_areas.iter().take(unit_areas.len() - 1) {
            result.push(area * scale);
        }
        let assigned = result[start..].iter().copied().sum::<f64>();
        let remaining = cell.area.get() - assigned;
        if !remaining.is_finite() || remaining <= 0.0 {
            return Err(WaterVolumeSolveError::InvalidFanTriangle {
                cell: cell.id,
                side: unit_areas.len() - 1,
            });
        }
        result.push(remaining);
    }
    check_cancelled(cancellation)?;
    Ok(result)
}

/// Returns wet area fraction and the wet-depth integral divided by full area.
fn integrate_positive_linear_triangle(depth_m: [f64; 3]) -> (f64, f64) {
    let positive = depth_m.iter().filter(|depth| **depth > 0.0).count();
    if positive == 0 {
        return (0.0, 0.0);
    }
    if depth_m.iter().all(|depth| *depth >= 0.0) {
        return (1.0, depth_m.iter().sum::<f64>() / 3.0);
    }
    if positive == 1 {
        let positive_index = depth_m
            .iter()
            .position(|depth| *depth > 0.0)
            .expect("one positive vertex");
        let positive_depth = depth_m[positive_index];
        let mut scales = [0.0_f64; 2];
        let mut offset = 0_usize;
        for (index, depth) in depth_m.iter().copied().enumerate() {
            if index != positive_index {
                scales[offset] = positive_depth / (positive_depth - depth);
                offset += 1;
            }
        }
        let area_fraction = (scales[0] * scales[1]).clamp(0.0, 1.0);
        return (area_fraction, area_fraction * positive_depth / 3.0);
    }

    let negative_index = depth_m
        .iter()
        .position(|depth| *depth < 0.0)
        .expect("mixed two-positive triangle has one negative vertex");
    let negative_depth = depth_m[negative_index];
    let mut scales = [0.0_f64; 2];
    let mut offset = 0_usize;
    for (index, depth) in depth_m.iter().copied().enumerate() {
        if index != negative_index {
            scales[offset] = -negative_depth / (depth - negative_depth);
            offset += 1;
        }
    }
    let dry_fraction = (scales[0] * scales[1]).clamp(0.0, 1.0);
    let wet_fraction = 1.0 - dry_fraction;
    let full_integral = depth_m.iter().sum::<f64>() / 3.0;
    let dry_integral = dry_fraction * negative_depth / 3.0;
    (wet_fraction, (full_integral - dry_integral).max(0.0))
}

fn wet_line_fraction(first_depth_m: f64, second_depth_m: f64) -> f64 {
    match (first_depth_m > 0.0, second_depth_m > 0.0) {
        (false, false) => 0.0,
        (true, true) => 1.0,
        (true, false) => (first_depth_m / (first_depth_m - second_depth_m)).clamp(0.0, 1.0),
        (false, true) => (second_depth_m / (second_depth_m - first_depth_m)).clamp(0.0, 1.0),
    }
}

fn next_up_f32(value: f32) -> f32 {
    if value.is_nan() || value == f32::INFINITY {
        return value;
    }
    if value == -0.0 {
        return f32::from_bits(1);
    }
    let bits = value.to_bits();
    f32::from_bits(if value >= 0.0 { bits + 1 } else { bits - 1 })
}

fn next_down_f32(value: f32) -> f32 {
    if value.is_nan() || value == f32::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return f32::from_bits(0x8000_0001);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

fn poll_cancelled(
    index: usize,
    cancellation: &BuildCancellation,
) -> Result<(), WaterVolumeSolveError> {
    if index & 255 == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), WaterVolumeSolveError> {
    if cancellation.is_cancelled() {
        Err(WaterVolumeSolveError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let next = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.correction += (self.sum - next) + value;
        } else {
            self.correction += (value - next) + self.sum;
        }
        self.sum = next;
    }

    fn total(&self) -> f64 {
        self.sum + self.correction
    }
}

#[cfg(test)]
mod tests {
    use super::{integrate_positive_linear_triangle, wet_line_fraction};

    #[test]
    fn one_linear_triangle_with_a_zero_vertex_is_exactly_half_wet() {
        let (wet_fraction, integrated_depth) = integrate_positive_linear_triangle([1.0, 0.0, -1.0]);
        assert_eq!(wet_fraction, 0.5);
        assert_eq!(integrated_depth, 1.0 / 6.0);
    }

    #[test]
    fn clipped_triangle_integral_is_continuous_across_zero_depth() {
        let dry = integrate_positive_linear_triangle([1.0, -1.0, -f64::EPSILON]);
        let wet = integrate_positive_linear_triangle([1.0, -1.0, f64::EPSILON]);
        assert!((dry.0 - wet.0).abs() <= 2.0 * f64::EPSILON);
        assert!((dry.1 - wet.1).abs() <= 2.0 * f64::EPSILON);
        assert_eq!(wet_line_fraction(1.0, -1.0), 0.5);
        assert_eq!(wet_line_fraction(-1.0, 1.0), 0.5);
    }
}
