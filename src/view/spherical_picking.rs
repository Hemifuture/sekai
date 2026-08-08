//! Renderer-neutral direction picking on the authoritative unit sphere.

use std::cmp::Ordering;

use thiserror::Error;

use super::SphericalPresentationSource;
use crate::world::spatial::{
    central_angle, SphereGeometryError, SphericalSurfaceSnapshot, SurfaceRef, SurfaceRefError,
    UnitVector3,
};
use crate::world::{CellId, EdgeId};

const DISCRIMINANT_ROUNDOFF_ULPS: f64 = 16.0;

/// A source-bound cache for deterministic spherical cell and edge picking.
#[derive(Debug, Clone)]
pub struct SphericalEntityLocator {
    source: SphericalPresentationSource,
    cells: Vec<CachedCell>,
}

#[derive(Debug, Clone)]
struct CachedCell {
    id: CellId,
    site: UnitVector3,
    incident_edges: Vec<CachedEdge>,
}

#[derive(Debug, Clone)]
struct CachedEdge {
    id: EdgeId,
    endpoints: [UnitVector3; 2],
    midpoint: UnitVector3,
}

impl SphericalEntityLocator {
    /// Builds a locator cache from one authoritative spherical surface.
    pub fn new(
        source: SphericalPresentationSource,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<Self, SphericalPickingError> {
        let surface_ref = SurfaceRef::try_for_spherical(surface)?;
        if source.surface_ref() != surface_ref {
            return Err(SphericalPickingError::SourceSurfaceMismatch {
                source_ref: source.surface_ref(),
                surface: surface_ref,
            });
        }

        let cells = surface
            .cells()
            .iter()
            .map(|cell| CachedCell {
                id: cell.id,
                site: cell.site,
                incident_edges: cell
                    .boundary_edges
                    .iter()
                    .map(|&edge_id| {
                        let edge = surface
                            .edge(edge_id)
                            .expect("validated cell boundary edge must exist");
                        let endpoints = edge.vertices.map(|vertex_id| {
                            surface
                                .vertex(vertex_id)
                                .expect("validated edge vertex must exist")
                                .position
                        });
                        CachedEdge {
                            id: edge.id,
                            endpoints,
                            midpoint: edge.midpoint,
                        }
                    })
                    .collect(),
            })
            .collect();
        Ok(Self { source, cells })
    }

    /// Returns the immutable source identity of this cache.
    pub const fn source(&self) -> &SphericalPresentationSource {
        &self.source
    }

    /// Finds the nearest authoritative Voronoi site by maximum dot product.
    ///
    /// This performs an O(cell-count) scan and is intended for discrete picking,
    /// not per-frame or hover processing.
    pub fn locate_cell(&self, direction: UnitVector3) -> Option<CellId> {
        self.cells
            .iter()
            .map(|cell| (cell.id, cell.site.dot(direction)))
            .max_by(|(left_id, left_dot), (right_id, right_dot)| {
                match left_dot.total_cmp(right_dot) {
                    Ordering::Equal => right_id.cmp(left_id),
                    ordering => ordering,
                }
            })
            .map(|(id, _)| id)
    }

    /// Finds the closest cached boundary edge of `cell` within `tolerance` radians.
    pub fn locate_incident_edge(
        &self,
        cell: CellId,
        direction: UnitVector3,
        tolerance: f64,
    ) -> Option<EdgeId> {
        if !tolerance.is_finite() || !(0.0..=std::f64::consts::PI).contains(&tolerance) {
            return None;
        }
        let cell = self.cells.iter().find(|candidate| candidate.id == cell)?;
        cell.incident_edges
            .iter()
            .filter_map(|edge| {
                let distance = minor_arc_segment_distance(direction, edge);
                (distance <= tolerance).then_some((edge.id, distance))
            })
            .min_by(|(left_id, left_distance), (right_id, right_distance)| {
                match left_distance.total_cmp(right_distance) {
                    Ordering::Equal => left_id.cmp(right_id),
                    ordering => ordering,
                }
            })
            .map(|(id, _)| id)
    }
}

/// Failures while binding a picking cache to an authoritative spherical surface.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalPickingError {
    /// The supplied surface does not match the locator's presentation source.
    #[error("locator source surface does not match the supplied spherical surface")]
    SourceSurfaceMismatch {
        /// Surface identity stored by the presentation source.
        source_ref: SurfaceRef,
        /// Identity derived from the supplied surface.
        surface: SurfaceRef,
    },
    /// The supplied spherical surface was not authoritative and valid.
    #[error("invalid spherical surface for picking: {0}")]
    InvalidSurface(#[from] SurfaceRefError),
}

/// A finite ray with a normalized direction for unit-sphere intersection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitRay {
    origin: [f64; 3],
    direction: UnitVector3,
}

