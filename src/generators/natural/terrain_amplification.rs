//! T1 terrain amplification: deterministic, local, presentation-only
//! refinement of the published formation product.
//!
//! Implements the frozen contract in
//! `docs/superpowers/specs/2026-08-19-terrain-amplification-t1-design.md`:
//! geodesic triangle interpolation of T0 fields with smooth barycentric
//! weights, four blended surface regimes, the C1–C10 conditioning table, a
//! Hurst-derived octave ladder with Nyquist clamping, labeled-substream
//! seeding, and the Fibonacci probe fingerprint. River carving arrives with
//! plan Task 5 and is intentionally absent here.

use std::collections::BTreeMap;

use blake3::Hasher;
use rand::RngCore;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;
use thiserror::Error;

use super::fractal::FractalProfile;
use super::morphology::noise::{GaborKernel, SphericalNoise3d};
use crate::generators::spatial::{BASE_FACE_VERTICES, BASE_VERTEX_COMPONENTS};
use crate::world::natural::{
    formation_annual_precipitation_mm, GeologicSubstrateSnapshot, GlobalCirculationSnapshot,
    NaturalSurfaceFormationSnapshot, RiverSegment, SphericalOrogenyKind, SphericalTectonicSnapshot,
    SurfaceWaterField, SurfaceWaterKind, ELEVATION_MAX_M, ELEVATION_MIN_M,
    FORMATION_FLOODPLAIN_ACCOMMODATION_M,
};
use crate::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::{CellId, RootSeed};

/// Domain separator for every T1 substream (mirrors the natural-substream
/// discipline with a distinct domain so T0 streams can never collide).
const SUBSTREAM_DOMAIN: &[u8] = b"sekai-t1-amplification-v1\0";

/// Frozen substream labels from spec §8.
const WARP_LABEL: &str = "t1.warp";
const CONTINENTAL_DETAIL_LABEL: &str = "t1.continental-detail";
const DISSECTION_LABEL: &str = "t1.dissection";
const BADLANDS_LABEL: &str = "t1.badlands";
const ABYSSAL_HILLS_LABEL: &str = "t1.abyssal-hills";
const COAST_LABEL: &str = "t1.coast";

/// Maximum manual octaves held per layer in M1 (spec §6 caps M2 at 13).
const MAX_LAYER_OCTAVES: usize = 6;

/// C2: orogenic sharpness half-life, reusing the T0 precedent (80 Myr).
const OROGENY_HALF_LIFE_MYR: f64 = 80.0;
/// C10: Hurst-exponent blend range for the land interior.
const HURST_MOUNTAIN: f64 = 0.5;
const HURST_PLAIN: f64 = 0.8;
/// C1: local-relief normalization (metres of neighbour drop for full detail).
const RELIEF_REFERENCE_M: f64 = 900.0;
const RELIEF_FLOOR: f64 = 0.05;

// River carving (spec §7, amendment A4/A10). Hydraulic geometry after
// Leopold & Maddock 1953: width w = a·Q^0.5, depth d ∝ Q^0.4.
const RIVER_WIDTH_COEFFICIENT: f64 = 5.0;
const RIVER_DEPTH_COEFFICIENT: f64 = 2.0;
const RIVER_DEPTH_EXPONENT: f64 = 0.4;
const RIVER_DEPTH_MIN_M: f64 = 3.0;
const RIVER_DEPTH_MAX_M: f64 = 80.0;
const RIVER_WIDTH_MIN_M: f64 = 2.0;
const RIVER_WIDTH_MAX_M: f64 = 5_000.0;
/// V-shaped valley wall (~19°) in high relief.
const VALLEY_SLOPE_STEEP: f64 = 0.35;
/// Near-flat floodplain apron in low relief.
const VALLEY_SLOPE_FLOODPLAIN: f64 = 0.01;
/// C4: amplitude floor for the most erodible substrate.
const ERODIBILITY_AMPLITUDE_FLOOR: f64 = 0.4;
/// C5: Langbein–Schumm dissection peak and width (mm/yr equivalents).
const DISSECTION_PEAK_PRECIPITATION_MM: f64 = 350.0;
const DISSECTION_PRECIPITATION_WIDTH_MM: f64 = 250.0;
/// C7: sediment-blanket damping scale in metres of blanket thickness.
const SEDIMENT_DAMPING_M: f64 = 350.0;
/// Regime base amplitudes in metres (initial values; Task 4 calibrates).
pub(super) const LAND_BASE_AMPLITUDE_M: f64 = 320.0;
pub(super) const SHELF_BASE_AMPLITUDE_M: f64 = 35.0;
const OCEAN_BASE_AMPLITUDE_M: f64 = 90.0;
pub(super) const RIDGE_AMPLITUDE_M: f64 = 450.0;
const DISSECTION_AMPLITUDE_M: f64 = 140.0;
pub(super) const BADLANDS_AMPLITUDE_M: f64 = 60.0;
const COAST_DETAIL_AMPLITUDE_M: f64 = 25.0;
/// C9: warp displacement bounds as fractions of one T0 cell spacing.
const WARP_COAST_FRACTION: f64 = 0.55;
const WARP_OCEAN_FRACTION: f64 = 0.35;
const WARP_LAND_FRACTION: f64 = 0.15;
/// Shelf transition half-width in metres of depth around the shelf break.
pub(super) const SHELF_TRANSITION_M: f64 = 80.0;
/// Robust normalization percentile for erodibility and age gradients.
const NORMALIZATION_PERCENTILE: f64 = 0.95;
/// Probe count fixed by spec §8.
pub const PROBE_COUNT: usize = 256;

/// One amplified sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmplifiedSample {
    /// Amplified elevation in metres, inside the authoritative bounds.
    pub elevation_m: f32,
    /// The dominant blended regime at this sample.
    pub regime: SurfaceRegime,
}

/// The four blended surface regimes from spec §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRegime {
    /// Deep ocean floor below the shelf transition.
    OceanFloor,
    /// The preserved P5 shelf plateau.
    ContinentalShelf,
    /// The coastline band dominated by warp and shoreline detail.
    CoastalBand,
    /// Everything above, inland.
    LandInterior,
}

/// Added-octave level: L = 0 reproduces interpolated T0, each level adds one
/// octave with wavelength λ₀·2^(−L) (spec §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmplificationLod(u8);

impl AmplificationLod {
    /// Creates a level clamped to the M1 layer capacity.
    pub fn new(levels: u8) -> Self {
        Self(levels.min(MAX_LAYER_OCTAVES as u8))
    }

    /// Nyquist rule (spec §6): the deepest level whose shortest wavelength
    /// stays at or above twice the sampling footprint.
    pub fn for_sampling_footprint(base_wavelength_m: f64, footprint_m: f64) -> Self {
        let mut level = 0_u8;
        let mut wavelength = base_wavelength_m;
        while level < MAX_LAYER_OCTAVES as u8 && wavelength / 2.0 >= 2.0 * footprint_m {
            wavelength /= 2.0;
            level += 1;
        }
        Self(level)
    }

    /// Returns the number of added octaves.
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// Borrowed per-cell T0 fields consumed by the amplifier (spec §2).
///
/// Slices are copied at construction so the amplifier is self-contained,
/// `Send + Sync`, and usable from worker threads.
pub struct AmplificationFieldsView<'a> {
    /// Final P5 elevation per cell (metres).
    pub final_elevation_m: &'a [f32],
    /// Global sea level (metres).
    pub sea_level_m: f32,
    /// Retained sediment blanket per cell (metres).
    pub sediment_thickness_m: &'a [f32],
    /// Substrate erodibility per cell (unitless, non-negative).
    pub erodibility: &'a [f32],
    /// Annual precipitation per cell (mm/yr).
    pub annual_precipitation_mm: &'a [f32],
    /// Oceanic crust age per cell (Myr; continental cells carry their value).
    pub crust_age_myr: &'a [f32],
    /// Tectonic grain east component per cell.
    pub lineation_east: &'a [f32],
    /// Tectonic grain north component per cell.
    pub lineation_north: &'a [f32],
    /// Orogeny classification per cell.
    pub orogeny_kind: &'a [SphericalOrogenyKind],
    /// Orogeny age per cell (Myr).
    pub orogeny_age_myr: &'a [f32],
}

/// Final sibling snapshots required to construct T1 without copying bundle state.
#[derive(Debug, Clone, Copy)]
pub struct FormationDerivationInputs<'a> {
    pub surface: &'a SphericalSurfaceSnapshot,
    pub compatibility: &'a SphericalTectonicSnapshot,
    pub substrate: &'a GeologicSubstrateSnapshot,
    pub formation: &'a NaturalSurfaceFormationSnapshot,
    pub climate: &'a GlobalCirculationSnapshot,
}

/// Errors returned while constructing the amplifier.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TerrainAmplificationError {
    /// A field's cardinality does not match the surface.
    #[error("field {field} has {found} entries but the surface has {expected} cells")]
    FieldCardinality {
        /// The mismatched field name.
        field: &'static str,
        /// Entries found.
        found: usize,
        /// Cells expected.
        expected: usize,
    },
    /// A river segment references a cell outside the surface.
    #[error("river segment touches cell {cell} but the surface has {cell_count} cells")]
    RiverSegmentOutOfRange {
        /// The offending cell index.
        cell: u32,
        /// Cells available.
        cell_count: usize,
    },
    /// A river segment does not cross one authoritative shared surface edge.
    #[error("river segment from cell {from:?} to {to:?} does not join adjacent cells")]
    RiverSegmentNotAdjacent {
        /// Published upstream cell.
        from: CellId,
        /// Published downstream cell.
        to: CellId,
    },
    /// The published river network contains a cycle.
    #[error("the river network contains a cycle")]
    RiverNetworkCycle,
    /// The cell count is not a class-I geodesic count (10·f²+2).
    #[error("cell count {cell_count} is not a geodesic 10·f²+2 lattice")]
    NotGeodesic {
        /// The offending cell count.
        cell_count: usize,
    },
    /// The locator table disagreed with the authoritative adjacency.
    #[error("geodesic locator validation failed: {reason}")]
    LocatorValidation {
        /// A short description of the failed check.
        reason: &'static str,
    },
}

