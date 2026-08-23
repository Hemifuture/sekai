use rand::RngCore as _;
use thiserror::Error;

use super::land_fraction::select_area_weighted_sea_level;
use super::random::{LabeledSubstreams, RELIEF_HOTSPOT_MORPHOLOGY_LABEL};
use super::spherical_island_relief::synthesize_spherical_hotspot_offset;
use super::spherical_relief::synthesize_conditioned_regional_detail;
use super::surface_water_geometry::{build_surface_water_geometry, solve_physical_sea_level};
use super::topology::{multi_source_distance, NaturalTopologyIndex};
use crate::engine::{Diagnostic, DiagnosticContext, DiagnosticSeverity, StageRng};
use crate::world::natural::{
    constraint_status, land_fraction_constraint_tolerance, scaled_earth_ocean_inventory_m3,
    BoundaryKind, CrustKind, ElevationField, EvolvedTectonicSnapshot,
    EvolvedTectonicValidationError, GeologicSubstrateSnapshot, GeologicSubstrateValidationError,
    PrimaryReliefSnapshot, PrimaryReliefValidationError, ReliefSpec, ReliefSpecError,
    ReliefValidationError, SeaLevelPolicy, SphericalReliefSnapshot, SphericalReliefValidationError,
    WaterVolumeSolveError, CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M, CONTINENTAL_CRUST_DENSITY_KG_M3,
    CRUST_BASE_ELEVATION_MAX_M, CRUST_BASE_ELEVATION_MIN_M,
    EARTH_OCEANIC_SEDIMENT_MEAN_THICKNESS_M, EARTH_OCEAN_CRUST_MEAN_AGE_MYR, ELEVATION_MAX_M,
    ELEVATION_MIN_M, OCEANIC_CRUST_DENSITY_KG_M3, OCEANIC_SEDIMENT_DENSITY_KG_M3,
    OCEAN_WATER_DENSITY_KG_M3, PASSIVE_MARGIN_OFFSET_ABS_MAX_M, PRIMARY_RELIEF_SCHEMA_V1,
    REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M, RELIEF_SCHEMA_V4, TECTONIC_OFFSET_MAX_M,
    TECTONIC_OFFSET_MIN_M, VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
use crate::world::spatial::{
    SphericalNaturalSurface, SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceRef,
    SurfaceRefError,
};
use crate::world::CellId;

const MANTLE_DENSITY_KG_M3: f32 = 3_300.0;
const CONTINENTAL_REFERENCE_THICKNESS_KM: f32 = 35.0;
const CONTINENTAL_REFERENCE_FREEBOARD_M: f32 = 250.0;
const OCEANIC_REFERENCE_THICKNESS_KM: f32 = 7.0;
const UNKNOWN_MIXED_OCEAN_AGE_MYR: f32 = 80.0;
const DYNAMIC_ACCUMULATED_RESPONSE_WEIGHT: f32 = 0.65;
const DYNAMIC_RATE_RESPONSE_M_PER_MM_PER_YEAR: f32 = 250.0;
const PASSIVE_MARGIN_SUPPORT_M: f64 = 900_000.0;
const PASSIVE_MARGIN_OCEANWARD_RISE_M: f32 = 1_200.0;
const PASSIVE_MARGIN_CONTINENTAL_DROP_M: f32 = -250.0;
const PASSIVE_MARGIN_FORCING_MAX_MM_PER_YEAR: f32 = 0.35;
const HEIGHT_QUANTUM_M: f32 = 0.25;
const CANCELLATION_POLL_STRIDE: usize = 256;
const MAX_CLAMP_DIAGNOSTICS: usize = 32;
const CLAMP_DIAGNOSTIC_CODE: &str = "natural.primary-relief-clamped";

/// Deterministic physical construction of the first formed global relief.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrimaryReliefGenerator;

impl PrimaryReliefGenerator {
    /// Generates density-aware, causal relief and solves sea level from water volume.
    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        surface: &SphericalSurfaceSnapshot,
        evolved: &EvolvedTectonicSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        relief_spec: &ReliefSpec,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<PrimaryReliefSnapshot, PrimaryReliefGenerationError> {
        check_cancelled(rng)?;
        surface.validate()?;
        evolved.validate_against(surface)?;
        substrate.validate_against(surface, evolved)?;
        relief_spec.validate()?;

        let cancellation = rng.cancellation_signal();
        let streams = LabeledSubstreams::capture(rng);
        streams
            .check_cancelled()
            .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;
        let compatibility = evolved.compatibility();
        let count = surface.cells().len();
        let material = evolved.material();
        let forcing = evolved.forcing();
        let mut isostatic_base = Vec::with_capacity(count);
        let mut dynamic_tectonic = Vec::with_capacity(count);

        for index in 0..count {
            if index % CANCELLATION_POLL_STRIDE == 0 {
                streams
                    .check_cancelled()
                    .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;
            }
            let continental_area = material.continental_reference_area_m2()[index];
            let oceanic_area = material.oceanic_reference_area_m2()[index];
            let total_area = continental_area + oceanic_area;
            let density = substrate.crust_density_kg_m3()[index];
            let continental_thickness = component_thickness_km(
                continental_area,
                material.continental_volume_m3()[index],
                CONTINENTAL_REFERENCE_THICKNESS_KM,
            );
            let oceanic_thickness = component_thickness_km(
                oceanic_area,
                material.oceanic_volume_m3()[index],
                OCEANIC_REFERENCE_THICKNESS_KM,
            );
            let ocean_age = if substrate.crust_kind(index) == Some(CrustKind::Oceanic) {
                substrate.ocean_age_myr()[index]
            } else {
                UNKNOWN_MIXED_OCEAN_AGE_MYR
            };
            let continental_base = continental_airy_elevation_m(continental_thickness, density);
            let oceanic_base = oceanic_isostatic_elevation_m(ocean_age, oceanic_thickness, density);
            let base = ((continental_base * continental_area as f32
                + oceanic_base * oceanic_area as f32)
                / total_area as f32)
                .clamp(CRUST_BASE_ELEVATION_MIN_M, CRUST_BASE_ELEVATION_MAX_M);
            let base = quantize(base);
            // The V5 compatibility elevation on oceanic crust is the same
            // plate-cooling depth the Parsons-Sclater base above already
            // carries; inheriting it would count thermal subsidence twice
            // (T0 calibration spec §4 L0). Continental crust keeps the
            // inherited orogenic response.
            let accumulated_response = if substrate.crust_kind(index) == Some(CrustKind::Oceanic) {
                0.0
            } else {
                causal_accumulated_response_m(
                    compatibility.tectonic_elevation_m()[index],
                    forcing.uplift_rate_mm_per_year()[index],
                    forcing.subsidence_rate_mm_per_year()[index],
                )
            };
            let dynamic = quantize(dynamic_tectonic_response_m(
                accumulated_response,
                forcing.uplift_rate_mm_per_year()[index],
                forcing.subsidence_rate_mm_per_year()[index],
            ));
            isostatic_base.push(base);
            dynamic_tectonic.push(dynamic);
        }

        let mut hotspot_rng = streams.stream(RELIEF_HOTSPOT_MORPHOLOGY_LABEL);
        let mut volcanic = synthesize_spherical_hotspot_offset(
            surface,
            compatibility,
            substrate.mantle(),
            hotspot_rng.next_u32(),
        );
        streams
            .check_cancelled()
            .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        let mut passive_margin = synthesize_passive_margin(
            &topology,
            compatibility,
            forcing.uplift_rate_mm_per_year(),
            forcing.subsidence_rate_mm_per_year(),
            forcing.shortening_rate_mm_per_year(),
        );
        let mut regional_detail =
            synthesize_conditioned_regional_detail(surface, compatibility, &streams)
                .map_err(|_| PrimaryReliefGenerationError::Cancelled)?
                .into_iter()
                .map(|value| {
                    quantize(value).clamp(
                        -CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M,
                        CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M,
                    )
                })
                .collect::<Vec<_>>();
        for value in &mut volcanic {
            *value = quantize(*value).clamp(VOLCANIC_OFFSET_MIN_M, VOLCANIC_OFFSET_MAX_M);
        }
        for value in &mut passive_margin {
            *value = quantize(*value).clamp(
                -PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
                PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
            );
        }

        let elevation = reconcile_primary_safety(
            &mut isostatic_base,
            &mut dynamic_tectonic,
            &mut volcanic,
            &mut passive_margin,
            &mut regional_detail,
            diagnostics,
        );
        streams
            .check_cancelled()
            .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;

        let areas = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .collect::<Vec<_>>();
        let earth_inventory = scaled_earth_ocean_inventory_m3(surface.total_cell_area().get())?;
        let (water_inventory, water_geometry) = match relief_spec.sea_level_policy {
            SeaLevelPolicy::WaterInventory => {
                let water_inventory =
                    earth_inventory * f64::from(relief_spec.water_inventory_ratio);
                let water = solve_physical_sea_level(surface, &elevation, water_inventory)?;
                (water_inventory, water.into_geometry())
            }
            SeaLevelPolicy::TargetLandFraction => {
                let selection = select_area_weighted_sea_level(
                    &areas,
                    &elevation,
                    f64::from(relief_spec.target_land_fraction),
                )
                .map_err(|error| {
                    PrimaryReliefGenerationError::InvalidLandFractionSelection(error.to_string())
                })?;
                let geometry = build_surface_water_geometry(
                    surface,
                    &elevation,
                    selection.sea_level_m,
                    &cancellation,
                )?;
                let water_inventory = geometry.total_water_volume_m3();
                (water_inventory, geometry)
            }
        };
        let sea_level_m = water_geometry.sea_level_m();
        let realized_water_volume = water_geometry.total_water_volume_m3();
        let elevation_field = ElevationField::from_values(elevation.clone())?;
        let land_ocean = water_geometry.land_ocean().clone();
        let regional = passive_margin
            .iter()
            .zip(&regional_detail)
            .map(|(&passive, &detail)| passive + detail)
            .collect::<Vec<_>>();
        let compatibility_relief = SphericalReliefSnapshot::new(
            RELIEF_SCHEMA_V4,
            SurfaceRef::for_spherical(surface),
            sea_level_m,
            ElevationField::from_values(isostatic_base.clone())?,
            ElevationField::from_values(dynamic_tectonic.clone())?,
            ElevationField::from_values(volcanic.clone())?,
            ElevationField::from_values(regional)?,
            elevation_field,
            land_ocean,
        )?;
        let physical_land = water_geometry
            .global_land_area_fraction(surface)
            .map_err(WaterVolumeSolveError::from)?;
        let tolerance = land_fraction_constraint_tolerance(surface)?;
        let status = constraint_status(relief_spec.target_land_fraction, physical_land, tolerance);
        let snapshot = PrimaryReliefSnapshot::new(
            PRIMARY_RELIEF_SCHEMA_V1,
            SurfaceRef::for_spherical(surface),
            compatibility_relief,
            isostatic_base,
            dynamic_tectonic,
            volcanic,
            passive_margin,
            regional_detail,
            elevation,
            water_inventory,
            realized_water_volume,
            relief_spec.target_land_fraction,
            physical_land,
            tolerance,
            status,
        )?;
        snapshot.validate_against(surface, &water_geometry, substrate, relief_spec)?;
        Ok(snapshot)
    }
}

/// Density-aware local Airy column balance for continental material.
pub fn continental_airy_elevation_m(thickness_km: f32, crust_density_kg_m3: f32) -> f32 {
    CONTINENTAL_REFERENCE_FREEBOARD_M
        + (((MANTLE_DENSITY_KG_M3 - crust_density_kg_m3) * thickness_km
            - (MANTLE_DENSITY_KG_M3 - CONTINENTAL_CRUST_DENSITY_KG_M3)
                * CONTINENTAL_REFERENCE_THICKNESS_KM)
            / MANTLE_DENSITY_KG_M3)
            * 1_000.0
}

/// GDH1 empirical ocean basement depth in metres for age in Myr (Stein &
/// Stein 1992): half-space cooling to 20 Myr, then the thinner, hotter plate
/// asymptote that replaced the Parsons-Sclater 1977 law whose old-crust floor
/// was 300-500 m too deep (T0 calibration spec §4 R4).
pub fn gdh1_ocean_depth_m(age_myr: f32) -> f32 {
    let age_myr = age_myr.max(0.0);
    if age_myr <= 20.0 {
        2_600.0 + 365.0 * age_myr.sqrt()
    } else {
        5_651.0 - 2_473.0 * (-0.0278 * age_myr).exp()
    }
}

/// Seafloor rise above the GDH1 basement from the Airy-compensated pelagic
/// sediment blanket of an oceanic column of the given age.
///
/// The blanket grows linearly with age at Earth's mean oceanic sediment
/// thickness over Earth's mean crustal age (CRUST1.0 oceanic types and Seton
/// et al. 2020) and is backstripped with the Sclater & Christie 1980 density
/// ratio `(rho_mantle - rho_sediment) / (rho_mantle - rho_water)`.
pub fn oceanic_sediment_seafloor_rise_m(age_myr: f32) -> f32 {
    let thickness_m =
        EARTH_OCEANIC_SEDIMENT_MEAN_THICKNESS_M * age_myr.max(0.0) / EARTH_OCEAN_CRUST_MEAN_AGE_MYR;
    thickness_m * (MANTLE_DENSITY_KG_M3 - OCEANIC_SEDIMENT_DENSITY_KG_M3)
        / (MANTLE_DENSITY_KG_M3 - OCEAN_WATER_DENSITY_KG_M3)
}

/// Oceanic thermal basement depth, density/thickness buoyancy relative to the
/// 7 km basalt reference, and the compensated sediment blanket.
pub fn oceanic_isostatic_elevation_m(
    age_myr: f32,
    thickness_km: f32,
    crust_density_kg_m3: f32,
) -> f32 {
    let buoyancy = ((MANTLE_DENSITY_KG_M3 - crust_density_kg_m3) * thickness_km
        - (MANTLE_DENSITY_KG_M3 - OCEANIC_CRUST_DENSITY_KG_M3) * OCEANIC_REFERENCE_THICKNESS_KM)
        / MANTLE_DENSITY_KG_M3
        * 1_000.0;
    -gdh1_ocean_depth_m(age_myr) + buoyancy + oceanic_sediment_seafloor_rise_m(age_myr)
}

/// Converts long-lived relief and present V5 rates into the bounded P3 response.
pub fn dynamic_tectonic_response_m(
    accumulated_response_m: f32,
    uplift_rate_mm_per_year: f32,
    subsidence_rate_mm_per_year: f32,
) -> f32 {
    (DYNAMIC_ACCUMULATED_RESPONSE_WEIGHT * accumulated_response_m
        + DYNAMIC_RATE_RESPONSE_M_PER_MM_PER_YEAR
            * (uplift_rate_mm_per_year - subsidence_rate_mm_per_year))
        .clamp(TECTONIC_OFFSET_MIN_M, TECTONIC_OFFSET_MAX_M)
}

/// Projects inherited coarse response onto an active normal forcing's causal sign.
pub fn causal_accumulated_response_m(
    accumulated_response_m: f32,
    uplift_rate_mm_per_year: f32,
    subsidence_rate_mm_per_year: f32,
) -> f32 {
    if uplift_rate_mm_per_year > subsidence_rate_mm_per_year && uplift_rate_mm_per_year > 0.0 {
        accumulated_response_m.max(0.0)
    } else if subsidence_rate_mm_per_year > uplift_rate_mm_per_year
        && subsidence_rate_mm_per_year > 0.0
    {
        accumulated_response_m.min(0.0)
    } else {
        accumulated_response_m
    }
}

fn synthesize_passive_margin(
    topology: &NaturalTopologyIndex,
    tectonic: &crate::world::natural::SphericalTectonicSnapshot,
    uplift: &[f32],
    subsidence: &[f32],
    shortening: &[f32],
) -> Vec<f32> {
    let mut continental_sources = Vec::new();
    let mut oceanic_sources = Vec::new();
    for (edge_index, owners) in topology.edge_owners().iter().enumerate() {
        let [Some(first), Some(second)] = *owners else {
            continue;
        };
        let first_kind = tectonic
            .crust_kind(first)
            .expect("validated tectonic field is dense");
        let second_kind = tectonic
            .crust_kind(second)
            .expect("validated tectonic field is dense");
        if first_kind == second_kind {
            continue;
        }
        let boundary = tectonic.boundaries()[edge_index];
        if !matches!(boundary.kind, BoundaryKind::None | BoundaryKind::Weak) {
            continue;
        }
        let strongly_forced = [first, second].into_iter().any(|cell| {
            let index = cell.raw() as usize;
            uplift[index].max(subsidence[index]).max(shortening[index])
                > PASSIVE_MARGIN_FORCING_MAX_MM_PER_YEAR
        });
        if strongly_forced {
            continue;
        }
        for (cell, kind) in [(first, first_kind), (second, second_kind)] {
            match kind {
                CrustKind::Continental => continental_sources.push(cell),
                CrustKind::Oceanic => oceanic_sources.push(cell),
            }
        }
    }
    continental_sources.sort_unstable();
    continental_sources.dedup();
    oceanic_sources.sort_unstable();
    oceanic_sources.dedup();
    if continental_sources.is_empty() || oceanic_sources.is_empty() {
        return vec![0.0; topology.cell_count()];
    }
    let support = topology
        .quantized_distance_for_meters(PASSIVE_MARGIN_SUPPORT_M)
        .max(1);
    let continental_distance = multi_source_distance(topology, &continental_sources, Some(support));
    let oceanic_distance = multi_source_distance(topology, &oceanic_sources, Some(support));
    (0..topology.cell_count())
        .map(|index| {
            match tectonic
                .crust_kind(CellId::from_raw(index as u32))
                .expect("validated tectonic field is dense")
            {
                CrustKind::Continental => {
                    PASSIVE_MARGIN_CONTINENTAL_DROP_M
                        * compact_graph_profile(continental_distance[index], support)
                }
                CrustKind::Oceanic => {
                    PASSIVE_MARGIN_OCEANWARD_RISE_M
                        * compact_graph_profile(oceanic_distance[index], support)
                }
            }
        })
        .collect()
}

fn compact_graph_profile(distance: u64, support: u64) -> f32 {
    if distance == u64::MAX || distance >= support {
        0.0
    } else {
        let t = distance as f32 / support as f32;
        1.0 - t * t * (3.0 - 2.0 * t)
    }
}

fn component_thickness_km(area_m2: f64, volume_m3: f64, fallback_km: f32) -> f32 {
    if area_m2 > 0.0 {
        (volume_m3 / area_m2 / 1_000.0) as f32
    } else {
        fallback_km
    }
}

#[allow(clippy::too_many_arguments)]
fn reconcile_primary_safety(
    isostatic: &mut [f32],
    dynamic: &mut [f32],
    volcanic: &mut [f32],
    passive: &mut [f32],
    detail: &mut [f32],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<f32> {
    let mut result = Vec::with_capacity(isostatic.len());
    let mut clamped_count = 0_usize;
    for index in 0..isostatic.len() {
        constrain_regional_pair(&mut passive[index], &mut detail[index]);
        let raw =
            isostatic[index] + dynamic[index] + volcanic[index] + passive[index] + detail[index];
        let target = raw.clamp(ELEVATION_MIN_M, ELEVATION_MAX_M);
        if target != raw {
            clamped_count += 1;
            let mut remaining = target - raw;
            let detail_min = (-CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M)
                .max(REGIONAL_OFFSET_MIN_M - passive[index]);
            let detail_max =
                CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M.min(REGIONAL_OFFSET_MAX_M - passive[index]);
            remaining = adjust_component(&mut detail[index], remaining, detail_min, detail_max);
            let passive_min =
                (-PASSIVE_MARGIN_OFFSET_ABS_MAX_M).max(REGIONAL_OFFSET_MIN_M - detail[index]);
            let passive_max =
                PASSIVE_MARGIN_OFFSET_ABS_MAX_M.min(REGIONAL_OFFSET_MAX_M - detail[index]);
            remaining = adjust_component(&mut passive[index], remaining, passive_min, passive_max);
            remaining = adjust_component(
                &mut dynamic[index],
                remaining,
                TECTONIC_OFFSET_MIN_M,
                TECTONIC_OFFSET_MAX_M,
            );
            remaining = adjust_component(
                &mut isostatic[index],
                remaining,
                CRUST_BASE_ELEVATION_MIN_M,
                CRUST_BASE_ELEVATION_MAX_M,
            );
            let _remaining = adjust_component(
                &mut volcanic[index],
                remaining,
                VOLCANIC_OFFSET_MIN_M,
                VOLCANIC_OFFSET_MAX_M,
            );
            if diagnostics.len() < MAX_CLAMP_DIAGNOSTICS {
                diagnostics.push(
                    Diagnostic::with_context(
                        DiagnosticSeverity::Warning,
                        CLAMP_DIAGNOSTIC_CODE,
                        format!("reconciled raw primary relief {raw} m to {target} m"),
                        DiagnosticContext {
                            cell_id: Some(CellId::from_raw(index as u32)),
                            ..DiagnosticContext::default()
                        },
                    )
                    .expect("engine-owned primary-relief diagnostic code is valid"),
                );
            }
        }
        result.push(
            isostatic[index] + dynamic[index] + volcanic[index] + passive[index] + detail[index],
        );
    }
    if clamped_count > MAX_CLAMP_DIAGNOSTICS {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticSeverity::Warning,
                CLAMP_DIAGNOSTIC_CODE,
                format!(
                    "{} additional cells required primary-relief reconciliation",
                    clamped_count - MAX_CLAMP_DIAGNOSTICS
                ),
            )
            .expect("engine-owned primary-relief diagnostic code is valid"),
        );
    }
    result
}

