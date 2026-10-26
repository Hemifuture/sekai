//! Test-side continental shape metrics M1-M4 of the G4 spec §7.1
//! (`docs/superpowers/specs/2026-10-08-g4-continental-geometry-physics-design.md`).
//!
//! These are acceptance measurements, not production algorithms: they read a
//! per-cell boolean mask on an authoritative spherical surface and use the
//! surface's own edge topology for connectivity. Earth reference masks are
//! loaded from bit-packed fixtures bound to the surface by `SurfaceRef`.

use std::collections::VecDeque;
use std::path::Path;

use sekai::world::spatial::{central_angle, SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
use sekai::world::CellId;

/// Number of equal caps in M1: the six geological continents of Mortimer et al. (2017).
pub const SIX_CAP_COUNT: usize = 6;
/// M2 major-block threshold as a fraction of the sphere (G4 §7.1, diagnosis convention).
pub const MAJOR_BLOCK_SPHERE_FRACTION: f64 = 0.01;
/// M4 box radii in graph hops (G4 §7.1).
pub const BOX_RADII_HOPS: [usize; 3] = [4, 8, 16];
/// Lloyd iterations stop when the assignment repeats; this only bounds a cycle.
const MAXIMUM_LLOYD_ITERATIONS: usize = 256;

/// The four G4 §7.1 shape metrics of one mask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeMetrics {
    /// M1: IoU of the mask with six equal-area Lloyd-placed caps.
    pub six_cap_iou: f64,
    /// M2: area coefficient of variation of blocks >= 1 % of the sphere.
    pub major_area_cv: f64,
    /// M3: largest tangent-covariance aspect ratio among major blocks.
    pub max_elongation: f64,
    /// M4: greedy graph box-covering dimension of the mask boundary.
    pub box_dimension: f64,
    /// Number of blocks counted by M2 and M3 (record item).
    pub major_blocks: usize,
    /// Mask area as a fraction of the sphere (record item).
    pub area_fraction: f64,
}

/// Precomputed cell adjacency of an authoritative surface.
pub struct CellGraph<'a> {
    surface: &'a SphericalSurfaceSnapshot,
    neighbors: Vec<Vec<u32>>,
}

impl<'a> CellGraph<'a> {
    /// Builds the adjacency list from the surface's edge topology.
    pub fn new(surface: &'a SphericalSurfaceSnapshot) -> Self {
        let neighbors = surface
            .cells()
            .iter()
            .map(|cell| {
                surface
                    .cell_edges(cell.id)
                    .expect("authoritative cells have edges")
                    .iter()
                    .map(|&edge| {
                        surface
                            .opposite_cell(cell.id, edge)
                            .expect("every edge has an opposite cell")
                            .raw()
                    })
                    .collect()
            })
            .collect();
        Self { surface, neighbors }
    }

