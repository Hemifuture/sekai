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

use blake3::Hasher;
use rand::RngCore;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;
use thiserror::Error;

use super::fractal::FractalProfile;
use super::morphology::noise::{GaborKernel, SphericalNoise3d};
use crate::generators::spatial::{BASE_FACE_VERTICES, BASE_VERTEX_COMPONENTS};
use crate::world::natural::{
    formation_annual_precipitation_mm, GeologicSubstrateSnapshot, NaturalSurfaceFormationSnapshot,
    SphericalOrogenyKind, SphericalTectonicSnapshot, ELEVATION_MAX_M, ELEVATION_MIN_M,
};
use crate::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::RootSeed;

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
/// C4: amplitude floor for the most erodible substrate.
const ERODIBILITY_AMPLITUDE_FLOOR: f64 = 0.4;
/// C5: Langbein–Schumm dissection peak and width (mm/yr equivalents).
const DISSECTION_PEAK_PRECIPITATION_MM: f64 = 350.0;
const DISSECTION_PRECIPITATION_WIDTH_MM: f64 = 250.0;
/// C7: sediment-blanket damping scale in metres of blanket thickness.
const SEDIMENT_DAMPING_M: f64 = 350.0;
/// Regime base amplitudes in metres (initial values; Task 4 calibrates).
const LAND_BASE_AMPLITUDE_M: f64 = 320.0;
const SHELF_BASE_AMPLITUDE_M: f64 = 35.0;
const OCEAN_BASE_AMPLITUDE_M: f64 = 90.0;
const RIDGE_AMPLITUDE_M: f64 = 450.0;
const DISSECTION_AMPLITUDE_M: f64 = 140.0;
const BADLANDS_AMPLITUDE_M: f64 = 60.0;
const COAST_DETAIL_AMPLITUDE_M: f64 = 25.0;
/// C9: warp displacement bounds as fractions of one T0 cell spacing.
const WARP_COAST_FRACTION: f64 = 0.55;
const WARP_OCEAN_FRACTION: f64 = 0.35;
const WARP_LAND_FRACTION: f64 = 0.15;
/// Shelf transition half-width in metres of depth around the shelf break.
const SHELF_TRANSITION_M: f64 = 80.0;
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
    local_relief_norm: Vec<f32>,
    age_gradient_norm: Vec<f32>,
    land: Vec<f32>,
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

        // C1 driver: local relief as the largest neighbour elevation drop.
        let local_relief_norm: Vec<f32> = (0..cell_count)
            .map(|index| {
                let here = f64::from(fields.final_elevation_m[index]);
                let relief = neighbours[index]
                    .iter()
                    .map(|&n| (here - f64::from(fields.final_elevation_m[n as usize])).abs())
                    .fold(0.0_f64, f64::max);
                ((relief / RELIEF_REFERENCE_M).clamp(RELIEF_FLOOR, 1.0)) as f32
            })
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
            local_relief_norm,
            age_gradient_norm,
            land,
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
        surface: &SphericalSurfaceSnapshot,
        compatibility: &SphericalTectonicSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        formation: &NaturalSurfaceFormationSnapshot,
        root_seed: RootSeed,
    ) -> Result<Self, TerrainAmplificationError> {
        let terrain = formation.terrain_fields();
        let monthly = formation
            .formation_climate()
            .fields()
            .monthly_precipitation_mm_day()
            .values();
        let annual_precipitation_mm: Vec<f32> = monthly
            .iter()
            .map(formation_annual_precipitation_mm)
            .collect();
        Self::new(
            surface,
            AmplificationFieldsView {
                final_elevation_m: terrain.final_elevation_m(),
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
        )
    }

    /// The base detail wavelength λ₀ = 2 × mean cell spacing (spec §6).
    pub fn base_wavelength_m(&self) -> f64 {
        self.base_wavelength_m
    }

    /// The sphere radius the amplifier was built for, in metres.
    pub fn radius_m(&self) -> f64 {
        self.radius_m
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
        let elevation = (interp.elevation_m + detail)
            .clamp(f64::from(ELEVATION_MIN_M), f64::from(ELEVATION_MAX_M));
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
        let hurst = HURST_PLAIN + (HURST_MOUNTAIN - HURST_PLAIN) * roughness;
        let persistence = 2.0_f64.powf(-hurst);

        // C1, C4 (amplitude channel), C7: the land amplitude envelope.
        let erodibility_amplitude = 1.0 - (1.0 - ERODIBILITY_AMPLITUDE_FLOOR) * interp.erodibility;
        let sediment_damping = (-interp.sediment).exp();
        let land_amplitude =
            LAND_BASE_AMPLITUDE_M * interp.local_relief * erodibility_amplitude * sediment_damping;
        let ocean_amplitude =
            OCEAN_BASE_AMPLITUDE_M * (0.3 + 0.7 * interp.age_gradient) * sediment_damping;
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
                * erodibility_amplitude
                * sediment_damping
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

            let badlands_gate = smoothstep(0.55, 0.8, interp.erodibility)
                * smoothstep(0.5, 0.8, precipitation_factor);
            if badlands_gate > 0.0 {
                let badlands_profile =
                    self.octave_profile((levels - 1).min(3), self.base_wavelength_m / 4.0);
                detail -= BADLANDS_AMPLITUDE_M
                    * badlands_gate
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
fn langbein_schumm(precipitation_mm: f64) -> f64 {
    let deviation =
        (precipitation_mm - DISSECTION_PEAK_PRECIPITATION_MM) / DISSECTION_PRECIPITATION_WIDTH_MM;
    (-deviation * deviation).exp()
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
}