impl UnitRay {
    /// Validates the origin and normalizes the ray direction.
    pub fn new(origin: [f64; 3], direction: [f64; 3]) -> Result<Self, RayError> {
        if origin.into_iter().any(|component| !component.is_finite()) {
            return Err(RayError::NonFiniteOrigin);
        }
        Ok(Self {
            origin,
            direction: UnitVector3::new(direction[0], direction[1], direction[2])
                .map_err(RayError::InvalidDirection)?,
        })
    }

    /// Returns the finite ray origin.
    pub const fn origin(self) -> [f64; 3] {
        self.origin
    }

    /// Returns the normalized ray direction.
    pub const fn direction(self) -> UnitVector3 {
        self.direction
    }
}

/// A nearest non-negative intersection of a ray with the unit sphere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaySphereHit {
    direction: UnitVector3,
    distance: f64,
}

impl RaySphereHit {
    /// Returns the normalized direction from the sphere center to the hit.
    pub const fn direction(self) -> UnitVector3 {
        self.direction
    }

    /// Returns the ray distance to the hit point.
    pub const fn distance(self) -> f64 {
        self.distance
    }
}

/// Errors constructing a unit-sphere ray.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RayError {
    /// A ray origin component was non-finite.
    #[error("ray origin components must be finite")]
    NonFiniteOrigin,
    /// The ray direction could not be normalized to a finite unit vector.
    #[error("invalid ray direction: {0}")]
    InvalidDirection(#[from] SphereGeometryError),
}

