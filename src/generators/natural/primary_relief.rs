use rand::RngCore as _;
use thiserror::Error;

use super::land_fraction::select_area_weighted_sea_level;
use super::random::{LabeledSubstreams, RELIEF_HOTSPOT_MORPHOLOGY_LABEL};
use super::spherical_island_relief::synthesize_spherical_hotspot_offset;
use super::spherical_relief::synthesize_conditioned_regional_detail;
use super::surface_water_geometry::{
    build_surface_water_working_geometry, solve_physical_sea_level_exact,
    SurfaceWaterWorkingGeometry,
};
use super::topology::{multi_source_distance, NaturalTopologyIndex};
use crate::engine::{BuildCancellation, Diagnostic, StageRng};
use crate::world::natural::{
    constraint_status, continental_airy_elevation_exact_m, land_fraction_constraint_tolerance,
    scaled_earth_ocean_inventory_m3, BoundaryKind, BoundaryRecord, CrustKind, CrustKindField,
    EvolvedTectonicSnapshot, EvolvedTectonicValidationError, GeologicSubstrateSnapshot,
    GeologicSubstrateValidationError, PrimaryReliefSnapshot, PrimaryReliefValidationError,
    ReliefSpec, ReliefSpecError, SeaLevelPolicy, WaterVolumeSolveError,
    CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M, CRUST_BASE_ELEVATION_MAX_EXACT_M,
    CRUST_BASE_ELEVATION_MIN_M, EARTH_OCEANIC_SEDIMENT_MEAN_THICKNESS_M,
    EARTH_OCEAN_CRUST_MEAN_AGE_MYR, ELEVATION_MAX_M, ELEVATION_MIN_M, OCEANIC_CRUST_DENSITY_KG_M3,
    OCEANIC_SEDIMENT_DENSITY_KG_M3, OCEAN_WATER_DENSITY_KG_M3, PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
    PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3, PRIMARY_RELIEF_CONTINENTAL_REFERENCE_THICKNESS_KM,
    PRIMARY_RELIEF_OCEANIC_REFERENCE_THICKNESS_KM, PRIMARY_RELIEF_SCHEMA_V3, VOLCANIC_OFFSET_MAX_M,
    VOLCANIC_OFFSET_MIN_M,
};
use crate::world::spatial::{
    SphericalNaturalSurface, SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceRef,
    SurfaceRefError,
};
use crate::world::CellId;

const UNKNOWN_MIXED_OCEAN_AGE_MYR: f64 = 80.0;
const PASSIVE_MARGIN_SUPPORT_M: f64 = 900_000.0;
const PASSIVE_MARGIN_OCEANWARD_RISE_M: f64 = 1_200.0;
const PASSIVE_MARGIN_CONTINENTAL_DROP_M: f64 = -250.0;
const CANCELLATION_POLL_STRIDE: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::generators::natural) struct PrimaryReliefWorkingState {
    isostatic_base_m: Vec<f64>,
    volcanic_construction_m: Vec<f64>,
    passive_margin_offset_m: Vec<f64>,
    conditioned_regional_detail_m: Vec<f64>,
    elevation_m: Vec<f64>,
    water_inventory_m3: f64,
    surface_water_geometry: SurfaceWaterWorkingGeometry,
    requested_land_fraction: f32,
}