    /// Returns the underlying surface.
    pub fn surface(&self) -> &'a SphericalSurfaceSnapshot {
        self.surface
    }

    fn area(&self, index: usize) -> f64 {
        self.surface.cells()[index].area.get()
    }

    fn centroid(&self, index: usize) -> UnitVector3 {
        self.surface.cells()[index].centroid
    }

    /// Computes M1-M4 for a mask indexed by raw cell id.
    pub fn metrics(&self, mask: &[bool]) -> ShapeMetrics {
        assert_eq!(mask.len(), self.neighbors.len(), "mask covers every cell");
        let sphere = self.surface.total_cell_area().get();
        let components = self.components(mask);
        let major = components
            .iter()
            .filter(|cells| {
                cells.iter().map(|&cell| self.area(cell)).sum::<f64>() / sphere
                    >= MAJOR_BLOCK_SPHERE_FRACTION
            })
            .collect::<Vec<_>>();
        let major_areas = major
            .iter()
            .map(|cells| cells.iter().map(|&cell| self.area(cell)).sum::<f64>())
            .collect::<Vec<_>>();
        let max_elongation = major
            .iter()
            .map(|cells| {
                let ids = cells
                    .iter()
                    .map(|&cell| CellId::from_raw(cell as u32))
                    .collect::<Vec<_>>();
                tangent_covariance_aspect_ratio(self.surface, &ids)
            })
            .fold(f64::NAN, f64::max);
        let area = (0..mask.len())
            .filter(|&cell| mask[cell])
            .map(|cell| self.area(cell))
            .sum::<f64>();
        ShapeMetrics {
            six_cap_iou: self.six_cap_iou(mask),
            major_area_cv: coefficient_of_variation(&major_areas),
            max_elongation,
            box_dimension: self.box_dimension(mask),
            major_blocks: major.len(),
            area_fraction: area / sphere,
        }
    }

    /// Edge-connected components of the mask, each as raw cell indices.
    pub fn components(&self, mask: &[bool]) -> Vec<Vec<usize>> {
        let mut visited = vec![false; mask.len()];
        let mut components = Vec::new();
        for start in 0..mask.len() {
            if visited[start] || !mask[start] {
                continue;
            }
            visited[start] = true;
            let mut queue = VecDeque::from([start]);
            let mut cells = Vec::new();
            while let Some(cell) = queue.pop_front() {
                cells.push(cell);
                for &neighbor in &self.neighbors[cell] {
                    let neighbor = neighbor as usize;
                    if mask[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        queue.push_back(neighbor);
                    }
                }
            }
            components.push(cells);
        }
        components
    }

    /// M1: six equal caps (each 1/6 of the mask area) placed by spherical
    /// Lloyd iteration (Lloyd 1982) from a farthest-point initialisation over
    /// the mask cells; IoU (Jaccard 1912) is cell-area weighted, a cell
    /// belonging to a cap when its centroid lies inside it.
    pub fn six_cap_iou(&self, mask: &[bool]) -> f64 {
        let cells = (0..mask.len())
            .filter(|&cell| mask[cell])
            .collect::<Vec<_>>();
        if cells.len() < SIX_CAP_COUNT {
            return f64::NAN;
        }
        let total = cells.iter().map(|&cell| self.area(cell)).sum::<f64>();
        let radius = self.surface.radius().get();
        let cap_area = total / SIX_CAP_COUNT as f64;
        let cap_angle = (1.0 - cap_area / (2.0 * std::f64::consts::PI * radius * radius))
            .clamp(-1.0, 1.0)
            .acos();
        let mut centers = self.farthest_point_centers(&cells);
        let mut assignment = vec![usize::MAX; cells.len()];
        for _ in 0..MAXIMUM_LLOYD_ITERATIONS {
            let next = cells
                .iter()
                .map(|&cell| nearest_center(self.centroid(cell), &centers))
                .collect::<Vec<_>>();
            if next == assignment {
                break;
            }
            assignment = next;
            let mut sums = vec![[0.0; 3]; SIX_CAP_COUNT];
            for (&cell, &center) in cells.iter().zip(&assignment) {
                let weight = self.area(cell);
                for (sum, component) in sums[center]
                    .iter_mut()
                    .zip(self.centroid(cell).components())
                {
                    *sum += weight * component;
                }
            }
            for (center, sum) in centers.iter_mut().zip(sums) {
                if let Ok(mean) = UnitVector3::new(sum[0], sum[1], sum[2]) {
                    *center = mean;
                }
            }
        }
        let mut intersection = 0.0;
        let mut union = 0.0;
        for (index, &inside) in mask.iter().enumerate() {
            let centroid = self.centroid(index);
            let in_cap = centers
                .iter()
                .any(|&center| central_angle(centroid, center) <= cap_angle);
            if inside && in_cap {
                intersection += self.area(index);
            }
            if inside || in_cap {
                union += self.area(index);
            }
        }
        intersection / union
    }

    /// Farthest-point sampling: the first center is the mask cell nearest the
    /// mask's area centroid, each further one maximises the angular distance
    /// to the chosen set; ties break by the lower cell id.
    fn farthest_point_centers(&self, cells: &[usize]) -> Vec<UnitVector3> {
        let ids = cells
            .iter()
            .map(|&cell| CellId::from_raw(cell as u32))
            .collect::<Vec<_>>();
        let mean = area_centroid(self.surface, &ids);
        let first = *cells
            .iter()
            .min_by(|&&a, &&b| {
                central_angle(self.centroid(a), mean)
                    .total_cmp(&central_angle(self.centroid(b), mean))
            })
            .expect("the mask is non-empty");
        let mut centers = vec![self.centroid(first)];
        let mut distance = cells
            .iter()
            .map(|&cell| central_angle(self.centroid(cell), centers[0]))
            .collect::<Vec<_>>();
        while centers.len() < SIX_CAP_COUNT {
            let (best, _) = distance
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1).then_with(|| b.0.cmp(&a.0)))
                .expect("the mask is non-empty");
            let center = self.centroid(cells[best]);
            centers.push(center);
            for (value, &cell) in distance.iter_mut().zip(cells) {
                *value = value.min(central_angle(self.centroid(cell), center));
            }
        }
        centers
    }

    /// M4: boundary cells are mask cells with an off-mask neighbour. For each
    /// radius r, greedy covering in cell-id order opens a box at the first
    /// uncovered boundary cell and covers every boundary cell within r graph
    /// hops (box covering on graphs, Song, Havlin & Makse 2005). The
    /// dimension is minus the least-squares slope of ln N against ln r
    /// (Mandelbrot 1967).
    pub fn box_dimension(&self, mask: &[bool]) -> f64 {
        let boundary = (0..mask.len())
            .map(|cell| {
                mask[cell]
                    && self.neighbors[cell]
                        .iter()
                        .any(|&neighbor| !mask[neighbor as usize])
            })
            .collect::<Vec<_>>();
        let points = BOX_RADII_HOPS
            .iter()
            .map(|&radius| {
                let count = self.greedy_box_count(&boundary, radius);
                ((radius as f64).ln(), (count as f64).ln())
            })
            .collect::<Vec<_>>();
        -least_squares_slope(&points)
    }

    fn greedy_box_count(&self, boundary: &[bool], radius: usize) -> usize {
        let mut covered = vec![false; boundary.len()];
        let mut hops = vec![usize::MAX; boundary.len()];
        let mut touched = Vec::new();
        let mut boxes = 0;
        for start in 0..boundary.len() {
            if !boundary[start] || covered[start] {
                continue;
            }
            boxes += 1;
            for &cell in &touched {
                hops[cell] = usize::MAX;
            }
            touched.clear();
            hops[start] = 0;
            touched.push(start);
            let mut queue = VecDeque::from([start]);
            while let Some(cell) = queue.pop_front() {
                if boundary[cell] {
                    covered[cell] = true;
                }
                if hops[cell] == radius {
                    continue;
                }
                for &neighbor in &self.neighbors[cell] {
                    let neighbor = neighbor as usize;
                    if hops[neighbor] == usize::MAX {
                        hops[neighbor] = hops[cell] + 1;
                        touched.push(neighbor);
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        boxes
    }
}

fn nearest_center(point: UnitVector3, centers: &[UnitVector3]) -> usize {
    centers
        .iter()
        .enumerate()
        .min_by(|a, b| {
            central_angle(point, *a.1)
                .total_cmp(&central_angle(point, *b.1))
                .then_with(|| a.0.cmp(&b.0))
        })
        .expect("six centers")
        .0
}

fn coefficient_of_variation(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt() / mean
}

fn least_squares_slope(points: &[(f64, f64)]) -> f64 {
    let count = points.len() as f64;
    let mean_x = points.iter().map(|point| point.0).sum::<f64>() / count;
    let mean_y = points.iter().map(|point| point.1).sum::<f64>() / count;
    let covariance = points
        .iter()
        .map(|point| (point.0 - mean_x) * (point.1 - mean_y))
        .sum::<f64>();
    let variance = points
        .iter()
        .map(|point| (point.0 - mean_x).powi(2))
        .sum::<f64>();
    covariance / variance
}

/// Unit vector of `vector`.
pub fn normalize(vector: [f64; 3]) -> [f64; 3] {
    let length = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    vector.map(|value| value / length)
}

/// Euclidean dot product.
pub fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

/// Euclidean cross product.
pub fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

/// Area-weighted centroid direction of a cell set.
pub fn area_centroid(surface: &SphericalSurfaceSnapshot, cells: &[CellId]) -> UnitVector3 {
    let sum = cells.iter().fold([0.0; 3], |mut sum, &cell| {
        let record = surface.cell(cell).unwrap();
        for (target, component) in sum.iter_mut().zip(record.centroid.components()) {
            *target += record.area.get() * component;
        }
        sum
    });
    UnitVector3::new(sum[0], sum[1], sum[2]).unwrap()
}

/// A right-handed tangent basis (east, north) at `radial`.
pub fn tangent_basis(radial: UnitVector3) -> ([f64; 3], [f64; 3]) {
    let radial = radial.components();
    let reference = if radial[2].abs() < 0.8 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let east = normalize(cross(reference, radial));
    let north = normalize(cross(radial, east));
    (east, north)
}

/// M3 for one block: the principal-axis ratio sqrt(lambda1 / lambda2) of the
/// area-weighted covariance of azimuthal-equidistant tangent coordinates
/// about the block's area centroid (Pearson 1901).
pub fn tangent_covariance_aspect_ratio(
    surface: &SphericalSurfaceSnapshot,
    cells: &[CellId],
) -> f64 {
    let center = area_centroid(surface, cells);
    let center_components = center.components();
    let (east, north) = tangent_basis(center);
    let mut samples = Vec::with_capacity(cells.len());
    let mut total_weight = 0.0;
    for &cell in cells {
        let record = surface.cell(cell).unwrap();
        let radial = record.centroid.components();
        let cosine = dot(center_components, radial).clamp(-1.0, 1.0);
        let angle = cosine.acos();
        let tangent = [
            radial[0] - center_components[0] * cosine,
            radial[1] - center_components[1] * cosine,
            radial[2] - center_components[2] * cosine,
        ];
        let tangent_length = dot(tangent, tangent).sqrt();
        let coordinates = if tangent_length <= f64::EPSILON {
            [0.0, 0.0]
        } else {
            let scale = angle * surface.radius().get() / tangent_length;
            [dot(tangent, east) * scale, dot(tangent, north) * scale]
        };
        samples.push((coordinates, record.area.get()));
        total_weight += record.area.get();
    }
    let mean = samples.iter().fold([0.0; 2], |mut sum, (point, weight)| {
        sum[0] += point[0] * weight;
        sum[1] += point[1] * weight;
        sum
    });
    let mean = [mean[0] / total_weight, mean[1] / total_weight];
    let [xx, xy, yy] = samples.iter().fold([0.0; 3], |mut sum, (point, weight)| {
        let x = point[0] - mean[0];
        let y = point[1] - mean[1];
        sum[0] += x * x * weight;
        sum[1] += x * y * weight;
        sum[2] += y * y * weight;
        sum
    });
    let xx = xx / total_weight;
    let xy = xy / total_weight;
    let yy = yy / total_weight;
    let trace = xx + yy;
    let discriminant = ((xx - yy).powi(2) + 4.0 * xy.powi(2)).sqrt();
    let maximum = ((trace + discriminant) * 0.5).max(0.0);
    let minimum = ((trace - discriminant) * 0.5).max(f64::EPSILON);
    (maximum / minimum).sqrt()
}

/// Equal-cap null model of G4 §7.3: six caps of `area_fraction / 6` of the
/// sphere each, centred on the octahedron vertices (mutually 90 deg apart, so
/// they never overlap below a total fraction of 1 - cos 45 deg = 0.29 per cap).
pub fn six_equal_caps_mask(surface: &SphericalSurfaceSnapshot, area_fraction: f64) -> Vec<bool> {
    let cap_fraction = area_fraction / SIX_CAP_COUNT as f64;
    let cap_angle = (1.0 - 2.0 * cap_fraction).clamp(-1.0, 1.0).acos();
    let axes = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ]
    .map(|axis| UnitVector3::new(axis[0], axis[1], axis[2]).unwrap());
    surface
        .cells()
        .iter()
        .map(|cell| {
            axes.iter()
                .any(|&axis| central_angle(cell.centroid, axis) <= cap_angle)
        })
        .collect()
}

/// Earth reference mask thresholds produced by `probe_earth_reference_masks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarthThreshold {
    /// Main gate mask: cell-mean ETOPO1 ice-surface elevation above -200 m (G4 Q1).
    ShelfBreak,
    /// Record item: cell-mean elevation above 0 m.
    SeaLevel,
}

impl EarthThreshold {
    /// Elevation threshold in metres; a cell is inside when its mean is strictly above it.
    pub const fn meters(self) -> f64 {
        match self {
            Self::ShelfBreak => -200.0,
            Self::SeaLevel => 0.0,
        }
    }

    /// Fixture file stem component.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ShelfBreak => "minus200m",
            Self::SeaLevel => "0m",
        }
    }
}

