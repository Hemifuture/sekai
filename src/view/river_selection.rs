//! Connected cartographic selection of an already validated drainage network.

/// Selects complete main-stem paths seeded by local LOD, in reach order.
///
/// Inputs are aligned metadata from a validated, single-receiver river network;
/// `width_m` reads its production hydraulic width. No hydrology is recomputed.
pub(crate) fn select_river_reaches(
    cells: &[(u32, u32)],
    orders: &[u8],
    cell_levels: &[u8],
    width_m: impl Fn(usize) -> f32,
) -> Vec<bool> {
    let max_order = orders.iter().copied().max().unwrap_or(0);
    let mut visible: Vec<bool> = cells
        .iter()
        .zip(orders)
        .map(|(&(from, to), &order)| {
            let level = cell_levels[from as usize].max(cell_levels[to as usize]);
            u16::from(order) + u16::from(level.saturating_sub(1)) >= u16::from(max_order)
        })
        .collect();
    let mut outgoing = vec![None; cell_levels.len()];
    let mut main_incoming: Vec<Option<usize>> = vec![None; cell_levels.len()];
    for (reach, &(from, to)) in cells.iter().enumerate() {
        outgoing[from as usize] = Some(reach);
        let preferred = &mut main_incoming[to as usize];
        let replace = preferred.is_none_or(|previous| {
            orders[reach]
                .cmp(&orders[previous])
                .then_with(|| width_m(reach).total_cmp(&width_m(previous)))
                // Cell IDs, unlike storage positions, survive reach reordering.
                .then_with(|| cells[previous].0.cmp(&from))
                .is_gt()
        });
        if replace {
            *preferred = Some(reach);
        }
    }
    let mut pending: Vec<usize> = visible
        .iter()
        .enumerate()
        .filter_map(|(reach, &selected)| selected.then_some(reach))
        .collect();
    while let Some(reach) = pending.pop() {
        let (from, to) = cells[reach];
        for adjacent in [main_incoming[from as usize], outgoing[to as usize]]
            .into_iter()
            .flatten()
        {
            if !visible[adjacent] {
                visible[adjacent] = true;
                pending.push(adjacent);
            }
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;

    // A confluence plus a longer trunk: local LOD must not invent sources or
    // sinks. This small graph covers the defect without generating a world.
    const CELLS: [(u32, u32); 5] = [(0, 2), (1, 2), (2, 3), (3, 4), (4, 5)];
    const ORDERS: [u8; 5] = [1, 1, 2, 2, 2];
    const WIDTHS: [f32; 5] = [10.0, 8.0, 20.0, 20.0, 20.0];

    #[test]
    fn coarse_trunk_retains_one_real_headwater() {
        assert_eq!(
            select_river_reaches(&CELLS, &ORDERS, &[1; 6], |i| WIDTHS[i]),
            vec![true, false, true, true, true]
        );
    }

    #[test]
    fn mixed_lod_keeps_tail_and_is_independent_of_storage_order() {
        let orders = [1; 5];
        // An unrelated higher-order river raises the global selection tier.
        let mut cells = CELLS.to_vec();
        cells.push((6, 7));
        let mut orders = orders.to_vec();
        orders.push(3);
        let levels = [3, 1, 1, 1, 1, 1, 1, 1];
        let expected = vec![true, false, true, true, true, true];
        assert_eq!(
            select_river_reaches(&cells, &orders, &levels, |_| 10.0),
            expected
        );
        cells.reverse();
        orders.reverse();
        let mut reversed = select_river_reaches(&cells, &orders, &levels, |_| 10.0);
        reversed.reverse();
        assert_eq!(reversed, expected);
    }

    #[test]
    fn deep_selection_restores_tributaries() {
        assert!(
            select_river_reaches(&CELLS, &ORDERS, &[4; 6], |i| WIDTHS[i])
                .into_iter()
                .all(|visible| visible)
        );
    }
}