struct FaceBasis {
    inverse: [[f64; 3]; 3],
}

struct GeodesicLocator {
    frequency: usize,
    faces: Vec<FaceBasis>,
    /// Per face, row-major over (i, j) with i + j ≤ f: the lattice cell id.
    table: Vec<u32>,
}

struct NoiseLayer {
    octaves: Vec<SphericalNoise3d>,
}

impl NoiseLayer {
    fn from_label(root: &[u8; 32], label: &str, count: usize) -> Self {
        let mut rng = substream(root, label);
        Self {
            octaves: (0..count)
                .map(|_| SphericalNoise3d::new(rng.next_u32()))
                .collect(),
        }
    }
}

/// The T1 amplifier: constructed once per published world, sampled anywhere.
pub struct TerrainAmplifier {
    radius_m: f64,
    cell_spacing_m: f64,
    base_wavelength_m: f64,
    sea_level_m: f64,
    locator: GeodesicLocator,
    // Owned per-cell fields (copied so the amplifier is self-contained).
    elevation_m: Vec<f32>,
    sediment_norm: Vec<f32>,
    erodibility_norm: Vec<f32>,
    precipitation_mm: Vec<f32>,
    lineation_east: Vec<f32>,
    lineation_north: Vec<f32>,
    orogeny_factor: Vec<f32>,
    local_relief_m: Vec<f32>,
    local_relief_norm: Vec<f32>,
    age_gradient_norm: Vec<f32>,
    land: Vec<f32>,
    // River carving tables (spec §7): empty when no network is attached.
    reaches: Vec<RiverReach>,
    reach_offsets: Vec<u32>,
    reach_indices: Vec<u32>,
    // Noise layers, one instance per manual octave.
    warp: NoiseLayer,
    continental: NoiseLayer,
    dissection: NoiseLayer,
    badlands: NoiseLayer,
    #[allow(dead_code)] // Activated at LODs whose Nyquist admits the 2–10 km band (M2).
    abyssal_hills: NoiseLayer,
    coast: NoiseLayer,
}

