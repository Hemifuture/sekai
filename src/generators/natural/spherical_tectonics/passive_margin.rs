//! Present-day passive-margin relaxation for the final current crust.
//!
//! Cortial's coarse model explicitly omits shallow submerged continental
//! margins. We add the standard efficient approximation used by map models: a
//! bounded graph-distance shelf/slope profile, while preserving currently
//! active Andean and Himalayan fronts. This changes elevation only; ownership,
//! crust kind, topology, and unit-sphere geometry remain untouched.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::model::CrustSample;
use crate::world::natural::{CrustKind, SphericalOrogenyKind};
use crate::world::spatial::SphericalSurfaceSnapshot;

const PASSIVE_MARGIN_WIDTH_M: f64 = 800_000.0;
const PASSIVE_MARGIN_EDGE_ELEVATION_M: f32 = -200.0;
const PASSIVE_MARGIN_INLAND_CAP_M: f32 = 600.0;
const ACTIVE_OROGENY_MAX_AGE_MYR: f32 = 32.0;
const MILLIMETERS_PER_METER: f64 = 1_000.0;

pub(super) fn relax_passive_margins(
    surface: &SphericalSurfaceSnapshot,
    samples: &mut [CrustSample],
) {
    debug_assert_eq!(samples.len(), surface.cells().len());
    let mut distance_mm = vec![u64::MAX; samples.len()];
    let mut pending = BinaryHeap::new();

    for edge in surface.edges() {
        let indices = edge.cells.map(|cell| cell.raw() as usize);
        if samples[indices[0]].kind == samples[indices[1]].kind {
            continue;
        }
        let continental = if samples[indices[0]].kind == CrustKind::Continental {
            indices[0]
        } else {
            indices[1]
        };
        if distance_mm[continental] != 0 {
            distance_mm[continental] = 0;
            pending.push(Reverse((0_u64, continental as u32)));
        }
    }

    let maximum_mm = (PASSIVE_MARGIN_WIDTH_M * MILLIMETERS_PER_METER).round() as u64;
    while let Some(Reverse((distance, raw_cell))) = pending.pop() {
        let cell = raw_cell as usize;
        if distance_mm[cell] != distance || distance >= maximum_mm {
            continue;
        }
        for &edge_id in surface.cell_edges(surface.cells()[cell].id).unwrap() {
            let edge = surface.edge(edge_id).unwrap();
            let neighbor = surface
                .opposite_cell(surface.cells()[cell].id, edge_id)
                .unwrap()
                .raw() as usize;
            if samples[neighbor].kind != CrustKind::Continental {
                continue;
            }
            let edge_mm = (edge.length.get() * MILLIMETERS_PER_METER).round() as u64;
            let candidate = distance.saturating_add(edge_mm);
            if candidate < distance_mm[neighbor] && candidate <= maximum_mm {
                distance_mm[neighbor] = candidate;
                pending.push(Reverse((candidate, neighbor as u32)));
            }
        }
    }

    for (sample, distance_mm) in samples.iter_mut().zip(distance_mm) {
        if sample.kind != CrustKind::Continental || distance_mm > maximum_mm {
            continue;
        }
        let active_orogeny = sample.orogeny != SphericalOrogenyKind::None
            && sample.orogeny_age_myr <= ACTIVE_OROGENY_MAX_AGE_MYR;
        if active_orogeny {
            continue;
        }
        let normalized = (distance_mm as f64 / maximum_mm as f64).clamp(0.0, 1.0);
        let smooth = normalized * normalized * (3.0 - 2.0 * normalized);
        let cap_m = f64::from(PASSIVE_MARGIN_EDGE_ELEVATION_M)
            + f64::from(PASSIVE_MARGIN_INLAND_CAP_M - PASSIVE_MARGIN_EDGE_ELEVATION_M) * smooth;
        sample.tectonic_elevation_m = sample.tectonic_elevation_m.min(cap_m as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::relax_passive_margins;
    use crate::generators::natural::spherical_tectonics::model::{CrustSample, LineageId};
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, SphericalOrogenyKind, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
        NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::{Meters, SphericalSpaceSpec};

    #[test]
    fn passive_shelf_submerges_only_inactive_continental_margin() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let margin = surface.edges()[0].cells[0].raw() as usize;
        let make = |index: usize| CrustSample {
            position: surface.cells()[index].centroid,
            anchor: surface.cells()[index].id,
            owner: LineageId::from_raw(0),
            kind: if index == margin {
                CrustKind::Continental
            } else {
                CrustKind::Oceanic
            },
            thickness_km: if index == margin { 35.0 } else { 7.0 },
            age_myr: if index == margin {
                CONTINENTAL_CRUST_AGE_SENTINEL_MYR
            } else {
                50.0
            },
            tectonic_elevation_m: 1_000.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
        };
        let mut passive = (0..surface.cells().len()).map(make).collect::<Vec<_>>();
        let mut active = passive.clone();
        active[margin].orogeny = SphericalOrogenyKind::Andean;
        active[margin].orogeny_age_myr = 0.0;

        relax_passive_margins(&surface, &mut passive);
        relax_passive_margins(&surface, &mut active);

        assert_eq!(passive[margin].tectonic_elevation_m, -200.0);
        assert_eq!(active[margin].tectonic_elevation_m, 1_000.0);
    }
}