impl PrimaryReliefWorkingState {
    /// Returns the exact retained P3 elevation before wire quantization.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::generators::natural) fn elevation_exact_m(&self) -> &[f64] {
        &self.elevation_m
    }

    /// Returns the exact fractional P3 water geometry carried into P5.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::generators::natural) const fn surface_water_geometry(
        &self,
    ) -> &SurfaceWaterWorkingGeometry {
        &self.surface_water_geometry
    }

    fn to_snapshot(
        &self,
        surface: &SphericalSurfaceSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        relief_spec: &ReliefSpec,
        cancellation: &BuildCancellation,
    ) -> Result<PrimaryReliefSnapshot, PrimaryReliefGenerationError> {
        let isostatic_base_m = project_component_to_wire(&self.isostatic_base_m);
        let volcanic_construction_m = project_component_to_wire(&self.volcanic_construction_m);
        let passive_margin_offset_m = project_component_to_wire(&self.passive_margin_offset_m);
        let conditioned_regional_detail_m =
            project_component_to_wire(&self.conditioned_regional_detail_m);
        let elevation_m = project_component_to_wire(&self.elevation_m);
        let surface_water_geometry =
            self.surface_water_geometry
                .to_wire(surface, &elevation_m, cancellation)?;
        let physical_land_fraction = surface_water_geometry
            .global_land_area_fraction(surface)
            .map_err(WaterVolumeSolveError::from)?;
        let tolerance = land_fraction_constraint_tolerance(surface)?;
        let status = constraint_status(
            self.requested_land_fraction,
            physical_land_fraction,
            tolerance,
        );
        let snapshot = PrimaryReliefSnapshot::new(
            PRIMARY_RELIEF_SCHEMA_V3,
            SurfaceRef::for_spherical(surface),
            isostatic_base_m,
            volcanic_construction_m,
            passive_margin_offset_m,
            conditioned_regional_detail_m,
            elevation_m,
            self.water_inventory_m3,
            surface_water_geometry,
            self.requested_land_fraction,
            physical_land_fraction,
            tolerance,
            status,
        )?;
        snapshot.validate_against(surface, substrate, relief_spec)?;
        Ok(snapshot)
    }
}

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
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<PrimaryReliefSnapshot, PrimaryReliefGenerationError> {
        check_cancelled(rng)?;
        let cancellation = rng.cancellation_signal();
        let streams = LabeledSubstreams::capture(rng);
        let (_, snapshot) = Self::generate_working_from_streams(
            surface,
            evolved,
            substrate,
            relief_spec,
            &streams,
            &cancellation,
        )?;
        Ok(snapshot)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::generators::natural) fn generate_working_from_streams(
        surface: &SphericalSurfaceSnapshot,
        evolved: &EvolvedTectonicSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        relief_spec: &ReliefSpec,
        streams: &LabeledSubstreams,
        cancellation: &BuildCancellation,
    ) -> Result<(PrimaryReliefWorkingState, PrimaryReliefSnapshot), PrimaryReliefGenerationError>
    {
        surface.validate()?;
        evolved.validate_against(surface)?;
        substrate.validate_against(surface, evolved)?;
        relief_spec.validate()?;
        streams
            .check_cancelled()
            .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;

        let tectonic = evolved.authoritative_view();
        let count = surface.cells().len();
        let material = tectonic.material();
        let mut isostatic_base_m = Vec::with_capacity(count);

        for index in 0..count {
            if index % CANCELLATION_POLL_STRIDE == 0 {
                streams
                    .check_cancelled()
                    .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;
            }
            let continental_area = material.continental_reference_area_m2()[index];
            let oceanic_area = material.oceanic_reference_area_m2()[index];
            let total_area = continental_area + oceanic_area;
            let density = f64::from(substrate.crust_density_kg_m3()[index]);
            let continental_thickness = component_thickness_km(
                continental_area,
                material.continental_volume_m3()[index],
                PRIMARY_RELIEF_CONTINENTAL_REFERENCE_THICKNESS_KM,
            );
            let oceanic_thickness = component_thickness_km(
                oceanic_area,
                material.oceanic_volume_m3()[index],
                PRIMARY_RELIEF_OCEANIC_REFERENCE_THICKNESS_KM,
            );
            let ocean_age = if substrate.crust_kind(index) == Some(CrustKind::Oceanic) {
                f64::from(substrate.ocean_age_myr()[index])
            } else {
                UNKNOWN_MIXED_OCEAN_AGE_MYR
            };
            let continental_base =
                continental_airy_elevation_exact_m(continental_thickness, density);
            let oceanic_base =
                oceanic_isostatic_elevation_exact_m(ocean_age, oceanic_thickness, density);
            let base =
                (continental_base * continental_area + oceanic_base * oceanic_area) / total_area;
            validate_component_value(
                "isostatic_base_m",
                index,
                base,
                f64::from(CRUST_BASE_ELEVATION_MIN_M),
                CRUST_BASE_ELEVATION_MAX_EXACT_M,
            )?;
            isostatic_base_m.push(base);
        }

        let mut hotspot_rng = streams.stream(RELIEF_HOTSPOT_MORPHOLOGY_LABEL);
        let volcanic_construction_m = synthesize_spherical_hotspot_offset(
            surface,
            tectonic.plates(),
            tectonic.cell_plates(),
            tectonic.crust_kinds(),
            substrate.mantle(),
            hotspot_rng.next_u32(),
        );
        streams
            .check_cancelled()
            .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;

        let surface_view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&surface_view);
        let passive_margin_offset_m =
            synthesize_passive_margin(&topology, tectonic.crust_kinds(), tectonic.boundaries());
        let conditioned_regional_detail_m = synthesize_conditioned_regional_detail(
            surface,
            tectonic.crust_kinds(),
            tectonic.crust_age_myr(),
            tectonic.lineation_east(),
            tectonic.lineation_north(),
            tectonic.orogeny_kind(),
            tectonic.orogeny_age_myr(),
            streams,
        )
        .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;

        validate_component_field(
            "volcanic_construction_m",
            &volcanic_construction_m,
            f64::from(VOLCANIC_OFFSET_MIN_M),
            f64::from(VOLCANIC_OFFSET_MAX_M),
        )?;
        validate_component_field(
            "passive_margin_offset_m",
            &passive_margin_offset_m,
            -f64::from(PASSIVE_MARGIN_OFFSET_ABS_MAX_M),
            f64::from(PASSIVE_MARGIN_OFFSET_ABS_MAX_M),
        )?;
        validate_component_field(
            "conditioned_regional_detail_m",
            &conditioned_regional_detail_m,
            -f64::from(CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M),
            f64::from(CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M),
        )?;

        let elevation_m = compose_primary_elevation(
            &isostatic_base_m,
            &volcanic_construction_m,
            &passive_margin_offset_m,
            &conditioned_regional_detail_m,
        )?;
        streams
            .check_cancelled()
            .map_err(|_| PrimaryReliefGenerationError::Cancelled)?;

        let cell_areas = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .collect::<Vec<_>>();
        let earth_inventory = scaled_earth_ocean_inventory_m3(surface.total_cell_area().get())?;
        let (water_inventory_m3, surface_water_geometry) = match relief_spec.sea_level_policy {
            SeaLevelPolicy::WaterInventory => {
                let inventory = earth_inventory * f64::from(relief_spec.water_inventory_ratio);
                let water =
                    solve_physical_sea_level_exact(surface, &elevation_m, inventory, cancellation)?;
                (inventory, water.into_geometry())
            }
            SeaLevelPolicy::TargetLandFraction => {
                let selection = select_area_weighted_sea_level(
                    &cell_areas,
                    &elevation_m,
                    f64::from(relief_spec.target_land_fraction),
                )
                .map_err(|error| {
                    PrimaryReliefGenerationError::InvalidLandFractionSelection(error.to_string())
                })?;
                let geometry = build_surface_water_working_geometry(
                    surface,
                    &elevation_m,
                    selection.sea_level_m,
                    cancellation,
                )?;
                (geometry.total_water_volume_m3(), geometry)
            }
        };
        let working = PrimaryReliefWorkingState {
            isostatic_base_m,
            volcanic_construction_m,
            passive_margin_offset_m,
            conditioned_regional_detail_m,
            elevation_m,
            water_inventory_m3,
            surface_water_geometry,
            requested_land_fraction: relief_spec.target_land_fraction,
        };
        let snapshot = working.to_snapshot(surface, substrate, relief_spec, cancellation)?;
        Ok((working, snapshot))
    }
}