impl TerrainAmplifier {
    /// Builds the amplifier from one validated surface and its T0 fields.
    pub fn new(
        surface: &SphericalSurfaceSnapshot,
        fields: AmplificationFieldsView<'_>,
        root_seed: RootSeed,
    ) -> Result<Self, TerrainAmplificationError> {
        let cell_count = surface.cells().len();
        check_cardinality(
            "final_elevation_m",
            fields.final_elevation_m.len(),
            cell_count,
        )?;
        check_cardinality(
            "sediment_thickness_m",
            fields.sediment_thickness_m.len(),
            cell_count,
        )?;
        check_cardinality("erodibility", fields.erodibility.len(), cell_count)?;
        check_cardinality(
            "annual_precipitation_mm",
            fields.annual_precipitation_mm.len(),
            cell_count,
        )?;
        check_cardinality("crust_age_myr", fields.crust_age_myr.len(), cell_count)?;
        check_cardinality("lineation_east", fields.lineation_east.len(), cell_count)?;
        check_cardinality("lineation_north", fields.lineation_north.len(), cell_count)?;
        check_cardinality("orogeny_kind", fields.orogeny_kind.len(), cell_count)?;
        check_cardinality("orogeny_age_myr", fields.orogeny_age_myr.len(), cell_count)?;

        let radius_m = surface.radius().get();
        let cell_spacing_m = (surface.total_cell_area().get() / cell_count as f64).sqrt();
        let base_wavelength_m = 2.0 * cell_spacing_m;
        let locator = GeodesicLocator::build(surface)?;

        // Neighbour lists for the derived per-cell factors.
        let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); cell_count];
        for edge in surface.edges() {
            let [a, b] = edge.cells;
            neighbours[a.raw() as usize].push(b.raw());
            neighbours[b.raw() as usize].push(a.raw());
        }

        // C1 driver: local relief as the largest neighbour elevation drop,
        // kept both in metres (the v2 spectral-continuation amplitude
        // source, amendment A7) and normalized (the Hurst/wall channels).
        // The norm divides the unrounded f64 relief so the M1 bit path
        // stays exactly as frozen.
        let local_relief_f64: Vec<f64> = (0..cell_count)
            .map(|index| {
                let here = f64::from(fields.final_elevation_m[index]);
                neighbours[index]
                    .iter()
                    .map(|&n| (here - f64::from(fields.final_elevation_m[n as usize])).abs())
                    .fold(0.0_f64, f64::max)
            })
            .collect();
        let local_relief_m: Vec<f32> = local_relief_f64.iter().map(|&r| r as f32).collect();
        let local_relief_norm: Vec<f32> = local_relief_f64
            .iter()
            .map(|&relief| ((relief / RELIEF_REFERENCE_M).clamp(RELIEF_FLOOR, 1.0)) as f32)
            .collect();

        // C8 driver: crust-age gradient magnitude as the spreading-rate proxy.
        let age_gradient: Vec<f64> = (0..cell_count)
            .map(|index| {
                let here = f64::from(fields.crust_age_myr[index]);
                neighbours[index]
                    .iter()
                    .map(|&n| (here - f64::from(fields.crust_age_myr[n as usize])).abs())
                    .fold(0.0_f64, f64::max)
            })
            .collect();
        let age_reference = robust_reference(&age_gradient);
        let age_gradient_norm: Vec<f32> = age_gradient
            .iter()
            .map(|&g| ((g / age_reference).clamp(0.0, 1.0)) as f32)
            .collect();

        // C4 driver normalization against the world's own robust reference.
        let erodibility_values: Vec<f64> =
            fields.erodibility.iter().map(|&e| f64::from(e)).collect();
        let erodibility_reference = robust_reference(&erodibility_values);
        let erodibility_norm: Vec<f32> = erodibility_values
            .iter()
            .map(|&e| ((e / erodibility_reference).clamp(0.0, 1.0)) as f32)
            .collect();

        // C7 driver: sediment blanket relative to the damping scale.
        let sediment_norm: Vec<f32> = fields
            .sediment_thickness_m
            .iter()
            .map(|&t| (f64::from(t) / SEDIMENT_DAMPING_M) as f32)
            .collect();

        // C2 driver: orogenic sharpness with the frozen 80 Myr half-life.
        let orogeny_factor: Vec<f32> = (0..cell_count)
            .map(|index| match fields.orogeny_kind[index] {
                SphericalOrogenyKind::None => 0.0,
                _ => {
                    (0.5_f64.powf(f64::from(fields.orogeny_age_myr[index]) / OROGENY_HALF_LIFE_MYR))
                        as f32
                }
            })
            .collect();

        let land: Vec<f32> = fields
            .final_elevation_m
            .iter()
            .map(|&e| if e >= fields.sea_level_m { 1.0 } else { 0.0 })
            .collect();

        let root = substream_root(root_seed);
        Ok(Self {
            radius_m,
            cell_spacing_m,
            base_wavelength_m,
            sea_level_m: f64::from(fields.sea_level_m),
            locator,
            elevation_m: fields.final_elevation_m.to_vec(),
            sediment_norm,
            erodibility_norm,
            precipitation_mm: fields.annual_precipitation_mm.to_vec(),
            lineation_east: fields.lineation_east.to_vec(),
            lineation_north: fields.lineation_north.to_vec(),
            orogeny_factor,
            local_relief_m,
            local_relief_norm,
            age_gradient_norm,
            land,
            reaches: Vec::new(),
            reach_offsets: Vec::new(),
            reach_indices: Vec::new(),
            warp: NoiseLayer::from_label(&root, WARP_LABEL, 4),
            continental: NoiseLayer::from_label(&root, CONTINENTAL_DETAIL_LABEL, MAX_LAYER_OCTAVES),
            dissection: NoiseLayer::from_label(&root, DISSECTION_LABEL, MAX_LAYER_OCTAVES),
            badlands: NoiseLayer::from_label(&root, BADLANDS_LABEL, 2),
            abyssal_hills: NoiseLayer::from_label(&root, ABYSSAL_HILLS_LABEL, 2),
            coast: NoiseLayer::from_label(&root, COAST_LABEL, 2),
        })
    }

    /// Assembles the amplifier straight from the published formation product.
    pub fn from_formation_product(
        inputs: FormationDerivationInputs<'_>,
        root_seed: RootSeed,
    ) -> Result<Self, TerrainAmplificationError> {
        let FormationDerivationInputs {
            surface,
            compatibility,
            substrate,
            formation,
            climate,
        } = inputs;
        let terrain = formation.terrain_fields();
        let monthly = climate.fields().monthly_precipitation_mm_day().values();
        let annual_precipitation_mm: Vec<f32> = monthly
            .iter()
            .map(formation_annual_precipitation_mm)
            .collect();
        Self::new(
            surface,
            AmplificationFieldsView {
                final_elevation_m: terrain.current_elevation_m(),
                sea_level_m: terrain.sea_level_m(),
                sediment_thickness_m: terrain.sediment().sediment_thickness_m(),
                erodibility: substrate.erodibility(),
                annual_precipitation_mm: &annual_precipitation_mm,
                crust_age_myr: compatibility.crust_age_myr(),
                lineation_east: compatibility.lineation_east(),
                lineation_north: compatibility.lineation_north(),
                orogeny_kind: compatibility.orogeny_kind(),
                orogeny_age_myr: compatibility.orogeny_age_myr(),
            },
            root_seed,
        )?
        .with_rivers(
            surface,
            formation.hydrology().river_segments(),
            formation.hydrology().surface_water(),
        )
    }

    /// Attaches the published river network for §7 carving (amendment A4).
    ///
    /// Node beds are the T0 elevations minus a hydraulic-geometry incision
    /// depth, made monotone along flow by a running minimum in topological
    /// order; reaches interpolate those beds linearly, so a carved bed can
    /// never rise downstream, and carving as a whole only ever lowers the
    /// surface through `min` — it can never dam.
    pub fn with_rivers(
        mut self,
        surface: &SphericalSurfaceSnapshot,
        segments: &[RiverSegment],
        surface_water: &SurfaceWaterField,
    ) -> Result<Self, TerrainAmplificationError> {
        if segments.is_empty() {
            return Ok(self);
        }
        let cells = surface.cells();
        let cell_count = cells.len();
        check_cardinality("surface_water", surface_water.len(), cell_count)?;
        let mut depth_m: BTreeMap<u32, f64> = BTreeMap::new();
        let mut upstream_count: BTreeMap<u32, u32> = BTreeMap::new();
        for segment in segments {
            for node in [segment.from().raw(), segment.to().raw()] {
                if node as usize >= cell_count {
                    return Err(TerrainAmplificationError::RiverSegmentOutOfRange {
                        cell: node,
                        cell_count,
                    });
                }
            }
            let discharge = f64::from(segment.mean_discharge_m3_s()).max(0.0);
            let depth = (RIVER_DEPTH_COEFFICIENT * discharge.powf(RIVER_DEPTH_EXPONENT))
                .clamp(RIVER_DEPTH_MIN_M, RIVER_DEPTH_MAX_M);
            for node in [segment.from().raw(), segment.to().raw()] {
                let slot = depth_m.entry(node).or_insert(0.0);
                *slot = slot.max(depth);
            }
            *upstream_count.entry(segment.to().raw()).or_insert(0) += 1;
            upstream_count.entry(segment.from().raw()).or_insert(0);
        }
        let mut bed_m: BTreeMap<u32, f64> = depth_m
            .iter()
            .map(|(&node, &depth)| (node, f64::from(self.elevation_m[node as usize]) - depth))
            .collect();
        let mut outgoing: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (index, segment) in segments.iter().enumerate() {
            outgoing
                .entry(segment.from().raw())
                .or_default()
                .push(index);
        }
        let mut queue: Vec<u32> = upstream_count
            .iter()
            .filter(|(_, &count)| count == 0)
            .map(|(&node, _)| node)
            .collect();
        let mut remaining = upstream_count.clone();
        let mut head = 0;
        while head < queue.len() {
            let node = queue[head];
            head += 1;
            let node_bed = bed_m[&node];
            if let Some(list) = outgoing.get(&node) {
                for &segment_index in list {
                    let to = segments[segment_index].to().raw();
                    let entry = bed_m
                        .get_mut(&to)
                        .expect("every segment endpoint has a bed entry");
                    *entry = entry.min(node_bed);
                    let pending = remaining
                        .get_mut(&to)
                        .expect("every segment endpoint has an upstream count");
                    *pending -= 1;
                    if *pending == 0 {
                        queue.push(to);
                    }
                }
            }
        }
        if head != upstream_count.len() {
            return Err(TerrainAmplificationError::RiverNetworkCycle);
        }

        let mut reaches = Vec::with_capacity(segments.len());
        let mut touching: Vec<Vec<u32>> = vec![Vec::new(); cell_count];
        for segment in segments {
            let from = segment.from().raw();
            let to = segment.to().raw();
            let discharge = f64::from(segment.mean_discharge_m3_s()).max(0.0);
            let width_m = (RIVER_WIDTH_COEFFICIENT * discharge.sqrt())
                .clamp(RIVER_WIDTH_MIN_M, RIVER_WIDTH_MAX_M);
            let bed_from = bed_m[&from];
            let from_id = segment.from();
            let to_id = segment.to();
            let shared_edge_id = cells[from as usize]
                .boundary_edges
                .iter()
                .copied()
                .find(|edge| cells[to as usize].boundary_edges.contains(edge))
                .ok_or(TerrainAmplificationError::RiverSegmentNotAdjacent {
                    from: from_id,
                    to: to_id,
                })?;
            let shared_edge = surface.edge(shared_edge_id).ok_or(
                TerrainAmplificationError::RiverSegmentNotAdjacent {
                    from: from_id,
                    to: to_id,
                },
            )?;
            if !shared_edge.cells.contains(&from_id) || !shared_edge.cells.contains(&to_id) {
                return Err(TerrainAmplificationError::RiverSegmentNotAdjacent {
                    from: from_id,
                    to: to_id,
                });
            }
            let from_center = cells[from as usize].centroid.components();
            let portal = shared_edge.midpoint.components();
            let to_center = cells[to as usize].centroid.components();
            let upstream_length = arc_angle(from_center, portal);
            let downstream_length = arc_angle(portal, to_center);
            let total_length = upstream_length + downstream_length;
            let portal_fraction = if total_length > 0.0 {
                upstream_length / total_length
            } else {
                0.5
            };
            let boundary = shared_edge.vertices.map(|vertex| {
                surface.vertices()[vertex.raw() as usize]
                    .position
                    .components()
            });
            let from_water = surface_water
                .get(from as usize)
                .expect("surface-water cardinality was validated");
            let to_water = surface_water
                .get(to as usize)
                .expect("surface-water cardinality was validated");
            let legs = [
                (from_water == SurfaceWaterKind::DryLand).then_some(RiverLeg {
                    from: from_center,
                    to: portal,
                    sector: [from_center, boundary[0], boundary[1]],
                    fraction_from: 0.0,
                    fraction_to: portal_fraction,
                }),
                (to_water == SurfaceWaterKind::DryLand).then_some(RiverLeg {
                    from: portal,
                    to: to_center,
                    sector: [to_center, boundary[0], boundary[1]],
                    fraction_from: portal_fraction,
                    fraction_to: 1.0,
                }),
            ];
            let index = u32::try_from(reaches.len()).map_err(|_| {
                TerrainAmplificationError::RiverSegmentOutOfRange {
                    cell: from,
                    cell_count,
                }
            })?;
            reaches.push(RiverReach {
                bed_from_m: bed_from,
                bed_to_m: bed_m[&to].min(bed_from),
                width_m,
                legs,
            });
            if from_water == SurfaceWaterKind::DryLand {
                touching[from as usize].push(index);
            }
            if to_water == SurfaceWaterKind::DryLand {
                touching[to as usize].push(index);
            }
        }
        let mut reach_offsets = Vec::with_capacity(cell_count + 1);
        let mut reach_indices = Vec::new();
        reach_offsets.push(0_u32);
        for list in &touching {
            reach_indices.extend_from_slice(list);
            let offset = u32::try_from(reach_indices.len())
                .map_err(|_| TerrainAmplificationError::RiverNetworkCycle)?;
            reach_offsets.push(offset);
        }
        self.reaches = reaches;
        self.reach_offsets = reach_offsets;
        self.reach_indices = reach_indices;
        Ok(self)
    }

    /// Returns the §7 valley carve elevation at one authoritative position.
    ///
    /// Valley walls blend from floodplain aprons to V-shaped slopes as the
    /// local relief crosses the floodplain accommodation band; the chord
    /// approximation to the reach arc is exact to well under a metre at
    /// cell-spacing scales.
    pub(super) fn river_carve_m(
        &self,
        position: UnitVector3,
        local_relief_norm: f64,
    ) -> Option<f64> {
        let (corners, _) = self.locator.locate(position);
        let p = position.components();
        let wall_slope = Self::carve_wall_slope(local_relief_norm);
        let mut carve: Option<f64> = None;
        for &cell in &corners {
            let start = self.reach_offsets[cell as usize] as usize;
            let end = self.reach_offsets[cell as usize + 1] as usize;
            for &reach_index in &self.reach_indices[start..end] {
                let reach = &self.reaches[reach_index as usize];
                for leg in reach.legs.iter().flatten() {
                    if !spherical_triangle_contains(p, leg.sector) {
                        continue;
                    }
                    let Some((lateral_m, along)) =
                        arc_nearest_point(leg.from, leg.to, p, self.radius_m)
                    else {
                        continue;
                    };
                    let fraction =
                        leg.fraction_from + along * (leg.fraction_to - leg.fraction_from);
                    let bed = reach.bed_from_m + fraction * (reach.bed_to_m - reach.bed_from_m);
                    let value = bed + (lateral_m - reach.half_width_m()).max(0.0) * wall_slope;
                    carve = Some(carve.map_or(value, |current: f64| current.min(value)));
                }
            }
        }
        carve
    }

    /// The base detail wavelength λ₀ = 2 × mean cell spacing (spec §6).
    pub fn base_wavelength_m(&self) -> f64 {
        self.base_wavelength_m
    }

    /// The sphere radius the amplifier was built for, in metres.
    pub fn radius_m(&self) -> f64 {
        self.radius_m
    }

    /// Borrowed per-cell conditioning drivers for the hierarchical engine.
    ///
    /// The T1 v2 hierarchical derivation reuses this amplifier's normalized
    /// C-table driver fields as its single fact source instead of
    /// re-deriving them.
    pub(super) fn conditioning(&self) -> ConditioningView<'_> {
        ConditioningView {
            elevation_m: &self.elevation_m,
            sea_level_m: self.sea_level_m,
            local_relief_m: &self.local_relief_m,
            local_relief_norm: &self.local_relief_norm,
            erodibility_norm: &self.erodibility_norm,
            sediment_norm: &self.sediment_norm,
            precipitation_mm: &self.precipitation_mm,
            orogeny_factor: &self.orogeny_factor,
            age_gradient_norm: &self.age_gradient_norm,
        }
    }

    /// Whether a published river network is attached for §7 carving.
    pub(super) fn has_rivers(&self) -> bool {
        !self.reaches.is_empty()
    }

    /// The carve-ready reaches in published segment order (index-aligned
    /// with the P5 river segment list).
    pub(super) fn river_reaches(&self) -> &[RiverReach] {
        &self.reaches
    }

    /// The per-cell CSR lists of reach indices touching each cell.
    pub(super) fn reach_lists(&self) -> (&[u32], &[u32]) {
        (&self.reach_offsets, &self.reach_indices)
    }

    /// The §7 valley wall slope blended by local relief (amendment A4).
    pub(super) fn carve_wall_slope(local_relief_norm: f64) -> f64 {
        VALLEY_SLOPE_FLOODPLAIN
            + (VALLEY_SLOPE_STEEP - VALLEY_SLOPE_FLOODPLAIN)
                * smoothstep(
                    0.0,
                    2.0 * FORMATION_FLOODPLAIN_ACCOMMODATION_M / RELIEF_REFERENCE_M,
                    local_relief_norm,
                )
    }

    /// The three lattice cells whose dual triangle contains `position`.
    pub(super) fn locate_corner_cells(&self, position: UnitVector3) -> [u32; 3] {
        self.locator.locate(position).0
    }

    /// Evaluates the amplified surface at one direction (spec §1).
    ///
    /// LOD 0 reproduces the interpolated T0 surface exactly: no warp and no
    /// added detail, so it doubles as the unwarped baseline for invariants.
    pub fn sample(&self, position: UnitVector3, lod: AmplificationLod) -> AmplifiedSample {
        // Phase 1: unwarped interpolation to derive the warp magnitude.
        let raw = self.interpolate(position);
        if lod.levels() == 0 {
            let elevation = raw
                .elevation_m
                .clamp(f64::from(ELEVATION_MIN_M), f64::from(ELEVATION_MAX_M));
            return AmplifiedSample {
                elevation_m: elevation as f32,
                regime: raw.weights.dominant(),
            };
        }
        let warp_fraction = WARP_LAND_FRACTION
            + (WARP_OCEAN_FRACTION - WARP_LAND_FRACTION) * raw.weights.ocean_like()
            + (WARP_COAST_FRACTION - WARP_LAND_FRACTION) * raw.weights.coast;
        let warped = self.warp(position, warp_fraction * self.cell_spacing_m);

        // Phase 2: everything else at the warped position.
        let interp = self.interpolate(warped);
        let weights = interp.weights;
        let detail = self.detail_m(warped, &interp, lod);
        let mut elevation = (interp.elevation_m + detail)
            .clamp(f64::from(ELEVATION_MIN_M), f64::from(ELEVATION_MAX_M));
        // Phase 3 (spec §7, amendment A4): river carving on the
        // authoritative, unwarped geometry so valleys align with the
        // published network. Carving only lowers (min): it can never dam.
        if !self.reaches.is_empty() {
            if let Some(carve) = self.river_carve_m(position, raw.local_relief) {
                elevation = elevation.min(carve.max(f64::from(ELEVATION_MIN_M)));
            }
        }
        AmplifiedSample {
            elevation_m: elevation as f32,
            regime: weights.dominant(),
        }
    }

    /// Blake3 fingerprint over the frozen Fibonacci probe set (spec §8).
    pub fn probe_fingerprint(&self, lod: AmplificationLod) -> [u8; 32] {
        let mut hasher = Hasher::new();
        for index in 0..PROBE_COUNT {
            let probe = fibonacci_probe(index, PROBE_COUNT);
            hasher.update(&self.sample(probe, lod).elevation_m.to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }

    fn warp(&self, position: UnitVector3, magnitude_m: f64) -> UnitVector3 {
        let coarse = self.octave_profile(0, 4.0 * self.cell_spacing_m);
        let fine = self.octave_profile(1, 4.0 * self.cell_spacing_m);
        let a = self.warp.octaves[0].fbm(position, coarse)
            + 0.5 * self.warp.octaves[1].fbm(position, fine);
        let b = self.warp.octaves[2].fbm(position, coarse)
            + 0.5 * self.warp.octaves[3].fbm(position, fine);
        let angular = magnitude_m / self.radius_m;
        let (east, north) = canonical_east_north_basis(position);
        let p = position.components();
        let displaced = [
            p[0] + angular * (a * east[0] + b * north[0]),
            p[1] + angular * (a * east[1] + b * north[1]),
            p[2] + angular * (a * east[2] + b * north[2]),
        ];
        normalize_direction(displaced)
    }

    fn octave_profile(&self, level: usize, base_wavelength_m: f64) -> FractalProfile {
        let wavelength = base_wavelength_m / 2.0_f64.powi(level as i32);
        FractalProfile {
            octaves: 1,
            frequency: self.radius_m / wavelength,
            lacunarity: 2.0,
            persistence: 0.5,
        }
    }

    fn detail_m(&self, position: UnitVector3, interp: &Interpolated, lod: AmplificationLod) -> f64 {
        if lod.levels() == 0 {
            return 0.0;
        }
        let w = &interp.weights;

        // C10: spatially blended Hurst exponent -> per-octave persistence.
        let roughness = interp.local_relief.max(interp.orogeny_factor);
        let hurst = surface_roughness_hurst(roughness);
        let persistence = 2.0_f64.powf(-hurst);

        // C1, C4 (amplitude channel), C7: the land amplitude envelope.
        let amplitude_cap = erodibility_amplitude(interp.erodibility);
        let damping = sediment_damping(interp.sediment);
        let land_amplitude = LAND_BASE_AMPLITUDE_M * interp.local_relief * amplitude_cap * damping;
        let ocean_amplitude = OCEAN_BASE_AMPLITUDE_M * (0.3 + 0.7 * interp.age_gradient) * damping;
        let amplitude = land_amplitude * w.land
            + SHELF_BASE_AMPLITUDE_M * w.shelf
            + ocean_amplitude * w.ocean
            + COAST_DETAIL_AMPLITUDE_M * w.coast;

        // Base fBm ladder with the Nyquist-clamped octave count (spec §6).
        let levels = usize::from(lod.levels()).min(MAX_LAYER_OCTAVES);
        let mut base = 0.0;
        let mut norm = 0.0;
        let mut gain = 1.0;
        for level in 0..levels {
            let profile = self.octave_profile(level, self.base_wavelength_m / 2.0);
            base += gain * self.continental.octaves[level].fbm(position, profile);
            norm += gain;
            gain *= persistence;
        }
        let mut detail = amplitude * (base / norm.max(f64::MIN_POSITIVE));

        // C2 + C3: anisotropic orogenic ridging along the tectonic grain.
        if interp.orogeny_factor > 0.0 && w.land > 0.0 {
            let ridged_profile = self.octave_profile(1, self.base_wavelength_m / 2.0);
            let ridge =
                self.continental.octaves[MAX_LAYER_OCTAVES - 1].ridged(position, ridged_profile);
            let anisotropy = self.grain_alignment(position, interp);
            detail += RIDGE_AMPLITUDE_M
                * interp.orogeny_factor
                * amplitude_cap
                * damping
                * w.land
                * ridge
                * anisotropy;
        }

        // C5 (+C4 texture channel, C6 gate): dissection carves downward only.
        let precipitation_factor = langbein_schumm(interp.precipitation_mm);
        if precipitation_factor > 0.0 && levels >= 2 && w.land > 0.0 {
            let texture_level = 1 + usize::from(interp.erodibility > 0.5);
            let dissect_profile =
                self.octave_profile(texture_level.min(levels - 1), self.base_wavelength_m / 2.0);
            let valleys = self.dissection.octaves[texture_level.min(MAX_LAYER_OCTAVES - 1)]
                .ridged(position, dissect_profile);
            detail -= DISSECTION_AMPLITUDE_M
                * precipitation_factor
                * interp.local_relief
                * w.land
                * valleys;

            let badlands = badlands_gate(interp.erodibility, precipitation_factor);
            if badlands > 0.0 {
                let badlands_profile =
                    self.octave_profile((levels - 1).min(3), self.base_wavelength_m / 4.0);
                detail -= BADLANDS_AMPLITUDE_M
                    * badlands
                    * w.land
                    * self.badlands.octaves[0].ridged(position, badlands_profile);
            }
        }

        // Shoreline micro-detail on the coast band (C9 companion texture).
        if w.coast > 0.0 {
            let coast_profile = self.octave_profile(levels - 1, self.base_wavelength_m / 2.0);
            detail += COAST_DETAIL_AMPLITUDE_M
                * w.coast
                * self.coast.octaves[1].fbm(position, coast_profile);
        }

        detail
    }

    /// C3: |cos| alignment between the sample's grain and the local east/north
    /// tangent expression of the interpolated lineation.
    fn grain_alignment(&self, position: UnitVector3, interp: &Interpolated) -> f64 {
        let east_north = (interp.lineation_east, interp.lineation_north);
        let magnitude = (east_north.0 * east_north.0 + east_north.1 * east_north.1).sqrt();
        if magnitude <= f64::EPSILON {
            return 0.35; // isotropic fallback keeps some ridging without grain
        }
        let (east, north) = canonical_east_north_basis(position);
        let tangent = [
            east[0] * east_north.0 + north[0] * east_north.1,
            east[1] * east_north.0 + north[1] * east_north.1,
            east[2] * east_north.0 + north[2] * east_north.1,
        ];
        let gabor = self.continental.octaves[0].sparse_gabor(
            position,
            tangent,
            GaborKernel {
                envelope_scale_rad: (4.0 * self.cell_spacing_m / self.radius_m).clamp(0.02, 0.9),
                carrier_frequency: (self.radius_m / (self.base_wavelength_m / 2.0))
                    .clamp(0.8, 512.0),
                impulse_count: 48,
            },
        );
        0.35 + 0.65 * (gabor + 1.0) * 0.5
    }

    fn interpolate(&self, position: UnitVector3) -> Interpolated {
        let (corners, weights) = self.locator.locate(position);
        let w = smooth_weights(weights);
        let value = |field: &[f32]| -> f64 {
            (0..3)
                .map(|k| w[k] * f64::from(field[corners[k] as usize]))
                .sum()
        };
        let elevation_m = value(&self.elevation_m);
        let depth = elevation_m - self.sea_level_m;
        let land_fraction = value(&self.land);
        let coast = 1.0 - smoothstep(0.0, 0.5, (land_fraction - 0.5).abs());
        let below = 1.0 - smoothstep(-SHELF_TRANSITION_M, SHELF_TRANSITION_M, depth);
        let shelf_break = crate::world::natural::FORMATION_SHELF_BREAK_DEPTH_M;
        let ocean = smoothstep(
            -(shelf_break + SHELF_TRANSITION_M),
            -(shelf_break - SHELF_TRANSITION_M),
            -depth,
        );
        let ocean_w = below * ocean * (1.0 - coast);
        let shelf_w = below * (1.0 - ocean) * (1.0 - coast);
        let land_w = (1.0 - below) * (1.0 - coast);
        let weights = RegimeWeights {
            ocean: ocean_w,
            shelf: shelf_w,
            coast,
            land: land_w,
        };
        Interpolated {
            elevation_m,
            local_relief: value(&self.local_relief_norm),
            erodibility: value(&self.erodibility_norm),
            sediment: value(&self.sediment_norm),
            precipitation_mm: value(&self.precipitation_mm),
            orogeny_factor: value(&self.orogeny_factor),
            age_gradient: value(&self.age_gradient_norm),
            lineation_east: value(&self.lineation_east),
            lineation_north: value(&self.lineation_north),
            weights,
        }
    }
}

/// Borrowed per-cell T0 conditioning drivers shared with the hierarchical
/// derivation engine (normalized once at amplifier construction).
pub(super) struct ConditioningView<'a> {
    pub(super) elevation_m: &'a [f32],
    pub(super) sea_level_m: f64,
    pub(super) local_relief_m: &'a [f32],
    pub(super) local_relief_norm: &'a [f32],
    pub(super) erodibility_norm: &'a [f32],
    pub(super) sediment_norm: &'a [f32],
    pub(super) precipitation_mm: &'a [f32],
    pub(super) orogeny_factor: &'a [f32],
    pub(super) age_gradient_norm: &'a [f32],
}