fn constrain_regional_pair(passive: &mut f32, detail: &mut f32) {
    let total = *passive + *detail;
    let target = total.clamp(REGIONAL_OFFSET_MIN_M, REGIONAL_OFFSET_MAX_M);
    let mut remaining = target - total;
    remaining = adjust_component(
        detail,
        remaining,
        -CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M,
        CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M,
    );
    let _remaining = adjust_component(
        passive,
        remaining,
        -PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
        PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
    );
}

fn adjust_component(value: &mut f32, delta: f32, minimum: f32, maximum: f32) -> f32 {
    let previous = *value;
    *value = (previous + delta).clamp(minimum, maximum);
    delta - (*value - previous)
}

fn quantize(value: f32) -> f32 {
    (value / HEIGHT_QUANTUM_M).round() * HEIGHT_QUANTUM_M
}

fn check_cancelled(rng: &StageRng) -> Result<(), PrimaryReliefGenerationError> {
    rng.check_cancelled()
        .map_err(|_| PrimaryReliefGenerationError::Cancelled)
}

/// Failures that prevent publication of physical P3 relief.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PrimaryReliefGenerationError {
    /// The owning build requested cooperative cancellation.
    #[error("primary relief generation was cancelled")]
    Cancelled,
    /// The authoritative sphere is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The validated surface could not provide its exact identity view.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The V5 tectonic causes or surface identity are invalid.
    #[error("invalid evolved tectonics: {0}")]
    InvalidEvolved(#[from] EvolvedTectonicValidationError),
    /// The substrate is invalid or disagrees with V5.
    #[error("invalid geologic substrate: {0}")]
    InvalidSubstrate(#[from] GeologicSubstrateValidationError),
    /// The authoring constraint is malformed.
    #[error("invalid relief specification: {0}")]
    InvalidSpec(#[from] ReliefSpecError),
    /// A generated dense elevation field is invalid.
    #[error("invalid generated elevation field: {0}")]
    InvalidReliefField(#[from] ReliefValidationError),
    /// The compatibility relief could not be constructed.
    #[error("invalid compatibility relief: {0}")]
    InvalidCompatibility(#[from] SphericalReliefValidationError),
    /// The physical water-volume operator failed.
    #[error("physical water solve failed: {0}")]
    InvalidWaterSolve(#[from] WaterVolumeSolveError),
    /// The authored land fraction could not be represented by the surface.
    #[error("target land-fraction solve failed: {0}")]
    InvalidLandFractionSelection(String),
    /// The final strict primary-relief snapshot is invalid.
    #[error("generated primary relief is invalid: {0}")]
    InvalidSnapshot(#[from] PrimaryReliefValidationError),
}