/// Density-aware local Airy column balance for continental material.
pub fn continental_airy_elevation_m(thickness_km: f32, crust_density_kg_m3: f32) -> f32 {
    continental_airy_elevation_exact_m(f64::from(thickness_km), f64::from(crust_density_kg_m3))
        as f32
}

/// GDH1 empirical ocean basement depth in metres for age in Myr (Stein &
/// Stein 1992): half-space cooling to 20 Myr, then the thinner, hotter plate
/// asymptote that replaced the Parsons-Sclater 1977 law whose old-crust floor
/// was 300-500 m too deep (T0 calibration spec §4 R4).
pub fn gdh1_ocean_depth_m(age_myr: f32) -> f32 {
    gdh1_ocean_depth_exact_m(f64::from(age_myr)) as f32
}

fn gdh1_ocean_depth_exact_m(age_myr: f64) -> f64 {
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
    oceanic_sediment_seafloor_rise_exact_m(f64::from(age_myr)) as f32
}

fn oceanic_sediment_seafloor_rise_exact_m(age_myr: f64) -> f64 {
    let thickness_m = f64::from(EARTH_OCEANIC_SEDIMENT_MEAN_THICKNESS_M) * age_myr.max(0.0)
        / f64::from(EARTH_OCEAN_CRUST_MEAN_AGE_MYR);
    thickness_m
        * (PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3 - f64::from(OCEANIC_SEDIMENT_DENSITY_KG_M3))
        / (PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3 - f64::from(OCEAN_WATER_DENSITY_KG_M3))
}