/// Fixture header: magic, then the 32-byte surface fingerprint, the u32 cell
/// count and the u32 edge count, then ceil(cells / 8) bytes, LSB-first.
const FIXTURE_MAGIC: &[u8; 16] = b"sekai.earthmask1";

/// Directory of the committed Earth fixtures.
pub fn earth_fixture_directory() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/earth")
}

/// Fixture path for one profile label (`draft` / `standard`) and threshold.
pub fn earth_fixture_path(profile: &str, threshold: EarthThreshold) -> std::path::PathBuf {
    earth_fixture_directory().join(format!("{profile}-{}.bin", threshold.label()))
}

/// Serialises a mask bound to `surface`.
pub fn encode_earth_mask(surface: &SphericalSurfaceSnapshot, mask: &[bool]) -> Vec<u8> {
    let surface_ref = SurfaceRef::for_spherical(surface);
    let mut bytes = Vec::with_capacity(FIXTURE_MAGIC.len() + 40 + mask.len().div_ceil(8));
    bytes.extend_from_slice(FIXTURE_MAGIC);
    bytes.extend_from_slice(&surface_ref.fingerprint());
    bytes.extend_from_slice(&surface_ref.cell_count().to_le_bytes());
    bytes.extend_from_slice(&surface_ref.edge_count().to_le_bytes());
    let mut packed = vec![0_u8; mask.len().div_ceil(8)];
    for (index, &inside) in mask.iter().enumerate() {
        if inside {
            packed[index / 8] |= 1 << (index % 8);
        }
    }
    bytes.extend_from_slice(&packed);
    bytes
}