/// One carve-ready river reach with monotone interpolated bed ends.
///
/// Shared with the hierarchical river rerouting as its L0 source of
/// truth (geometry, beds, and hydraulic width all come from here).
#[derive(Debug, Clone)]
pub(super) struct RiverReach {
    pub(super) bed_from_m: f64,
    pub(super) bed_to_m: f64,
    pub(super) width_m: f64,
    pub(super) legs: [Option<RiverLeg>; 2],
}

impl RiverReach {
    pub(super) fn half_width_m(&self) -> f64 {
        self.width_m * 0.5
    }
}

/// One directed dry-land portion of a published reach, bounded by the
/// owning cell centroid and the authoritative shared-edge portal.
#[derive(Debug, Clone, Copy)]
pub(super) struct RiverLeg {
    pub(super) from: [f64; 3],
    pub(super) to: [f64; 3],
    pub(super) sector: [[f64; 3]; 3],
    pub(super) fraction_from: f64,
    pub(super) fraction_to: f64,
}

struct Interpolated {
    elevation_m: f64,
    local_relief: f64,
    erodibility: f64,
    sediment: f64,
    precipitation_mm: f64,
    orogeny_factor: f64,
    age_gradient: f64,
    lineation_east: f64,
    lineation_north: f64,
    weights: RegimeWeights,
}

