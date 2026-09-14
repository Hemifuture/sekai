// 应力依据：MOM6 MOM_hor_visc，Smagorinsky (1993)、Griffies & Hallberg (2000)。
// https://ncar.github.io/MOM6/APIs/namespacemom__hor__visc.html
// 曲面切向对称梯度／弱应变依据：Olshanskii et al. (2018), SIAM JSC 40(4)。
// https://www.igpm.rwth-aachen.de/Download/reports/pdf/IGPM475.pdf
// 本候选是拟合分片平面的 P1 弱应变，并非论文完整 TraceFEM／压力稳定化方案。
//
// 元素能量与 CFL：令 m_i=A_i H_i，c=2 nu A_T Hbar，
// B_i v = dev sym[P_T ((P_i v) tensor grad(phi_i)) P_T]。
// K_ij = c B_i^* B_j，M du/dt=-K u，故 u^T K u=sum c |Edev|_F^2 >= 0。
// 对二维切平面，|dev sym(v tensor g)|_F^2=|v|^2 |g|^2/2；
// P_i、P_T 都是正交投影，所以 ||B_i||_2 <= |grad(phi_i)|/sqrt(2)。
// 因此 ||K_ij||_2 <= nu A_T Hbar |g_i| |g_j|。
// 按 block 行范数的 Gershgorin 界，lambda_max(M^-1 K)
// <= max_i sum_{T contains i} nu A_T Hbar |g_i| sum_j |g_j| / m_i。
// 返回此上界的一半，沿用现有特征值区间 [-2*rate,0] 的 RK3 步长消费者。
// 这里的 1/2 来自接口的谱界约定；没有经验稳定系数。

use super::{
    check_cancelled, dot, is_atmosphere_role, norm, tangentize, BuildCancellation,
    ClimateLayerRole, ClimateModelProfile, CubedSphereGrid, LayeredClimateState,
    LayeredTendencyError, LayeredTendencySystem, LayeredTendencyWorkspace,
    ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S,
};
use crate::world::natural::{atmosphere_strain_triangle_count, AtmosphereStrainTriangle};
use crate::world::spatial::cross;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct AtmosphereStrainWorkspace {
    grid_fingerprint: [u8; 32],
    pub(super) triangles: Vec<AtmosphereStrainTriangle>,
    pub(super) acceleration: Vec<[f64; 3]>,
}