/// Loads a mask fixture, failing unless it is bound to exactly `surface`.
pub fn load_earth_mask(
    surface: &SphericalSurfaceSnapshot,
    path: &Path,
) -> Result<Vec<bool>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let surface_ref = SurfaceRef::for_spherical(surface);
    let header = FIXTURE_MAGIC.len() + 40;
    if bytes.len() < header || &bytes[..FIXTURE_MAGIC.len()] != FIXTURE_MAGIC {
        return Err(format!("{}: not an Earth mask fixture", path.display()));
    }
    let fingerprint = &bytes[FIXTURE_MAGIC.len()..FIXTURE_MAGIC.len() + 32];
    let cells = u32::from_le_bytes(bytes[header - 8..header - 4].try_into().unwrap());
    let edges = u32::from_le_bytes(bytes[header - 4..header].try_into().unwrap());
    if fingerprint != surface_ref.fingerprint()
        || cells != surface_ref.cell_count()
        || edges != surface_ref.edge_count()
    {
        return Err(format!("{}: SurfaceRef mismatch", path.display()));
    }
    let cell_count = cells as usize;
    if bytes.len() != header + cell_count.div_ceil(8) {
        return Err(format!("{}: truncated mask", path.display()));
    }
    Ok((0..cell_count)
        .map(|index| bytes[header + index / 8] & (1 << (index % 8)) != 0)
        .collect())
}