#[derive(Debug, Clone, Copy)]
struct RegimeWeights {
    ocean: f64,
    shelf: f64,
    coast: f64,
    land: f64,
}

impl RegimeWeights {
    fn ocean_like(&self) -> f64 {
        self.ocean + self.shelf
    }

    fn dominant(&self) -> SurfaceRegime {
        let candidates = [
            (self.ocean, SurfaceRegime::OceanFloor),
            (self.shelf, SurfaceRegime::ContinentalShelf),
            (self.coast, SurfaceRegime::CoastalBand),
            (self.land, SurfaceRegime::LandInterior),
        ];
        candidates
            .into_iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, regime)| regime)
            .expect("four candidates are never empty")
    }
}

impl GeodesicLocator {
    fn build(surface: &SphericalSurfaceSnapshot) -> Result<Self, TerrainAmplificationError> {
        let cell_count = surface.cells().len();
        let frequency_sq = (cell_count.saturating_sub(2)) as f64 / 10.0;
        let frequency = frequency_sq.sqrt().round() as usize;
        if frequency < 1 || 10 * frequency * frequency + 2 != cell_count {
            return Err(TerrainAmplificationError::NotGeodesic { cell_count });
        }

        let vertices: Vec<[f64; 3]> = BASE_VERTEX_COMPONENTS
            .iter()
            .map(|&v| {
                let norm = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                [v[0] / norm, v[1] / norm, v[2] / norm]
            })
            .collect();
        let faces: Vec<FaceBasis> = BASE_FACE_VERTICES
            .iter()
            .map(|&[a, b, c]| {
                let corners = [
                    vertices[a as usize],
                    vertices[b as usize],
                    vertices[c as usize],
                ];
                FaceBasis {
                    inverse: invert_3x3(corners),
                }
            })
            .collect();

        let stride = (frequency + 1) * (frequency + 2) / 2;
        let mut table = vec![u32::MAX; faces.len() * stride];
        for (cell_index, cell) in surface.cells().iter().enumerate() {
            let direction = cell.centroid.components();
            for (face_index, face) in faces.iter().enumerate() {
                let bary = barycentric(face, direction);
                if bary.iter().all(|&w| w >= -1.0e-9) {
                    let i = (bary[1] * frequency as f64).round() as usize;
                    let j = (bary[2] * frequency as f64).round() as usize;
                    if i + j <= frequency {
                        let slot = face_index * stride + lattice_index(frequency, i, j);
                        if table[slot] != u32::MAX && table[slot] != cell_index as u32 {
                            return Err(TerrainAmplificationError::LocatorValidation {
                                reason: "two cells rounded onto one lattice slot",
                            });
                        }
                        table[slot] = cell_index as u32;
                    }
                }
            }
        }
        if table.contains(&u32::MAX) {
            return Err(TerrainAmplificationError::LocatorValidation {
                reason: "a lattice slot received no cell",
            });
        }

        let locator = Self {
            frequency,
            faces,
            table,
        };
        locator.validate(surface)?;
        Ok(locator)
    }

    /// One-time check: every located triangle's corners are mutual neighbours
    /// in the authoritative adjacency (spec §3.1).
    fn validate(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), TerrainAmplificationError> {
        use std::collections::HashSet;
        let mut adjacency: HashSet<(u32, u32)> = HashSet::with_capacity(surface.edges().len());
        for edge in surface.edges() {
            let (a, b) = (edge.cells[0].raw(), edge.cells[1].raw());
            adjacency.insert((a.min(b), a.max(b)));
        }
        for probe_index in 0..PROBE_COUNT {
            let probe = fibonacci_probe(probe_index, PROBE_COUNT);
            let (corners, _) = self.locate(probe);
            for (a, b) in [(0, 1), (1, 2), (0, 2)] {
                let (lo, hi) = (corners[a].min(corners[b]), corners[a].max(corners[b]));
                if lo == hi || !adjacency.contains(&(lo, hi)) {
                    return Err(TerrainAmplificationError::LocatorValidation {
                        reason: "triangle corners are not mutual authoritative neighbours",
                    });
                }
            }
        }
        Ok(())
    }

    fn locate(&self, position: UnitVector3) -> ([u32; 3], [f64; 3]) {
        let direction = position.components();
        let mut best_face = 0;
        let mut best_bary = [0.0; 3];
        let mut best_min = f64::NEG_INFINITY;
        for (face_index, face) in self.faces.iter().enumerate() {
            let bary = barycentric(face, direction);
            let min = bary[0].min(bary[1]).min(bary[2]);
            if min > best_min {
                best_min = min;
                best_face = face_index;
                best_bary = bary;
            }
        }
        let f = self.frequency as f64;
        let x = (best_bary[1].max(0.0) * f).min(f);
        let y = (best_bary[2].max(0.0) * f).min(f);
        let mut i = x.floor() as usize;
        let mut j = y.floor() as usize;
        if i + j >= self.frequency {
            // Clamp to the last full lattice row along the far edge.
            let overflow = i + j + 1 - self.frequency;
            let reduce_i = overflow.min(i);
            i -= reduce_i;
            j -= overflow - reduce_i;
        }
        let fx = x - i as f64;
        let fy = y - j as f64;
        let stride = (self.frequency + 1) * (self.frequency + 2) / 2;
        let base = best_face * stride;
        let corner = |a: usize, b: usize| self.table[base + lattice_index(self.frequency, a, b)];
        // The inverted triangle (i, j) only exists strictly inside the face:
        // its far corner (i+1, j+1) must stay on the lattice (i + j <= f - 2).
        // On the diagonal row the upright triangle is the only real triangle;
        // smooth_weights clamps and renormalizes the slightly extrapolated
        // barycentrics there.
        if fx + fy <= 1.0 || i + j + 2 > self.frequency {
            (
                [corner(i, j), corner(i + 1, j), corner(i, j + 1)],
                [1.0 - fx - fy, fx, fy],
            )
        } else {
            (
                [corner(i + 1, j + 1), corner(i, j + 1), corner(i + 1, j)],
                [fx + fy - 1.0, 1.0 - fx, 1.0 - fy],
            )
        }
    }
}

fn check_cardinality(
    field: &'static str,
    found: usize,
    expected: usize,
) -> Result<(), TerrainAmplificationError> {
    if found == expected {
        Ok(())
    } else {
        Err(TerrainAmplificationError::FieldCardinality {
            field,
            found,
            expected,
        })
    }
}

fn lattice_index(frequency: usize, i: usize, j: usize) -> usize {
    // Row-major over j with shrinking rows: sum_{r<j} (frequency+1-r) + i.
    j * (frequency + 1) - j * (j.saturating_sub(1)) / 2 + i
}