impl LayeredTendencySystem<'_> {
    pub(super) fn atmosphere_strain_diffusion(
        &self,
        state: &LayeredClimateState,
        role: ClimateLayerRole,
        workspace: &mut LayeredTendencyWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<f64, LayeredTendencyError> {
        let maximum_rate = self.atmosphere_strain_sources(state, role, workspace, cancellation)?;
        let cache = workspace
            .atmosphere_strain
            .as_ref()
            .expect("initialized strain cache");
        for (cell, target) in workspace.vector_scratch.iter_mut().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            *target = cache.acceleration[cell].map(|value| value as f32);
        }
        check_cancelled(cancellation)?;
        Ok(maximum_rate)
    }

    fn atmosphere_strain_sources(
        &self,
        state: &LayeredClimateState,
        role: ClimateLayerRole,
        workspace: &mut LayeredTendencyWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<f64, LayeredTendencyError> {
        assert_eq!(state.profile(), ClimateModelProfile::C2LayeredV1);
        assert!(
            is_atmosphere_role(role),
            "strain viscosity is atmosphere-only"
        );
        if workspace.cell_count != self.grid.cell_count()
            || workspace.edge_count != self.grid.edges().len()
        {
            return Err(LayeredTendencyError::WorkspaceGridMismatch);
        }
        if workspace
            .atmosphere_strain
            .as_ref()
            .is_some_and(|cache| &cache.grid_fingerprint != self.grid.fingerprint())
        {
            workspace.atmosphere_strain = None;
        }
        if workspace.atmosphere_strain.is_none() {
            let triangles = atmosphere_strain_triangles(self.grid, cancellation)?;
            workspace.atmosphere_strain = Some(AtmosphereStrainWorkspace {
                grid_fingerprint: *self.grid.fingerprint(),
                triangles,
                acceleration: vec![[0.0; 3]; self.grid.cell_count()],
            });
        }
        let cache = workspace
            .atmosphere_strain
            .as_mut()
            .expect("initialized strain cache");
        let acceleration = &mut cache.acceleration;
        acceleration.fill([0.0; 3]);
        // Full assembly clears or computes H after viscosity; fast assembly
        // has already copied its provisional H into the tendency. The shared
        // reconstructed H/u transport then overwrites this buffer with actual
        // depths. The saved upper H buffer is never borrowed here.
        let spectral_row_bounds = &mut workspace.thickness_tendency_m_s;
        spectral_row_bounds.fill(0.0);
        let velocities = state.velocity_m_s(role).expect("active atmosphere layer");
        for (triangle_index, triangle) in cache.triangles.iter().enumerate() {
            if triangle_index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let nodes = triangle.nodes.map(|cell| cell as usize);
            let depths = [
                self.fluid_layer_thickness_m(state, role, nodes[0])?,
                self.fluid_layer_thickness_m(state, role, nodes[1])?,
                self.fluid_layer_thickness_m(state, role, nodes[2])?,
            ];
            let mean_depth = depths.iter().sum::<f64>() / 3.0;
            let conductance =
                ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S * triangle.area_m2 * mean_depth;
            let nodal_velocities = nodes.map(|cell| {
                tangentize(
                    velocities[cell].map(f64::from),
                    self.grid.cells()[cell].center_unit(),
                )
            });
            let strain = atmosphere_deviatoric_strain(triangle, nodal_velocities);
            let gradient_norms = triangle.gradients_m_inv.map(norm);
            let gradient_norm_sum = gradient_norms.iter().sum::<f64>();
            for (local, &cell) in nodes.iter().enumerate() {
                let force = std::array::from_fn(|row| {
                    -2.0 * conductance * dot(strain[row], triangle.gradients_m_inv[local])
                });
                let force = tangentize(force, self.grid.cells()[cell].center_unit());
                let volume = self.grid.cells()[cell].area_m2() * depths[local];
                for component in 0..3 {
                    acceleration[cell][component] += force[component] / volume;
                }
                spectral_row_bounds[cell] +=
                    conductance * gradient_norms[local] * gradient_norm_sum / volume;
            }
        }
        check_cancelled(cancellation)?;
        Ok(0.5 * spectral_row_bounds.iter().copied().fold(0.0_f64, f64::max))
    }
}

fn atmosphere_deviatoric_strain(
    triangle: &AtmosphereStrainTriangle,
    nodal_velocities: [[f64; 3]; 3],
) -> [[f64; 3]; 3] {
    let mut gradient = [[0.0_f64; 3]; 3];
    for (velocity, basis_gradient) in nodal_velocities.into_iter().zip(triangle.gradients_m_inv) {
        let velocity = tangentize(velocity, triangle.normal);
        for row in 0..3 {
            for column in 0..3 {
                gradient[row][column] += velocity[row] * basis_gradient[column];
            }
        }
    }
    let trace = gradient[0][0] + gradient[1][1] + gradient[2][2];
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            let identity = if row == column { 1.0 } else { 0.0 };
            0.5 * (gradient[row][column] + gradient[column][row])
                - 0.5 * trace * (identity - triangle.normal[row] * triangle.normal[column])
        })
    })
}

