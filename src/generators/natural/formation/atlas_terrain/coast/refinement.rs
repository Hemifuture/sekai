//! Refine source faces where the sampled shoreline has separate submerged passages.

use std::collections::hash_map::Entry;

use civ_atlas_terrain::FineTerrain;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use super::super::{check_cancelled, AtlasGenerationError};
use super::ports::{add_path, submerged_path};
use super::support::{edge_key, Mesh};
use super::vertex_beds;
use crate::engine::BuildCancellation;
use crate::world::natural::atlas_terrain::ATLAS_COAST_PARENT_RASTER_INTERVALS;
use crate::world::spatial::{add, central_angle, UnitVector3};

fn unit([x, y, z]: [f64; 3]) -> UnitVector3 {
    UnitVector3::new(x, y, z).expect("validated source face lies in an open hemisphere")
}

fn diameter(p: [[f64; 3]; 3]) -> f64 {
    let [a, b, c] = p.map(unit);
    central_angle(a, b)
        .max(central_angle(b, c))
        .max(central_angle(c, a))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Perimeter {
    Empty,
    SingleWetRun,
    SeparateWetRuns,
}

/// Classes alternate corner/midpoint around the boundary of a single subtriangle.
fn perimeter(wet: [bool; 6]) -> Perimeter {
    if !wet.contains(&true) {
        return Perimeter::Empty;
    }
    let runs = (0..6).filter(|&i| wet[i] && !wet[(i + 5) % 6]).count();
    if runs > 1 {
        Perimeter::SeparateWetRuns
    } else {
        Perimeter::SingleWetRun
    }
}

struct WetStencil {
    corners: [[f64; 3]; 3],
    wet_rim: [bool; 6],
    /// The ordered endpoint pair names the half from its first endpoint to the midpoint.
    vertices: [u32; 3],
}

impl WetStencil {
    fn rim(&self) -> [[f64; 3]; 6] {
        let [a, b, c] = self.corners;
        [a, add(a, b), b, add(b, c), c, add(c, a)]
    }

    fn centre(&self) -> [f64; 3] {
        let [a, b, c] = self.corners;
        add(add(a, b), c)
    }

    fn rim_edge(&self, slot: usize) -> [u32; 2] {
        let [a, b, c] = self.vertices;
        [[a, b], [b, a], [b, c], [c, b], [c, a], [a, c]][slot]
    }

    /// Connect observed wet stations only along wholly submerged great-circle segments.
    /// Reuse the short segments already tested and try only links between separate groups.
    fn connected<Beds>(
        &self,
        centre_wet: bool,
        spacing: f64,
        known: impl IntoIterator<Item = (usize, usize, bool)>,
        sample: impl FnOnce(&[[f64; 3]]) -> Result<Beds, AtlasGenerationError>,
    ) -> Result<bool, AtlasGenerationError>
    where
        Beds: IntoIterator<Item = f64>,
    {
        let [a, ab, b, bc, c, ca] = self.rim();
        let positions = [a, ab, b, bc, c, ca, self.centre()];
        let mut wet = [false; 7];
        wet[..6].copy_from_slice(&self.wet_rim);
        wet[6] = centre_wet;
        let mut groups = [0, 1, 2, 3, 4, 5, 6];
        let mut tested = [[false; 7]; 7];
        for (a, b, submerged) in known {
            tested[a.min(b)][a.max(b)] = true;
            if submerged {
                join(&mut groups, a, b);
            }
        }
        let mut directions = Vec::new();
        let mut links = Vec::new();
        for start in 0..7 {
            if !wet[start] {
                continue;
            }
            for end in start + 1..7 {
                if !wet[end] || tested[start][end] || groups[start] == groups[end] {
                    continue;
                }
                // A full source side passes through its observed midpoint; a median
                // passes through the observed centre. Let their two shorter links decide.
                if end < 6 && ((start % 2 == 0 && end % 2 == 0) || end - start == 3) {
                    continue;
                }
                let count = add_path([positions[start], positions[end]], spacing, &mut directions);
                links.push((start, end, count));
            }
        }
        if !links.is_empty() {
            let mut beds = sample(&directions)?.into_iter();
            for (start, end, count) in links {
                if submerged_path(&mut beds, count) {
                    join(&mut groups, start, end);
                }
            }
        }
        let mut wet_groups = groups
            .into_iter()
            .zip(wet)
            .filter_map(|(group, wet)| wet.then_some(group));
        let first = wet_groups.next();
        Ok(wet_groups.all(|group| Some(group) == first))
    }
}

fn join(groups: &mut [usize; 7], start: usize, end: usize) {
    let (from, to) = (groups[end], groups[start]);
    for label in groups {
        if *label == from {
            *label = to;
        }
    }
}

fn separate_water<Beds>(
    stencils: &[WetStencil],
    spacing: f64,
    sample: impl Fn(&[[f64; 3]]) -> Result<Beds, AtlasGenerationError>,
) -> Result<Vec<bool>, AtlasGenerationError>
where
    Beds: IntoIterator<Item = f64>,
{
    let mut separate = vec![false; stencils.len()];
    if stencils.is_empty() {
        return Ok(separate);
    }
    let centres: Vec<_> = stencils.iter().map(WetStencil::centre).collect();
    let mut single_runs = Vec::new();
    let mut shared_paths = FxHashMap::default();
    let mut links = Vec::new();
    let mut counts = Vec::new();
    let mut directions = Vec::new();
    for (i, (stencil, bed)) in stencils.iter().zip(sample(&centres)?).enumerate() {
        let wet_centre = bed < 0.0;
        match perimeter(stencil.wet_rim) {
            // Refining a submerged interior surrounded by dry samples preserves
            // the bowl as its own potential inland floor.
            Perimeter::Empty => separate[i] = wet_centre,
            Perimeter::SeparateWetRuns => {
                separate[i] = !stencil.connected(wet_centre, spacing, [], &sample)?;
            }
            Perimeter::SingleWetRun => {
                let rim = stencil.rim();
                let first = links.len();
                // Five short links suffice for an entirely wet ring. A partial run may
                // cross the last-to-first link, so certify every consecutive wet pair.
                let end = if stencil.wet_rim.contains(&false) {
                    6
                } else {
                    5
                };
                for i in 0..end {
                    let j = (i + 1) % 6;
                    if !stencil.wet_rim[i] || !stencil.wet_rim[j] {
                        continue;
                    }
                    let path = *shared_paths.entry(stencil.rim_edge(i)).or_insert_with(|| {
                        let path = counts.len();
                        counts.push(add_path([rim[i], rim[j]], spacing, &mut directions));
                        path
                    });
                    links.push((i, j, path));
                }
                if wet_centre {
                    let port = [1, 3, 5, 0, 2, 4]
                        .into_iter()
                        .find(|&i| stencil.wet_rim[i])
                        .expect("a wet run contains a wet rim station");
                    links.push((port, 6, counts.len()));
                    counts.push(add_path(
                        [rim[port], stencil.centre()],
                        spacing,
                        &mut directions,
                    ));
                }
                if first != links.len() {
                    single_runs.push((i, stencil, wet_centre, first..links.len()));
                }
            }
        }
    }
    if single_runs.is_empty() {
        return Ok(separate);
    }
    let mut beds = sample(&directions)?.into_iter();
    let submerged: Vec<_> = counts
        .into_iter()
        .map(|count| submerged_path(&mut beds, count))
        .collect();
    for (i, stencil, wet_centre, range) in single_runs {
        let paths = &links[range];
        if paths.iter().any(|&(_, _, path)| !submerged[path])
            && !stencil.connected(
                wet_centre,
                spacing,
                paths.iter().map(|&(i, j, path)| (i, j, submerged[path])),
                &sample,
            )?
        {
            separate[i] = true;
        }
    }
    Ok(separate)
}

/// Reference trial values for real or prospective edge midpoints in one adaptation wave.
struct MidpointBeds {
    slots: FxHashMap<[u32; 2], usize>,
    values: Vec<f64>,
}

impl MidpointBeds {
    fn sample(
        fine: &FineTerrain,
        mesh: &Mesh,
        nodes: &[u32],
        cancel: &BuildCancellation,
    ) -> Result<Self, AtlasGenerationError> {
        let mut slots = FxHashMap::default();
        let mut directions = Vec::new();
        for &node in nodes {
            check_cancelled(cancel)?;
            for edge in edges(mesh.node_vertices(node)) {
                if mesh.existing_midpoint(edge).is_none() {
                    if let Entry::Vacant(entry) = slots.entry(edge_key(edge)) {
                        entry.insert(directions.len());
                        directions.push(mesh.edge_center(edge));
                    }
                }
            }
        }
        let values = vertex_beds(fine, &directions, cancel)?;
        Ok(Self { slots, values })
    }

    fn on_edge(&self, mesh: &Mesh, beds: &[f64], edge: [u32; 2]) -> f64 {
        match mesh.existing_midpoint(edge) {
            Some(vertex) => beds[vertex as usize],
            None => self.values[self.slots[&edge_key(edge)]],
        }
    }

    fn stencil(&self, mesh: &Mesh, beds: &[f64], node: u32) -> WetStencil {
        let vertices = mesh.node_vertices(node);
        let [a, b, c] = vertices.map(|v| beds[v as usize] < 0.0);
        let [ab, bc, ca] = edges(vertices).map(|edge| self.on_edge(mesh, beds, edge) < 0.0);
        WetStencil {
            corners: vertices.map(|v| mesh.directions[v as usize]),
            wet_rim: [a, ab, b, bc, c, ca],
            vertices,
        }
    }

    /// Install exactly the midpoint beds already sampled when this leaf failed its proof.
    fn split(
        &self,
        mesh: &mut Mesh,
        beds: &mut Vec<f64>,
        node: u32,
    ) -> Result<[u32; 4], AtlasGenerationError> {
        let [ab_bed, bc_bed, ca_bed] =
            edges(mesh.node_vertices(node)).map(|edge| self.on_edge(mesh, beds, edge));
        let children = mesh.split(node)?;
        let [bc, ca, ab] = mesh.node_vertices(children[3]);
        beds.resize(mesh.directions.len(), 0.0);
        for (vertex, value) in [(ab, ab_bed), (bc, bc_bed), (ca, ca_bed)] {
            beds[vertex as usize] = value;
        }
        Ok(children)
    }
}

fn edges([a, b, c]: [u32; 3]) -> [[u32; 2]; 3] {
    [[a, b], [b, c], [c, a]]
}

fn extend_beds(
    fine: &FineTerrain,
    mesh: &Mesh,
    beds: &mut Vec<f64>,
    cancel: &BuildCancellation,
) -> Result<(), AtlasGenerationError> {
    let missing = mesh.directions.get(beds.len()..).ok_or_else(|| {
        AtlasGenerationError::Geometry("more coastal beds than mesh vertices".into())
    })?;
    if !missing.is_empty() {
        beds.extend(vertex_beds(fine, missing, cancel)?);
    }
    Ok(())
}

/// Adapt only the supplied candidate leaves. First reach the coast working scale, then split
/// individual compartments with unresolved wet components until the fixed source spacing.
/// The caller owns mesh vertex beds in the same order. A cancelled or failed private build
/// propagates its error before publication.
pub(super) fn adapt(
    fine: &FineTerrain,
    mesh: &mut Mesh,
    beds: &mut Vec<f64>,
    mut pending: Vec<u32>,
    cancel: &BuildCancellation,
) -> Result<(), AtlasGenerationError> {
    check_cancelled(cancel)?;
    let native = fine.canonical_angular_step_radians();
    let parent_step = native
        * f64::from(fine.scale().pixels_per_unit())
        * f64::from(ATLAS_COAST_PARENT_RASTER_INTERVALS);
    extend_beds(fine, mesh, beds, cancel)?;
    while !pending.is_empty() {
        let mut inspect = Vec::new();
        let mut next = Vec::new();
        for node in pending.drain(..) {
            check_cancelled(cancel)?;
            let span = diameter(
                mesh.node_vertices(node)
                    .map(|v| mesh.directions[v as usize]),
            );
            if span > parent_step {
                next.extend(mesh.split(node)?);
            } else if span > native {
                inspect.push(node);
            }
        }
        extend_beds(fine, mesh, beds, cancel)?;
        if !inspect.is_empty() {
            let midpoints = MidpointBeds::sample(fine, mesh, &inspect, cancel)?;
            // A group shares one fine-terrain workspace per batch and certificates for
            // adjacent half edges. Its size affects scheduling only.
            const GROUP: usize = 128;
            #[cfg(not(target_arch = "wasm32"))]
            let groups = inspect.par_chunks(GROUP);
            #[cfg(target_arch = "wasm32")]
            let groups = inspect.chunks(GROUP);
            let unresolved = groups
                .map(|nodes| {
                    check_cancelled(cancel)?;
                    let stencils = nodes
                        .iter()
                        .map(|&n| midpoints.stencil(mesh, beds, n))
                        .collect::<Vec<_>>();
                    let separate = separate_water(&stencils, native, |directions| {
                        Ok(fine
                            .sample_surface_batch(directions, &|| cancel.is_cancelled())?
                            .into_iter()
                            .map(|sample| sample.bed_elevation_m))
                    })?;
                    Ok(nodes
                        .iter()
                        .copied()
                        .zip(separate)
                        .filter_map(|(node, split)| split.then_some(node))
                        .collect::<Vec<_>>())
                })
                .collect::<Result<Vec<_>, AtlasGenerationError>>()?;
            for node in unresolved.into_iter().flatten() {
                check_cancelled(cancel)?;
                next.extend(midpoints.split(mesh, beds, node)?);
            }
        }
        pending = next;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_separate_wet_lobes_from_one_straight_shore() {
        let stencil = [[0, 0], [1, 0], [2, 0], [1, 1], [0, 2], [0, 1]];
        let straight = stencil.map(|[x, _]| x - 1 < 0);
        assert_eq!(perimeter(straight), Perimeter::SingleWetRun);
        assert_eq!(
            perimeter([true, false, true, false, false, false]),
            Perimeter::SeparateWetRuns
        );
        // A single wet corner beside two sea-level midpoints still requires a centre check.
        let touching = stencil.map(|[x, y]| x + y - 1 < 0);
        assert_eq!(perimeter(touching), Perimeter::SingleWetRun);
    }

    #[test]
    fn a_sampled_ridge_separates_bays_until_the_channel_is_submerged() {
        // Gnomonic (east, north) coordinates form two straight arms from (-1, 0)
        // and (1, 0) to the wet centre (0, 1). Their common boundary midpoint is dry.
        for east_bay in [false, true] {
            let stencil = WetStencil {
                corners: [[1., -1., 0.], [1., 1., 0.], [1., 0., 3.]],
                wet_rim: [true, false, east_bay, false, false, false],
                vertices: [0, 1, 2],
            };
            let connected = |ridge_m: f64| {
                let bed = |[radial, east, north]: [f64; 3]| {
                    let (x, y) = (east / radial, north / radial);
                    if (!east_bay && x > 0.0) || (y - (1.0 - x.abs())).abs() > 0.1 {
                        1.0
                    } else if (0.25..=0.75).contains(&x.abs()) {
                        ridge_m
                    } else {
                        -1.0
                    }
                };
                assert!(bed(stencil.centre()) < 0.0);
                // Even when a whole arm is shorter than the sample spacing, its
                // midpoint must expose the intervening ridge.
                stencil
                    .connected(true, std::f64::consts::PI, [], |points| {
                        Ok(points.iter().copied().map(bed).collect::<Vec<_>>())
                    })
                    .unwrap()
            };
            assert!(!connected(1.0));
            assert!(!connected(0.0));
            assert!(connected(-1.0));
        }
    }

    #[test]
    fn a_bowl_behind_a_single_wet_coast_requires_an_opening_through_its_rim() {
        let corners = [[1., -1., 0.], [1., 1., 0.], [1., 0., 3.]];
        for flooded_surroundings in [false, true] {
            for channel_open in [false, true] {
                let bed = |[radial, east, north]: [f64; 3]| {
                    let (x, y) = (east / radial, north / radial);
                    let distance = x.hypot(y - 1.0);
                    let wet = distance < 0.2
                        || y < 0.2
                        || (flooded_surroundings && distance > 0.6)
                        || (channel_open && x.abs() < 0.1 && y < 1.1);
                    if wet {
                        -1.0
                    } else {
                        1.0
                    }
                };
                let mut stencil = WetStencil {
                    corners,
                    wet_rim: [false; 6],
                    vertices: [0, 1, 2],
                };
                stencil.wet_rim = stencil.rim().map(|p| bed(p) < 0.0);
                assert_eq!(
                    stencil.wet_rim,
                    if flooded_surroundings {
                        [true; 6]
                    } else {
                        [true, true, true, false, false, false]
                    }
                );
                assert!(bed(stencil.centre()) < 0.0);
                let separate = separate_water(&[stencil], 0.1, |points| {
                    Ok(points.iter().copied().map(bed).collect::<Vec<_>>())
                })
                .unwrap();
                assert_eq!(separate, [!channel_open]);
            }
        }
        // Below-sea interior with a wholly dry perimeter must keep its own closed floor.
        let bowl = WetStencil {
            corners,
            wet_rim: [false; 6],
            vertices: [0, 1, 2],
        };
        assert_eq!(
            separate_water(&[bowl], 0.1, |centres| Ok(vec![-1.0; centres.len()])).unwrap(),
            [true]
        );
    }

    #[test]
    fn adjacent_wet_rim_stations_require_a_submerged_path_when_the_centre_is_dry() {
        for barrier_m in [1.0, 0.0, -1.0] {
            let mut stencil = WetStencil {
                corners: [[1., -1., 0.], [1., 1., 0.], [1., 0., 3.]],
                wet_rim: [false; 6],
                vertices: [0, 1, 2],
            };
            let bed = |[radial, east, north]: [f64; 3]| {
                let (x, y) = (east / radial, north / radial);
                if y.abs() > 0.1 || x > 0.1 {
                    1.0
                } else if (-0.75..=-0.25).contains(&x) {
                    barrier_m
                } else {
                    -1.0
                }
            };
            stencil.wet_rim = stencil.rim().map(|point| bed(point) < 0.0);
            assert_eq!(stencil.wet_rim, [true, true, false, false, false, false]);
            assert!(bed(stencil.centre()) > 0.0);
            let neighbour = WetStencil {
                corners: [stencil.corners[1], [1., 2., 3.], stencil.corners[2]],
                vertices: [1, 3, 2],
                wet_rim: [false; 6],
            };
            let separate = separate_water(&[stencil, neighbour], 0.1, |points| {
                Ok(points.iter().copied().map(bed).collect::<Vec<_>>())
            })
            .unwrap();
            assert_eq!(separate, [barrier_m >= 0.0, false]);
        }
    }
}