/// Planar barycentric coordinates of the central (gnomonic) projection onto
/// the face plane — the projective variant of spherical barycentric
/// interpolation (Langer et al. 2006 family).
fn barycentric(face: &FaceBasis, direction: [f64; 3]) -> [f64; 3] {
    let m = &face.inverse;
    let raw = [
        m[0][0] * direction[0] + m[0][1] * direction[1] + m[0][2] * direction[2],
        m[1][0] * direction[0] + m[1][1] * direction[1] + m[1][2] * direction[2],
        m[2][0] * direction[0] + m[2][1] * direction[1] + m[2][2] * direction[2],
    ];
    let sum = raw[0] + raw[1] + raw[2];
    // The central projection maps antipodal points onto identical ratios; a
    // positive coefficient sum selects the front hemisphere of the face.
    if sum <= f64::EPSILON {
        return [f64::NEG_INFINITY; 3];
    }
    [raw[0] / sum, raw[1] / sum, raw[2] / sum]
}

fn invert_3x3(columns: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    // Columns are the face corner vectors; invert the matrix whose columns
    // are the corners so that inverse * direction yields unnormalized
    // barycentric coordinates.
    let m = [
        [columns[0][0], columns[1][0], columns[2][0]],
        [columns[0][1], columns[1][1], columns[2][1]],
        [columns[0][2], columns[1][2], columns[2][2]],
    ];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let inv_det = 1.0 / det;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ]
}

/// Smoothstep-remapped, renormalized barycentric weights (spec §3.3).
fn smooth_weights(weights: [f64; 3]) -> [f64; 3] {
    let smooth = |w: f64| {
        let w = w.clamp(0.0, 1.0);
        w * w * (3.0 - 2.0 * w)
    };
    let s = [smooth(weights[0]), smooth(weights[1]), smooth(weights[2])];
    let sum = s[0] + s[1] + s[2];
    if sum <= f64::EPSILON {
        weights
    } else {
        [s[0] / sum, s[1] / sum, s[2] / sum]
    }
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// C5: the Langbein–Schumm peaked dissection response.
pub(super) fn langbein_schumm(precipitation_mm: f64) -> f64 {
    let deviation =
        (precipitation_mm - DISSECTION_PEAK_PRECIPITATION_MM) / DISSECTION_PRECIPITATION_WIDTH_MM;
    (-deviation * deviation).exp()
}

/// C4 amplitude channel: the detail-amplitude cap falling with substrate
/// erodibility toward the frozen floor.
pub(super) fn erodibility_amplitude(erodibility: f64) -> f64 {
    1.0 - (1.0 - ERODIBILITY_AMPLITUDE_FLOOR) * erodibility
}

/// C7: sediment-blanket amplitude damping toward a smooth filled surface.
pub(super) fn sediment_damping(sediment_norm: f64) -> f64 {
    (-sediment_norm).exp()
}

/// C10 baseline: the roughness-blended Hurst exponent between plains and
/// young mountains.
pub(super) fn surface_roughness_hurst(roughness: f64) -> f64 {
    HURST_PLAIN + (HURST_MOUNTAIN - HURST_PLAIN) * roughness
}

/// C6: the double-gated badlands peak — weak substrate and a semi-arid
/// dissection maximum together.
pub(super) fn badlands_gate(erodibility: f64, dissection: f64) -> f64 {
    smoothstep(0.55, 0.8, erodibility) * smoothstep(0.5, 0.8, dissection)
}

/// Minor-arc angle between two unit directions.
pub(super) fn arc_angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1] + a[2] * b[2])
        .clamp(-1.0, 1.0)
        .acos()
}

/// Whether `point` lies in the closed minor-arc spherical triangle.
///
/// Each directed edge compares the point with the opposite vertex, so the
/// test is independent of clockwise/counter-clockwise corner order.
pub(super) fn spherical_triangle_contains(point: [f64; 3], triangle: [[f64; 3]; 3]) -> bool {
    spherical_triangle_margin(point, triangle) >= -64.0 * f64::EPSILON
}

/// Smallest oriented half-space product for a spherical triangle.
/// Positive values are strictly inside, zero is on an edge.
pub(super) fn spherical_triangle_margin(point: [f64; 3], triangle: [[f64; 3]; 3]) -> f64 {
    let determinant = |a: [f64; 3], b: [f64; 3], p: [f64; 3]| {
        (a[1] * b[2] - a[2] * b[1]) * p[0]
            + (a[2] * b[0] - a[0] * b[2]) * p[1]
            + (a[0] * b[1] - a[1] * b[0]) * p[2]
    };
    let [a, b, c] = triangle;
    [(a, b, c), (b, c, a), (c, a, b)]
        .into_iter()
        .map(|(from, to, inside)| determinant(from, to, point) * determinant(from, to, inside))
        .fold(f64::INFINITY, f64::min)
}

/// Exact great-circle nearest point of `p` on the arc `from → to`:
/// returns `(lateral distance in metres, fraction along the arc)`, with
/// the nearer endpoint as the fallback when the projection leaves the
/// arc. A raw chord would sag hundreds of metres below the arc at
/// cell-spacing scales and miss the bed entirely (amendment A4).
pub(super) fn arc_nearest_point(
    from: [f64; 3],
    to: [f64; 3],
    p: [f64; 3],
    radius_m: f64,
) -> Option<(f64, f64)> {
    let normal = [
        from[1] * to[2] - from[2] * to[1],
        from[2] * to[0] - from[0] * to[2],
        from[0] * to[1] - from[1] * to[0],
    ];
    let normal_len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if normal_len <= f64::EPSILON {
        return None;
    }
    let normal = [
        normal[0] / normal_len,
        normal[1] / normal_len,
        normal[2] / normal_len,
    ];
    let sine = p[0] * normal[0] + p[1] * normal[1] + p[2] * normal[2];
    let projected = [
        p[0] - sine * normal[0],
        p[1] - sine * normal[1],
        p[2] - sine * normal[2],
    ];
    let projected_len =
        (projected[0] * projected[0] + projected[1] * projected[1] + projected[2] * projected[2])
            .sqrt();
    if projected_len <= f64::EPSILON {
        return None;
    }
    let onto = [
        projected[0] / projected_len,
        projected[1] / projected_len,
        projected[2] / projected_len,
    ];
    let cross_toward = |a: [f64; 3], b: [f64; 3]| {
        (a[1] * b[2] - a[2] * b[1]) * normal[0]
            + (a[2] * b[0] - a[0] * b[2]) * normal[1]
            + (a[0] * b[1] - a[1] * b[0]) * normal[2]
    };
    let within =
        cross_toward(from, onto) >= -f64::EPSILON && cross_toward(onto, to) >= -f64::EPSILON;
    if within {
        let total = arc_angle(from, to);
        let fraction = if total <= f64::EPSILON {
            0.0
        } else {
            (arc_angle(from, onto) / total).clamp(0.0, 1.0)
        };
        Some((sine.clamp(-1.0, 1.0).abs().asin() * radius_m, fraction))
    } else {
        let to_from = arc_angle(p, from);
        let to_to = arc_angle(p, to);
        if to_from <= to_to {
            Some((to_from * radius_m, 0.0))
        } else {
            Some((to_to * radius_m, 1.0))
        }
    }
}