fn atmosphere_strain_triangles(
    grid: &CubedSphereGrid,
    cancellation: &BuildCancellation,
) -> Result<Vec<AtmosphereStrainTriangle>, LayeredTendencyError> {
    let mut incident = vec![([0_u32; 4], 0_usize); grid.vertices().len()];
    for (cell_index, cell) in grid.cells().iter().enumerate() {
        if cell_index % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        for &vertex in cell.vertices() {
            let (cells, count) = &mut incident[vertex as usize];
            cells[*count] = cell_index as u32;
            *count += 1;
        }
    }
    let mut triangles = Vec::with_capacity(atmosphere_strain_triangle_count(grid.cell_count()));
    for (vertex_index, (cells, count)) in incident.iter_mut().enumerate() {
        let cells = &mut cells[..*count];
        if vertex_index % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        assert!(
            matches!(cells.len(), 3 | 4),
            "validated cubed-sphere vertices have three or four incident cells"
        );
        let normal = grid.vertices()[vertex_index];
        // Choose the least aligned Cartesian axis to obtain a deterministic,
        // nonsingular local basis. Sorting is geometric, never cell-id order.
        let axis_index = (0..3)
            .min_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
            .expect("three Cartesian axes");
        let axis = std::array::from_fn(|component| if component == axis_index { 1.0 } else { 0.0 });
        let east = cross(normal, axis);
        let east_norm = norm(east);
        let east = east.map(|value| value / east_norm);
        let north = cross(normal, east);
        cells.sort_by(|&a, &b| {
            let angle = |cell: u32| {
                let center = grid.cells()[cell as usize].center_unit();
                dot(center, north).atan2(dot(center, east))
            };
            angle(a).total_cmp(&angle(b)).then(a.cmp(&b))
        });
        // Rotate the cyclic order to its smallest id. A quad then uses the
        // diagonal from this node to its cyclic opposite, independent of sort seam.
        let first = cells
            .iter()
            .enumerate()
            .min_by_key(|(_, cell)| **cell)
            .map(|(index, _)| index)
            .expect("nonempty dual polygon");
        cells.rotate_left(first);
        for local in 1..cells.len() - 1 {
            triangles.push(atmosphere_strain_triangle(
                grid,
                [cells[0], cells[local], cells[local + 1]],
            ));
        }
    }
    check_cancelled(cancellation)?;
    debug_assert_eq!(
        triangles.len(),
        atmosphere_strain_triangle_count(grid.cell_count())
    );
    Ok(triangles)
}