/// Oceanic thermal basement depth, density/thickness buoyancy relative to the
/// 7 km basalt reference, and the compensated sediment blanket.
pub fn oceanic_isostatic_elevation_m(
    age_myr: f32,
    thickness_km: f32,
    crust_density_kg_m3: f32,
) -> f32 {
    oceanic_isostatic_elevation_exact_m(
        f64::from(age_myr),
        f64::from(thickness_km),
        f64::from(crust_density_kg_m3),
    ) as f32
}

fn oceanic_isostatic_elevation_exact_m(
    age_myr: f64,
    thickness_km: f64,
    crust_density_kg_m3: f64,
) -> f64 {
    let buoyancy = ((PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3 - crust_density_kg_m3)
        * thickness_km
        - (PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3 - f64::from(OCEANIC_CRUST_DENSITY_KG_M3))
            * PRIMARY_RELIEF_OCEANIC_REFERENCE_THICKNESS_KM)
        / PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3
        * 1_000.0;
    -gdh1_ocean_depth_exact_m(age_myr) + buoyancy + oceanic_sediment_seafloor_rise_exact_m(age_myr)
}

fn synthesize_passive_margin(
    topology: &NaturalTopologyIndex,
    crust_kinds: &CrustKindField,
    boundaries: &[BoundaryRecord],
) -> Vec<f64> {
    let mut continental_sources = Vec::new();
    let mut oceanic_sources = Vec::new();
    for (edge_index, owners) in topology.edge_owners().iter().enumerate() {
        let [Some(first), Some(second)] = *owners else {
            continue;
        };
        let first_kind = crust_kinds
            .get(first.raw() as usize)
            .expect("validated tectonic field is dense");
        let second_kind = crust_kinds
            .get(second.raw() as usize)
            .expect("validated tectonic field is dense");
        if first_kind == second_kind {
            continue;
        }
        if !matches!(
            boundaries[edge_index].kind,
            BoundaryKind::None | BoundaryKind::Weak
        ) {
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
            match crust_kinds
                .get(index)
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

fn compact_graph_profile(distance: u64, support: u64) -> f64 {
    if distance == u64::MAX || distance >= support {
        0.0
    } else {
        let t = distance as f64 / support as f64;
        1.0 - t * t * (3.0 - 2.0 * t)
    }
}

fn component_thickness_km(area_m2: f64, volume_m3: f64, fallback_km: f64) -> f64 {
    if area_m2 > 0.0 {
        volume_m3 / area_m2 / 1_000.0
    } else {
        fallback_km
    }
}

fn validate_component_value(
    field: &'static str,
    index: usize,
    found: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), PrimaryReliefGenerationError> {
    if !found.is_finite() || !(minimum..=maximum).contains(&found) {
        return Err(PrimaryReliefGenerationError::ComponentOutOfRange {
            field,
            cell: CellId::from_raw(index as u32),
            found,
            minimum,
            maximum,
        });
    }
    Ok(())
}

fn validate_component_field(
    field: &'static str,
    values: &[f64],
    minimum: f64,
    maximum: f64,
) -> Result<(), PrimaryReliefGenerationError> {
    for (index, &found) in values.iter().enumerate() {
        validate_component_value(field, index, found, minimum, maximum)?;
    }
    Ok(())
}

fn compose_primary_elevation(
    isostatic: &[f64],
    volcanic: &[f64],
    passive: &[f64],
    detail: &[f64],
) -> Result<Vec<f64>, PrimaryReliefGenerationError> {
    let expected = isostatic.len();
    for (field, found) in [
        ("volcanic_construction_m", volcanic.len()),
        ("passive_margin_offset_m", passive.len()),
        ("conditioned_regional_detail_m", detail.len()),
    ] {
        if found != expected {
            return Err(PrimaryReliefGenerationError::ComponentLengthMismatch {
                field,
                expected,
                found,
            });
        }
    }
    let mut elevation = Vec::with_capacity(expected);
    for index in 0..expected {
        let exact = isostatic[index] + volcanic[index] + passive[index] + detail[index];
        if !exact.is_finite()
            || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&exact)
        {
            return Err(PrimaryReliefGenerationError::ElevationOutOfRange {
                cell: CellId::from_raw(index as u32),
                found: exact,
            });
        }
        elevation.push(exact);
    }
    Ok(elevation)
}

fn project_component_to_wire(values: &[f64]) -> Vec<f32> {
    values.iter().map(|value| *value as f32).collect()
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
    /// Causal component arrays do not share one surface cardinality.
    #[error("component {field} has length {found}; expected {expected}")]
    ComponentLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    /// A named causal component left its established artifact support domain.
    #[error("{field} at {cell:?} is {found}; expected {minimum}..={maximum}")]
    ComponentOutOfRange {
        field: &'static str,
        cell: CellId,
        found: f64,
        minimum: f64,
        maximum: f64,
    },
    /// The unmodified causal component sum left the final elevation domain.
    #[error("primary elevation at {cell:?} is outside its supported domain: {found}")]
    ElevationOutOfRange { cell: CellId, found: f64 },
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

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;
    use std::time::Instant;

    use super::{LabeledSubstreams, PrimaryReliefGenerator};
    use crate::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
    use crate::generators::natural::{
        solve_physical_sea_level, EvolvedTectonicGenerator, GeologicSubstrateGenerator,
    };
    use crate::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
    use crate::world::natural::{
        EvolvedTectonicSnapshot, GeologicSpec, GeologicSubstrateSnapshot, NaturalQualityProfile,
        ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
        WorldFormationPreset, CONTINENTAL_CRUST_DENSITY_KG_M3, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
        CRUST_BASE_ELEVATION_MAX_M, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WATER_VOLUME_RELATIVE_TOLERANCE,
    };
    use crate::world::{Meters, RootSeed};

    struct Fixture {
        bundle: ProfileSurfaceBundle,
        evolved: EvolvedTectonicSnapshot,
        substrate: GeologicSubstrateSnapshot,
    }

    fn formation() -> ResolvedWorldFormation {
        ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .unwrap()
    }

    fn fixture() -> &'static Fixture {
        static FIXTURE: OnceLock<Fixture> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            let bundle = ProfileSurfaceBuilder::build(
                NaturalQualityProfile::Draft,
                Meters::new(6_371_000.0).unwrap(),
                &BuildCancellation::new(),
            )
            .unwrap();
            let mut tectonic_rng = StageRng::from_seed(derive_stage_seed(
                RootSeed::new(42),
                StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
            ));
            let evolved = EvolvedTectonicGenerator::generate(
                &bundle,
                &TectonicSpec::default(),
                &formation(),
                &mut tectonic_rng,
            )
            .unwrap();
            let mut substrate_rng = StageRng::from_seed(derive_stage_seed(
                RootSeed::new(42),
                StageIdentity::new("natural.geologic-substrate", 1, "sekai.core"),
            ));
            let substrate = GeologicSubstrateGenerator::generate(
                bundle.authoritative_surface(),
                &evolved,
                &GeologicSpec::default(),
                &formation(),
                &mut substrate_rng,
            )
            .unwrap();
            Fixture {
                bundle,
                evolved,
                substrate,
            }
        })
    }

    fn generate_working(
        evolved: &EvolvedTectonicSnapshot,
    ) -> (
        super::PrimaryReliefWorkingState,
        crate::world::natural::PrimaryReliefSnapshot,
    ) {
        let fixture = fixture();
        let cancellation = BuildCancellation::new();
        let mut rng = StageRng::from_seed_with_cancellation(
            derive_stage_seed(
                RootSeed::new(42),
                StageIdentity::new("natural.primary-relief", 3, "sekai.core"),
            ),
            &cancellation,
        );
        let streams = LabeledSubstreams::capture(&mut rng);
        PrimaryReliefGenerator::generate_working_from_streams(
            fixture.bundle.authoritative_surface(),
            evolved,
            &fixture.substrate,
            &ReliefSpec::default(),
            &streams,
            &cancellation,
        )
        .unwrap()
    }

    #[test]
    fn current_tectonic_rates_do_not_create_p3_displacement() {
        let fixture = fixture();
        let mut wire = serde_json::to_value(&fixture.evolved).unwrap();
        for field in ["uplift_rate_mm_per_year", "subsidence_rate_mm_per_year"] {
            for value in wire["forcing"][field].as_array_mut().unwrap() {
                *value = serde_json::json!(0.0_f32);
            }
        }
        let changed: EvolvedTectonicSnapshot = serde_json::from_value(wire).unwrap();

        let original = generate_working(&fixture.evolved);
        let rate_changed = generate_working(&changed);

        assert_eq!(rate_changed, original);
        let published = serde_json::to_value(&original.1).unwrap();
        assert!(published.get("dynamic_tectonic_offset_m").is_none());
    }

    #[test]
    fn crust_base_domain_covers_the_frozen_airy_input_domain() {
        let exact = super::continental_airy_elevation_exact_m(
            f64::from(CONTINENTAL_CRUST_MAX_THICKNESS_KM),
            f64::from(CONTINENTAL_CRUST_DENSITY_KG_M3),
        );
        assert_eq!(
            exact.to_bits(),
            super::CRUST_BASE_ELEVATION_MAX_EXACT_M.to_bits()
        );
        assert!(f64::from(CRUST_BASE_ELEVATION_MAX_M) >= exact);
        let previous = f32::from_bits(CRUST_BASE_ELEVATION_MAX_M.to_bits() - 1);
        assert!(f64::from(previous) < exact);
    }

    #[test]
    fn p3_water_ledger_is_solved_on_exact_f64_elevation() {
        let fixture = fixture();
        let (working, snapshot) = generate_working(&fixture.evolved);
        let exact_realized = working.surface_water_geometry.total_water_volume_m3();
        let exact_relative_error = crate::world::natural::water_volume_relative_error(
            exact_realized,
            working.water_inventory_m3,
        );
        assert!(exact_relative_error <= WATER_VOLUME_RELATIVE_TOLERANCE);

        for index in 0..snapshot.elevation_m().len() {
            assert_eq!(
                working.surface_water_geometry.land_ocean().get(index),
                snapshot.land_ocean().get(index)
            );
        }

        let wire_solution = solve_physical_sea_level(
            fixture.bundle.authoritative_surface(),
            snapshot.elevation_m(),
            working.water_inventory_m3,
        )
        .unwrap();
        let projection_started = Instant::now();
        let projected = working
            .surface_water_geometry
            .to_wire(
                fixture.bundle.authoritative_surface(),
                snapshot.elevation_m(),
                &BuildCancellation::new(),
            )
            .unwrap();
        eprintln!(
            "P3 exact water: relative_error={exact_relative_error:e}, exact_vs_wire_volume_delta_m3={:e}, exact_to_wire_ms={:.3}",
            exact_realized - wire_solution.realized_water_volume_m3(),
            projection_started.elapsed().as_secs_f64() * 1_000.0,
        );
        assert_eq!(projected, *snapshot.surface_water_geometry());
    }
}