/// Returns the nearest non-negative intersection of `ray` with the unit sphere.
pub fn intersect_unit_sphere(ray: UnitRay) -> Option<RaySphereHit> {
    let origin = ray.origin();
    let direction = ray.direction().components();
    let b = 2.0 * dot(origin, direction);
    let c = dot(origin, origin) - 1.0;
    let four_c = 4.0 * c;
    let raw_discriminant = b.mul_add(b, -four_c);
    if !raw_discriminant.is_finite() {
        return None;
    }
    let scale = b.mul_add(b, 0.0).abs() + four_c.abs();
    let roundoff_bound = DISCRIMINANT_ROUNDOFF_ULPS * f64::EPSILON * scale;
    let discriminant = if raw_discriminant < 0.0 {
        if roundoff_bound.is_finite() && raw_discriminant >= -roundoff_bound {
            0.0
        } else {
            return None;
        }
    } else {
        raw_discriminant
    };

    let root = discriminant.sqrt();
    let q = -0.5 * (b + if b >= 0.0 { root } else { -root });
    let first = if q == 0.0 { -b * 0.5 } else { q };
    let second = if q == 0.0 { -b * 0.5 } else { c / q };
    let distance = [first, second]
        .into_iter()
        .filter(|candidate| *candidate >= 0.0 && candidate.is_finite())
        .min_by(f64::total_cmp)?;
    let point = [
        origin[0] + distance * direction[0],
        origin[1] + distance * direction[1],
        origin[2] + distance * direction[2],
    ];
    let direction = UnitVector3::new(point[0], point[1], point[2]).ok()?;
    Some(RaySphereHit {
        direction,
        distance,
    })
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn minor_arc_segment_distance(point: UnitVector3, edge: &CachedEdge) -> f64 {
    let [first, second] = edge.endpoints;
    let first_to_second = central_angle(first, second);
    let normal_components = cross(first.components(), second.components());
    let normal_length = norm(normal_components);
    if normal_length > 0.0 && normal_length.is_finite() {
        let normal = normal_components.map(|component| component / normal_length);
        let signed_height = dot(point.components(), normal);
        let projected = [
            point.components()[0] - signed_height * normal[0],
            point.components()[1] - signed_height * normal[1],
            point.components()[2] - signed_height * normal[2],
        ];
        if let Ok(foot) = UnitVector3::new(projected[0], projected[1], projected[2]) {
            let through_foot = central_angle(first, foot) + central_angle(foot, second);
            if through_foot.total_cmp(&(first_to_second + 64.0 * f64::EPSILON)) != Ordering::Greater
            {
                return central_angle(point, foot);
            }
        }
    }
    central_angle(point, first)
        .min(central_angle(point, second))
        .min(central_angle(point, edge.midpoint))
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use crate::engine::{BuildEngine, BuildResultHash, ExternalArtifacts, MemoryStageCache};
    use crate::generators::spatial::{
        spherical_foundation_graph, SphericalSpaceArtifact, SphericalSurfaceArtifact,
    };
    use crate::view::{
        ProjectionPoint, SphericalPresentationSource, SphericalProjection,
        SphericalProjectionError, SphericalProjectionKind,
    };
    use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
    use crate::world::{CellId, EdgeId, Meters, RootSeed, SphericalSpaceSpec};

    use super::{CachedCell, CachedEdge, SphericalEntityLocator};

    fn surface() -> SphericalSurfaceSnapshot {
        let space = SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        };
        let mut inputs = ExternalArtifacts::new();
        inputs.insert(SphericalSpaceArtifact::new(space)).unwrap();
        let outcome = BuildEngine::new(spherical_foundation_graph().unwrap())
            .build(RootSeed::new(7), inputs, &mut MemoryStageCache::new())
            .unwrap();
        outcome
            .artifacts
            .get::<SphericalSurfaceArtifact>()
            .unwrap()
            .snapshot()
            .clone()
    }

    fn source(surface: &SphericalSurfaceSnapshot) -> SphericalPresentationSource {
        SphericalPresentationSource::new(
            RootSeed::new(7),
            SurfaceRef::for_spherical(surface),
            BuildResultHash::new([7; 32]),
            1,
        )
    }

    #[test]
    fn generated_surface_sites_and_incident_edge_midpoints_resolve_to_stable_ids() {
        let surface = surface();
        let locator = SphericalEntityLocator::new(source(&surface), &surface).unwrap();

        for cell in surface.cells() {
            assert_eq!(locator.locate_cell(cell.site), Some(cell.id));
        }
        for edge in surface.edges() {
            for owner in edge.cells {
                assert_eq!(
                    locator.locate_incident_edge(owner, edge.midpoint, PI),
                    Some(edge.id)
                );
            }
        }
    }

    #[test]
    fn equal_dot_and_equal_incident_edge_distances_choose_lowest_stable_ids() {
        let source = source(&surface());
        let east = UnitVector3::new(1.0, 0.0, 0.0).unwrap();
        let forward = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
        let locator = SphericalEntityLocator {
            source,
            cells: vec![
                CachedCell {
                    id: CellId::from_raw(9),
                    site: east,
                    incident_edges: vec![
                        CachedEdge {
                            id: EdgeId::from_raw(8),
                            endpoints: [direction(-40.0), direction(-20.0)],
                            midpoint: direction(-30.0),
                        },
                        CachedEdge {
                            id: EdgeId::from_raw(2),
                            endpoints: [direction(20.0), direction(40.0)],
                            midpoint: direction(30.0),
                        },
                    ],
                },
                CachedCell {
                    id: CellId::from_raw(3),
                    site: east,
                    incident_edges: Vec::new(),
                },
            ],
        };

        assert_eq!(locator.locate_cell(east), Some(CellId::from_raw(3)));
        assert_eq!(
            locator.locate_incident_edge(CellId::from_raw(9), east, PI),
            Some(EdgeId::from_raw(2))
        );
        assert_eq!(
            locator.locate_incident_edge(CellId::from_raw(9), direction(20.0), 0.0),
            Some(EdgeId::from_raw(2))
        );
        assert_eq!(locator.locate_cell(forward), Some(CellId::from_raw(3)));
    }

    #[test]
    fn projection_and_camera_ray_select_the_same_cell_and_invalid_hits_return_none() {
        let surface = surface();
        let locator = SphericalEntityLocator::new(source(&surface), &surface).unwrap();
        let expected = UnitVector3::new(0.2, -0.7, 0.6).unwrap();
        let [x, y, z] = expected.components();
        let hit = super::intersect_unit_sphere(
            super::UnitRay::new([3.0 * x, 3.0 * y, 3.0 * z], [-x, -y, -z]).unwrap(),
        )
        .unwrap();

        for kind in [
            SphericalProjectionKind::EqualEarth,
            SphericalProjectionKind::Equirectangular,
        ] {
            let projection = SphericalProjection::new(kind, 0.25).unwrap();
            let inverse = projection
                .inverse(projection.forward(expected).unwrap())
                .unwrap();
            assert_eq!(
                locator.locate_cell(inverse),
                locator.locate_cell(hit.direction())
            );
            assert_eq!(
                projection.inverse(ProjectionPoint::new(10.0, 10.0)),
                Err(SphericalProjectionError::OutsideProjectionOutline)
            );
        }
        assert!(super::intersect_unit_sphere(
            super::UnitRay::new([0.0, 0.0, 3.0], [1.0, 0.0, 0.0]).unwrap()
        )
        .is_none());

        let cell = &surface.cells()[0];
        assert_eq!(
            locator.locate_incident_edge(cell.id, cell.site, 1.0e-12),
            None
        );
        let non_incident = surface
            .edges()
            .iter()
            .find(|edge| !cell.boundary_edges.contains(&edge.id))
            .unwrap();
        assert_eq!(
            locator.locate_incident_edge(cell.id, non_incident.midpoint, 1.0e-12),
            None
        );
        assert_eq!(locator.locate_incident_edge(cell.id, cell.site, -0.1), None);
        assert_eq!(
            locator.locate_incident_edge(cell.id, cell.site, f64::INFINITY),
            None
        );
    }

    fn direction(longitude_degrees: f64) -> UnitVector3 {
        let longitude = longitude_degrees.to_radians();
        UnitVector3::new(longitude.cos(), longitude.sin(), 0.0).unwrap()
    }
}