fn atmosphere_strain_triangle(grid: &CubedSphereGrid, nodes: [u32; 3]) -> AtmosphereStrainTriangle {
    let positions = nodes.map(|cell| {
        grid.cells()[cell as usize]
            .center_unit()
            .map(|coordinate| coordinate * grid.radius_m())
    });
    let edge = |first: usize, second: usize| {
        std::array::from_fn(|component| positions[second][component] - positions[first][component])
    };
    let area_vector = cross(edge(0, 1), edge(0, 2));
    let twice_area = norm(area_vector);
    assert!(
        twice_area.is_finite() && twice_area > 0.0,
        "dual cell-center triangles must be nondegenerate"
    );
    let normal = area_vector.map(|value| value / twice_area);
    let gradients_m_inv = [(1, 2), (2, 0), (0, 1)]
        .map(|(first, second)| cross(normal, edge(first, second)).map(|value| value / twice_area));
    AtmosphereStrainTriangle {
        nodes,
        normal,
        area_m2: 0.5 * twice_area,
        gradients_m_inv,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{next_f32_up, ClimateLayerLayout, PlanetForcing, CLIMATE_MONTH_COUNT};
    use super::*;

    fn fixture() -> (CubedSphereGrid, LayeredClimateState) {
        fixture_at_radius(6_371_000.0)
    }

    fn fixture_at_radius(radius: f64) -> (CubedSphereGrid, LayeredClimateState) {
        let grid = CubedSphereGrid::new(1, radius).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let role = ClimateLayerRole::LowerAtmosphere;
        state.height_anomaly_m_mut(role).unwrap().fill(0.0);
        state.velocity_m_s_mut(role).unwrap().fill([0.0; 3]);
        (grid, state)
    }

    fn sources(
        system: &LayeredTendencySystem<'_>,
        state: &LayeredClimateState,
        role: ClimateLayerRole,
    ) -> (Vec<[f64; 3]>, f64) {
        let mut workspace = LayeredTendencyWorkspace::for_grid(system.grid);
        let rate = system
            .atmosphere_strain_sources(state, role, &mut workspace, &BuildCancellation::new())
            .unwrap();
        (
            workspace
                .atmosphere_strain
                .as_ref()
                .unwrap()
                .acceleration
                .clone(),
            rate,
        )
    }

    #[test]
    fn strain_cache_rebuilds_for_same_resolution_different_radius() {
        let (first, first_state) = fixture();
        let (second, second_state) = fixture_at_radius(2.0 * first.radius_m());
        let mut workspace = LayeredTendencyWorkspace::for_grid(&first);
        assert!(workspace.atmosphere_strain.is_none());
        let role = ClimateLayerRole::LowerAtmosphere;
        let cancellation = BuildCancellation::new();
        let first_system = LayeredTendencySystem::new(&first);
        first_system
            .atmosphere_strain_diffusion(&first_state, role, &mut workspace, &cancellation)
            .unwrap();
        let pointer = workspace
            .atmosphere_strain
            .as_ref()
            .unwrap()
            .triangles
            .as_ptr();
        first_system
            .atmosphere_strain_diffusion(&first_state, role, &mut workspace, &cancellation)
            .unwrap();
        assert_eq!(
            pointer,
            workspace
                .atmosphere_strain
                .as_ref()
                .unwrap()
                .triangles
                .as_ptr()
        );
        let second_system = LayeredTendencySystem::new(&second);
        let rebuilt_rate = second_system
            .atmosphere_strain_diffusion(&second_state, role, &mut workspace, &cancellation)
            .unwrap();
        let mut fresh = LayeredTendencyWorkspace::for_grid(&second);
        let fresh_rate = second_system
            .atmosphere_strain_diffusion(&second_state, role, &mut fresh, &cancellation)
            .unwrap();
        assert_eq!(workspace.atmosphere_strain, fresh.atmosphere_strain);
        assert_eq!(workspace.vector_scratch, fresh.vector_scratch);
        assert_eq!(rebuilt_rate, fresh_rate);
    }

    fn roundoff_gamma(grid: &CubedSphereGrid) -> f64 {
        // Conservative scalar-operation counts of the evaluated 3D kernel:
        // geometry; input projections; tensor gradient; trace; deviator;
        // H/coefficient; gradient norms; forces; output projections;
        // volumes; accumulation; spectral rows. Count all elements in a
        // row's bound, so summation is covered without a fitted tolerance.
        let operations_per_triangle = 79 + 66 + 54 + 2 + 63 + 6 + 20 + 63 + 33 + 6 + 18 + 15;
        let triangles = atmosphere_strain_triangles(grid, &BuildCancellation::new())
            .unwrap()
            .len();
        let operations = triangles * operations_per_triangle + 6 * grid.cell_count();
        let product = operations as f64 * f64::EPSILON;
        product / (1.0 - product)
    }

    #[test]
    fn strain_preserves_all_three_rigid_rotations() {
        let (grid, mut state) = fixture();
        let system = LayeredTendencySystem::new(&grid);
        let role = ClimateLayerRole::LowerAtmosphere;
        let cancellation = BuildCancellation::new();
        for axis in 0..3 {
            let omega = std::array::from_fn(|component| if component == axis { 1.0 } else { 0.0 });
            for (cell, velocity) in state.velocity_m_s_mut(role).unwrap().iter_mut().enumerate() {
                // Grid-1 centers are Cartesian axes, so these unit-speed
                // rotations have exactly representable f32 nodal values.
                *velocity =
                    cross(omega, grid.cells()[cell].center_unit()).map(|value| value as f32);
            }
            let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
            let rate = system
                .atmosphere_strain_diffusion(&state, role, &mut workspace, &cancellation)
                .unwrap();
            let speed = state
                .velocity_m_s(role)
                .unwrap()
                .iter()
                .map(|value| norm(value.map(f64::from)))
                .fold(0.0_f64, f64::max);
            let bound = roundoff_gamma(&grid) * (2.0 * rate) * speed;
            let residual = workspace
                .vector_scratch
                .iter()
                .map(|value| norm(value.map(f64::from)))
                .fold(0.0_f64, f64::max);
            assert!(residual <= bound, "axis {axis}: {residual} > {bound}");
        }
    }

    #[test]
    fn strain_dissipates_with_unequal_depths_and_zero_torque() {
        let (grid, mut state) = fixture();
        let role = ClimateLayerRole::LowerAtmosphere;
        let reference = state.reference_thickness_m(role).unwrap();
        for cell in 0..grid.cell_count() {
            state.height_anomaly_m_mut(role).unwrap()[cell] =
                reference / (cell + 1) as f32 - reference;
            let value = (cell + 1) as f64;
            state.velocity_m_s_mut(role).unwrap()[cell] = tangentize(
                [value, value * value, 1.0 - value],
                grid.cells()[cell].center_unit(),
            )
            .map(|component| component as f32);
        }
        let system = LayeredTendencySystem::new(&grid);
        let (acceleration, _) = sources(&system, &state, role);
        let mut work = 0.0;
        let mut torque = [0.0; 3];
        let mut torque_scale = 0.0;
        for (cell, acceleration) in acceleration.iter().enumerate() {
            let volume = grid.cells()[cell].area_m2()
                * system.fluid_layer_thickness_m(&state, role, cell).unwrap();
            let force = acceleration.map(|value| value * volume);
            work += dot(
                force,
                state.velocity_m_s(role).unwrap()[cell].map(f64::from),
            );
            let position = grid.cells()[cell]
                .center_unit()
                .map(|value| value * grid.radius_m());
            let local_torque = cross(position, force);
            for component in 0..3 {
                torque[component] += local_torque[component];
            }
            torque_scale += norm(position) * norm(force);
        }
        assert!(work < 0.0);
        assert!(norm(torque) <= roundoff_gamma(&grid) * torque_scale);
    }

    #[test]
    fn strain_bounds_the_thin_column_operator_spectrum() {
        let (grid, mut state) = fixture();
        let role = ClimateLayerRole::LowerAtmosphere;
        // Smallest positive H represented by the existing height storage.
        state.height_anomaly_m_mut(role).unwrap()[0] =
            next_f32_up(-state.reference_thickness_m(role).unwrap());
        let system = LayeredTendencySystem::new(&grid);
        let count = 3 * grid.cell_count();
        let masses: Vec<f64> = grid
            .cells()
            .iter()
            .enumerate()
            .map(|(cell, geometry)| {
                geometry.area_m2() * system.fluid_layer_thickness_m(&state, role, cell).unwrap()
            })
            .collect();
        let mut stiffness = vec![vec![0.0_f64; count]; count];
        let mut spectral_bound = 0.0;
        for column in 0..count {
            state.velocity_m_s_mut(role).unwrap().fill([0.0; 3]);
            state.velocity_m_s_mut(role).unwrap()[column / 3][column % 3] = 1.0;
            let (response, rate) = sources(&system, &state, role);
            spectral_bound = 2.0 * rate;
            for row in 0..count {
                stiffness[row][column] =
                    -response[row / 3][row % 3] * (masses[row / 3] / masses[column / 3]).sqrt();
            }
        }
        let roundoff = roundoff_gamma(&grid) * spectral_bound;
        for (row, entries) in stiffness.iter().enumerate() {
            for (column, &entry) in entries.iter().enumerate().take(row) {
                assert!((entry - stiffness[column][row]).abs() <= roundoff);
            }
        }
        // Cholesky of Lambda*I - sym(M^-1/2 K M^-1/2) is a full-spectrum
        // check, rather than one favorable Rayleigh vector. No eigenvalue
        // tolerance or iteration count is introduced.
        let mut factor = vec![vec![0.0_f64; count]; count];
        for row in 0..count {
            for column in 0..=row {
                let mut value = -0.5 * (stiffness[row][column] + stiffness[column][row]);
                if row == column {
                    value += spectral_bound;
                }
                for (&row_entry, &column_entry) in
                    factor[row][..column].iter().zip(&factor[column][..column])
                {
                    value -= row_entry * column_entry;
                }
                if row == column {
                    assert!(
                        value > 0.0,
                        "spectral upper bound fails at pivot {row}: {value}"
                    );
                    factor[row][column] = value.sqrt();
                } else {
                    factor[row][column] = value / factor[column][column];
                }
            }
        }
    }

    #[test]
    fn strain_detects_element_pure_shear() {
        let (grid, _) = fixture();
        let triangles = atmosphere_strain_triangles(&grid, &BuildCancellation::new()).unwrap();
        let triangle = &triangles[0];
        let first = triangle.gradients_m_inv[0];
        let first = first.map(|value| value / norm(first));
        let second = cross(triangle.normal, first);
        let origin = grid.cells()[triangle.nodes[0] as usize].center_unit();
        let velocities = triangle.nodes.map(|cell| {
            let position = std::array::from_fn(|component| {
                grid.radius_m()
                    * (grid.cells()[cell as usize].center_unit()[component] - origin[component])
            });
            std::array::from_fn(|component| {
                first[component] * dot(position, second) + second[component] * dot(position, first)
            })
        });
        let strain = atmosphere_deviatoric_strain(triangle, velocities);
        let squared_strain: f64 = strain
            .into_iter()
            .flatten()
            .map(|value| value * value)
            .sum();
        assert!(
            squared_strain > 0.0,
            "P1 strain must retain off-diagonal shear"
        );
    }
}
