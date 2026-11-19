//! Cubed-sphere cell-centre triangulation shared by strain and wind reconstruction.
use crate::engine::{BuildCancellation, BuildCancellationError};
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::spatial::{cross, dot};

pub(super) fn for_each_triangle(
    grid: &CubedSphereGrid,
    cancellation: &BuildCancellation,
    mut visit: impl FnMut(&[u32], [u32; 3]),
) -> Result<(), BuildCancellationError> {
    let mut incident = vec![([0_u32; 4], 0_usize); grid.vertices().len()];
    for (cell_index, cell) in grid.cells().iter().enumerate() {
        if cell_index % 256 == 0 {
            cancellation.check_cancelled()?;
        }
        for &vertex in cell.vertices() {
            let (cells, count) = &mut incident[vertex as usize];
            cells[*count] = cell_index as u32;
            *count += 1;
        }
    }
    for (vertex_index, (cells, count)) in incident.iter_mut().enumerate() {
        let cells = &mut cells[..*count];
        if vertex_index % 256 == 0 {
            cancellation.check_cancelled()?;
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
        let east_norm = dot(east, east).sqrt();
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
            visit(cells, [cells[0], cells[local], cells[local + 1]]);
        }
    }
    cancellation.check_cancelled()?;
    Ok(())
}