fn substream_root(root_seed: RootSeed) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(SUBSTREAM_DOMAIN);
    hasher.update(&root_seed.raw().to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn substream(root: &[u8; 32], label: &str) -> ChaCha8Rng {
    let mut hasher = Hasher::new();
    hasher.update(SUBSTREAM_DOMAIN);
    hasher.update(root);
    hasher.update(&(label.len() as u64).to_le_bytes());
    hasher.update(label.as_bytes());
    ChaCha8Rng::from_seed(*hasher.finalize().as_bytes())
}

fn normalize_direction(components: [f64; 3]) -> UnitVector3 {
    let norm = (components[0] * components[0]
        + components[1] * components[1]
        + components[2] * components[2])
        .sqrt();
    UnitVector3::new(
        components[0] / norm,
        components[1] / norm,
        components[2] / norm,
    )
    .expect("a normalized displacement stays a unit vector")
}

/// The frozen spherical Fibonacci probe directions (spec §8).
pub fn fibonacci_probe(index: usize, count: usize) -> UnitVector3 {
    const GOLDEN_FRACTION: f64 = 0.618_033_988_749_894_9;
    let z = 1.0 - 2.0 * (index as f64 + 0.5) / count as f64;
    let radius = (1.0 - z * z).max(0.0).sqrt();
    let angle = std::f64::consts::TAU * ((index as f64 * GOLDEN_FRACTION) % 1.0);
    normalize_direction([radius * angle.cos(), radius * angle.sin(), z])
}

fn robust_reference(values: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return 1.0;
    }
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * NORMALIZATION_PERCENTILE).round() as usize;
    sorted[index].max(f64::MIN_POSITIVE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::{Meters, SphericalSpaceSpec};

    const TEST_RADIUS_M: f64 = 6_371_000.0;

    fn test_surface() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build_cancellable(
            &SphericalSpaceSpec {
                radius: Meters::new(TEST_RADIUS_M).unwrap(),
                target_cell_count: 162,
            },
            || false,
        )
        .unwrap()
    }

    struct SyntheticFields {
        elevation: Vec<f32>,
        sediment: Vec<f32>,
        erodibility: Vec<f32>,
        precipitation: Vec<f32>,
        crust_age: Vec<f32>,
        lineation_east: Vec<f32>,
        lineation_north: Vec<f32>,
        orogeny_kind: Vec<SphericalOrogenyKind>,
        orogeny_age: Vec<f32>,
    }

    impl SyntheticFields {
        fn new(surface: &SphericalSurfaceSnapshot, precipitation_mm: f32, sediment_m: f32) -> Self {
            let cells = surface.cells();
            let mut fields = Self {
                elevation: Vec::new(),
                sediment: Vec::new(),
                erodibility: Vec::new(),
                precipitation: Vec::new(),
                crust_age: Vec::new(),
                lineation_east: Vec::new(),
                lineation_north: Vec::new(),
                orogeny_kind: Vec::new(),
                orogeny_age: Vec::new(),
            };
            for cell in cells {
                let [x, y, z] = cell.centroid.components();
                // Northern sloped land, flat southern abyssal plain: keeps
                // the ocean detail purely additive for the C7 direction test.
                fields.elevation.push(if z >= 0.0 {
                    (2_500.0 * z - 500.0) as f32
                } else {
                    -1_800.0
                });
                fields
                    .sediment
                    .push(if z < -0.5 { sediment_m } else { 0.0 });
                fields.erodibility.push(if x > 0.0 { 0.9 } else { 0.2 });
                fields.precipitation.push(precipitation_mm);
                fields.crust_age.push((y.abs() * 100.0) as f32);
                fields.lineation_east.push(1.0);
                fields.lineation_north.push(0.0);
                fields.orogeny_kind.push(if z > 0.6 {
                    SphericalOrogenyKind::Andean
                } else {
                    SphericalOrogenyKind::None
                });
                fields.orogeny_age.push(10.0);
            }
            fields
        }

        fn view(&self) -> AmplificationFieldsView<'_> {
            AmplificationFieldsView {
                final_elevation_m: &self.elevation,
                sea_level_m: 0.0,
                sediment_thickness_m: &self.sediment,
                erodibility: &self.erodibility,
                annual_precipitation_mm: &self.precipitation,
                crust_age_myr: &self.crust_age,
                lineation_east: &self.lineation_east,
                lineation_north: &self.lineation_north,
                orogeny_kind: &self.orogeny_kind,
                orogeny_age_myr: &self.orogeny_age,
            }
        }
    }

    fn amplifier(
        precipitation_mm: f32,
        sediment_m: f32,
    ) -> (TerrainAmplifier, SphericalSurfaceSnapshot) {
        let surface = test_surface();
        let fields = SyntheticFields::new(&surface, precipitation_mm, sediment_m);
        let amplifier = TerrainAmplifier::new(&surface, fields.view(), RootSeed::new(42)).unwrap();
        (amplifier, surface)
    }

    fn dry_surface_water(surface: &SphericalSurfaceSnapshot) -> SurfaceWaterField {
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; surface.cells().len()])
    }

    #[test]
    fn construction_validates_cardinality_and_lattice() {
        let surface = test_surface();
        let fields = SyntheticFields::new(&surface, 800.0, 0.0);
        assert!(TerrainAmplifier::new(&surface, fields.view(), RootSeed::new(1)).is_ok());
        let short: Vec<f32> = fields.elevation[..10].to_vec();
        let mut view = fields.view();
        view.final_elevation_m = &short;
        assert!(matches!(
            TerrainAmplifier::new(&surface, view, RootSeed::new(1)),
            Err(TerrainAmplificationError::FieldCardinality { .. })
        ));
    }

    #[test]
    fn probe_fingerprint_is_deterministic_across_builds_and_threads() {
        let (first, _surface) = amplifier(800.0, 0.0);
        let (second, _surface_two) = amplifier(800.0, 0.0);
        let lod = AmplificationLod::new(3);
        assert_eq!(first.probe_fingerprint(lod), second.probe_fingerprint(lod));

        let sequential: Vec<f32> = (0..16)
            .map(|i| {
                first
                    .sample(fibonacci_probe(i, PROBE_COUNT), lod)
                    .elevation_m
            })
            .collect();
        let threaded: Vec<f32> = std::thread::scope(|scope| {
            let handle_a = scope.spawn(|| {
                (0..8)
                    .map(|i| {
                        first
                            .sample(fibonacci_probe(i, PROBE_COUNT), lod)
                            .elevation_m
                    })
                    .collect::<Vec<_>>()
            });
            let handle_b = scope.spawn(|| {
                (8..16)
                    .map(|i| {
                        first
                            .sample(fibonacci_probe(i, PROBE_COUNT), lod)
                            .elevation_m
                    })
                    .collect::<Vec<_>>()
            });
            let mut all = handle_a.join().unwrap();
            all.extend(handle_b.join().unwrap());
            all
        });
        assert_eq!(sequential, threaded);
    }

    #[test]
    fn every_probe_is_finite_and_bounded() {
        let (amplifier, _surface) = amplifier(800.0, 0.0);
        let lod = AmplificationLod::new(4);
        for index in 0..PROBE_COUNT {
            let sample = amplifier.sample(fibonacci_probe(index, PROBE_COUNT), lod);
            assert!(sample.elevation_m.is_finite());
            assert!((ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&sample.elevation_m));
        }
    }

    #[test]
    fn seam_and_pole_neighbourhoods_are_continuous() {
        let (amplifier, _surface) = amplifier(800.0, 0.0);
        let lod = AmplificationLod::new(3);
        let epsilon = 1.0e-4_f64;
        for latitude in [-1.1_f64, -0.4, 0.3, 0.9] {
            let (sin_lat, cos_lat) = latitude.sin_cos();
            let east = normalize_direction([
                cos_lat * (std::f64::consts::PI - epsilon).cos(),
                cos_lat * (std::f64::consts::PI - epsilon).sin(),
                sin_lat,
            ]);
            let west = normalize_direction([
                cos_lat * (-std::f64::consts::PI + epsilon).cos(),
                cos_lat * (-std::f64::consts::PI + epsilon).sin(),
                sin_lat,
            ]);
            let delta = (amplifier.sample(east, lod).elevation_m
                - amplifier.sample(west, lod).elevation_m)
                .abs();
            assert!(
                delta < 5.0,
                "meridian seam jump {delta} m at lat {latitude}"
            );
        }
        let near_pole_a = normalize_direction([1.0e-5, 0.0, 1.0]);
        let near_pole_b = normalize_direction([0.0, 1.0e-5, 1.0]);
        let delta = (amplifier.sample(near_pole_a, lod).elevation_m
            - amplifier.sample(near_pole_b, lod).elevation_m)
            .abs();
        assert!(delta < 5.0, "polar jump {delta} m");
    }

    #[test]
    fn erodibility_lowers_amplitude_on_the_weak_hemisphere() {
        let (amplifier, _surface) = amplifier(50.0, 0.0);
        let lod = AmplificationLod::new(4);
        let mut weak = Vec::new();
        let mut resistant = Vec::new();
        for index in 0..4096 {
            let probe = fibonacci_probe(index, 4096);
            let [x, _, z] = probe.components();
            if !(0.35..=0.9).contains(&z) {
                continue; // mid-latitude land, away from coast and orogen
            }
            let detail = f64::from(amplifier.sample(probe, lod).elevation_m)
                - amplifier.interpolate(probe).elevation_m;
            if x > 0.2 {
                weak.push(detail);
            } else if x < -0.2 {
                resistant.push(detail);
            }
        }
        let rms = |values: &[f64]| {
            (values.iter().map(|v| v * v).sum::<f64>() / values.len().max(1) as f64).sqrt()
        };
        assert!(
            rms(&weak) < rms(&resistant),
            "C4 direction violated: weak {} resistant {}",
            rms(&weak),
            rms(&resistant)
        );
    }

    #[test]
    fn dissection_peaks_in_the_semi_arid_band() {
        let lod = AmplificationLod::new(4);
        let carve_for = |precipitation: f32| {
            let (amplifier, _surface) = amplifier(precipitation, 0.0);
            let mut carved = 0.0;
            let mut count = 0.0;
            for index in 0..4096 {
                let probe = fibonacci_probe(index, 4096);
                let [_, _, z] = probe.components();
                if !(0.35..=0.9).contains(&z) {
                    continue;
                }
                let detail = f64::from(amplifier.sample(probe, lod).elevation_m)
                    - amplifier.interpolate(probe).elevation_m;
                carved += detail.min(0.0).abs();
                count += 1.0;
            }
            carved / count
        };
        let arid = carve_for(50.0);
        let semi_arid = carve_for(350.0);
        let humid = carve_for(2_000.0);
        assert!(
            semi_arid > arid && semi_arid > humid,
            "C5 peak violated: arid {arid}, semi-arid {semi_arid}, humid {humid}"
        );
    }

    #[test]
    fn sediment_blanket_damps_ocean_detail() {
        let lod = AmplificationLod::new(4);
        let rms_for = |sediment: f32| {
            let (amplifier, _surface) = amplifier(800.0, sediment);
            let mut sum = 0.0;
            let mut count = 0.0;
            for index in 0..4096 {
                let probe = fibonacci_probe(index, 4096);
                let [_, _, z] = probe.components();
                if z > -0.6 {
                    continue; // deep southern ocean only
                }
                let detail = f64::from(amplifier.sample(probe, lod).elevation_m)
                    - amplifier.interpolate(probe).elevation_m;
                sum += detail * detail;
                count += 1.0;
            }
            (sum / count).sqrt()
        };
        assert!(rms_for(600.0) < rms_for(0.0), "C7 direction violated");
    }

    #[test]
    fn first_differences_show_no_border_spikes() {
        let (amplifier, _surface) = amplifier(800.0, 0.0);
        let lod = AmplificationLod::new(3);
        let steps = 4_000;
        let mut previous = None;
        let mut diffs: Vec<f64> = Vec::with_capacity(steps);
        for step in 0..=steps {
            let angle = 0.9 * step as f64 / steps as f64;
            let direction =
                normalize_direction([angle.cos() * 0.6, angle.sin() * 0.6, 0.52 + 0.2 * angle]);
            let elevation = f64::from(amplifier.sample(direction, lod).elevation_m);
            if let Some(last) = previous {
                diffs.push(elevation - last);
            }
            previous = Some(elevation);
        }
        let mut magnitudes: Vec<f64> = diffs.iter().map(|d| d.abs()).collect();
        magnitudes.sort_by(f64::total_cmp);
        let median = magnitudes[magnitudes.len() / 2].max(1.0e-6);
        let max = magnitudes.last().copied().unwrap();
        assert!(
            max / median < 40.0,
            "border spike: max {max} m vs median {median} m"
        );
    }

    #[test]
    fn amplification_preserves_the_synthetic_land_fraction() {
        let (amplifier, _surface) = amplifier(800.0, 0.0);
        let lod = AmplificationLod::new(4);
        let mut t0_land = 0_u32;
        let mut amplified_land = 0_u32;
        let total = 16_384_usize;
        for index in 0..total {
            let probe = fibonacci_probe(index, total);
            if amplifier.interpolate(probe).elevation_m >= 0.0 {
                t0_land += 1;
            }
            if amplifier.sample(probe, lod).elevation_m >= 0.0 {
                amplified_land += 1;
            }
        }
        let drift = (f64::from(t0_land) - f64::from(amplified_land)).abs() / total as f64;
        assert!(drift <= 0.02, "land fraction drift {drift}");
    }

    fn single_reach_with_water(
        from_water: crate::world::natural::SurfaceWaterKind,
        to_water: crate::world::natural::SurfaceWaterKind,
        order: u8,
        discharge_m3_s: f32,
    ) -> (TerrainAmplifier, SphericalSurfaceSnapshot, usize) {
        use crate::world::natural::{
            RiverSegment, RiverSegmentKind, SurfaceWaterField, SurfaceWaterKind,
        };
        use crate::world::RiverSegmentId;

        let surface = test_surface();
        let fields = SyntheticFields::new(&surface, 800.0, 0.0);
        let edge_index = 0;
        let edge = &surface.edges()[edge_index];
        let [from, to] = edge.cells;
        let mut water = vec![SurfaceWaterKind::DryLand; surface.cells().len()];
        water[from.raw() as usize] = from_water;
        water[to.raw() as usize] = to_water;
        let water = SurfaceWaterField::from_kinds(water);
        let segments = [RiverSegment::new(
            RiverSegmentId::from_raw(0),
            from,
            to,
            RiverSegmentKind::Channel,
            order,
            discharge_m3_s,
        )
        .unwrap()];
        let amplifier = TerrainAmplifier::new(&surface, fields.view(), RootSeed::new(9))
            .unwrap()
            .with_rivers(&surface, &segments, &water)
            .unwrap();
        (amplifier, surface, edge_index)
    }

    #[test]
    fn river_reaches_split_at_shared_edge_and_omit_water_legs() {
        use crate::world::natural::SurfaceWaterKind::{DryLand, Lake};

        let cases = [
            (DryLand, DryLand, 2_usize),
            (DryLand, Lake, 1),
            (Lake, DryLand, 1),
            (Lake, Lake, 0),
        ];
        for (from_water, to_water, expected_legs) in cases {
            let (amplifier, surface, edge_index) =
                single_reach_with_water(from_water, to_water, 1, 80.0);
            let edge = &surface.edges()[edge_index];
            let [from, to] = edge.cells;
            let portal = edge.midpoint.components();
            let reach = &amplifier.reaches[0];
            let legs: Vec<_> = reach.legs.iter().flatten().collect();
            assert_eq!(legs.len(), expected_legs);

            if from_water == DryLand {
                let leg = legs
                    .iter()
                    .find(|leg| {
                        leg.from == surface.cells()[from.raw() as usize].centroid.components()
                    })
                    .expect("dry upstream cell owns one leg");
                assert_eq!(
                    leg.from,
                    surface.cells()[from.raw() as usize].centroid.components()
                );
                assert_eq!(leg.to, portal);
            }
            if to_water == DryLand {
                let leg = legs
                    .iter()
                    .find(|leg| leg.to == surface.cells()[to.raw() as usize].centroid.components())
                    .expect("dry downstream cell owns one leg");
                assert_eq!(leg.from, portal);
                assert_eq!(
                    leg.to,
                    surface.cells()[to.raw() as usize].centroid.components()
                );
            }

            for (cell, kind) in [(from, from_water), (to, to_water)] {
                let start = amplifier.reach_offsets[cell.raw() as usize] as usize;
                let end = amplifier.reach_offsets[cell.raw() as usize + 1] as usize;
                assert_eq!(
                    amplifier.reach_indices[start..end].contains(&0),
                    kind == DryLand,
                    "only dry owner cells index the reach"
                );
            }
        }
    }

    #[test]
    fn river_width_depends_on_discharge_not_strahler_order() {
        use crate::world::natural::SurfaceWaterKind::DryLand;

        let (first, _, _) = single_reach_with_water(DryLand, DryLand, 1, 160.0);
        let (second, _, _) = single_reach_with_water(DryLand, DryLand, 7, 160.0);
        assert_eq!(first.reaches[0].width_m, second.reaches[0].width_m);
    }

    #[test]
    fn non_adjacent_river_segment_is_rejected() {
        use crate::world::natural::{
            RiverSegment, RiverSegmentKind, SurfaceWaterField, SurfaceWaterKind,
        };
        use crate::world::{CellId, RiverSegmentId};

        let surface = test_surface();
        let fields = SyntheticFields::new(&surface, 800.0, 0.0);
        let from = CellId::from_raw(0);
        let neighbours: Vec<_> = surface.cells()[0]
            .boundary_edges
            .iter()
            .flat_map(|edge| surface.edges()[edge.raw() as usize].cells)
            .collect();
        let to = (1..surface.cells().len() as u32)
            .map(CellId::from_raw)
            .find(|cell| !neighbours.contains(cell))
            .expect("the test lattice has non-neighbouring cells");
        let segment = RiverSegment::new(
            RiverSegmentId::from_raw(0),
            from,
            to,
            RiverSegmentKind::Channel,
            1,
            10.0,
        )
        .unwrap();
        let water =
            SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; surface.cells().len()]);

        assert!(matches!(
            TerrainAmplifier::new(&surface, fields.view(), RootSeed::new(9))
                .unwrap()
                .with_rivers(&surface, &[segment], &water),
            Err(TerrainAmplificationError::RiverSegmentNotAdjacent {
                from: found_from,
                to: found_to,
            }) if found_from == from && found_to == to
        ));
    }

    #[test]
    fn river_carving_only_lowers_with_monotone_beds() {
        use crate::world::natural::{RiverSegment, RiverSegmentKind};
        use crate::world::RiverSegmentId;

        let surface = test_surface();
        let fields = SyntheticFields::new(&surface, 800.0, 0.0);
        let plain = TerrainAmplifier::new(&surface, fields.view(), RootSeed::new(9)).unwrap();
        let edge = &surface.edges()[0];
        let (a, b) = (edge.cells[0], edge.cells[1]);
        let next = surface
            .edges()
            .iter()
            .find(|candidate| candidate.cells.contains(&b) && !candidate.cells.contains(&a))
            .unwrap();
        let c = if next.cells[0] == b {
            next.cells[1]
        } else {
            next.cells[0]
        };
        let segments = vec![
            RiverSegment::new(
                RiverSegmentId::from_raw(0),
                a,
                b,
                RiverSegmentKind::Channel,
                1,
                120.0,
            )
            .unwrap(),
            RiverSegment::new(
                RiverSegmentId::from_raw(1),
                b,
                c,
                RiverSegmentKind::Channel,
                2,
                260.0,
            )
            .unwrap(),
        ];
        let carved = TerrainAmplifier::new(&surface, fields.view(), RootSeed::new(9))
            .unwrap()
            .with_rivers(&surface, &segments, &dry_surface_water(&surface))
            .unwrap();

        // Beds never rise downstream (spec §7 invariant, built structurally).
        for reach in &carved.reaches {
            assert!(reach.bed_from_m >= reach.bed_to_m);
        }

        // The carve surface itself descends monotonically along the chain.
        let chain = [a, b, c].map(|cell| surface.cells()[cell.raw() as usize].centroid);
        let mut previous = f64::INFINITY;
        for leg in 0..2 {
            let from = chain[leg].components();
            let to = chain[leg + 1].components();
            for step in 0..=24 {
                let t = f64::from(step) / 24.0;
                let direction = UnitVector3::new(
                    from[0] + t * (to[0] - from[0]),
                    from[1] + t * (to[1] - from[1]),
                    from[2] + t * (to[2] - from[2]),
                )
                .unwrap();
                let carve = carved.river_carve_m(direction, 0.0).unwrap();
                assert!(carve <= previous + 1e-6, "carve rose: {carve} > {previous}");
                previous = carve;
            }
        }

        // Carving only ever lowers the amplified surface (min semantics).
        let lod = AmplificationLod::new(3);
        for index in 0..512 {
            let probe = fibonacci_probe(index, 512);
            let with_rivers = carved.sample(probe, lod).elevation_m;
            let without = plain.sample(probe, lod).elevation_m;
            assert!(with_rivers <= without + 1e-3);
        }
    }

    #[test]
    fn river_network_cycles_are_rejected() {
        use crate::world::natural::{RiverSegment, RiverSegmentKind};
        use crate::world::RiverSegmentId;

        let surface = test_surface();
        let fields = SyntheticFields::new(&surface, 800.0, 0.0);
        let edge = &surface.edges()[0];
        let (a, b) = (edge.cells[0], edge.cells[1]);
        let segments = vec![
            RiverSegment::new(
                RiverSegmentId::from_raw(0),
                a,
                b,
                RiverSegmentKind::Channel,
                1,
                10.0,
            )
            .unwrap(),
            RiverSegment::new(
                RiverSegmentId::from_raw(1),
                b,
                a,
                RiverSegmentKind::Channel,
                1,
                10.0,
            )
            .unwrap(),
        ];
        assert!(matches!(
            TerrainAmplifier::new(&surface, fields.view(), RootSeed::new(9))
                .unwrap()
                .with_rivers(&surface, &segments, &dry_surface_water(&surface)),
            Err(TerrainAmplificationError::RiverNetworkCycle)
        ));
    }
}
