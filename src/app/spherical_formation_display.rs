//! Immutable field document for the formation-product chain (P2v5→P5).
//!
//! Mirrors the natural-foundation document boundary: it extracts shared
//! siblings from one complete causal-formation bundle build outcome,
//! cross-validates their identities, and exposes the same renderer-independent
//! [`FieldDocument`] surface the spherical presenters already consume.

use std::sync::Arc;

use thiserror::Error;

use super::field_document::{owned_view_diagnostics, FieldDocument};
use super::natural_field_payloads::elevation_display_radius_m;
use crate::engine::{
    ArtifactError, BuildOutcome, BuildOutcomeIntegrityError, BuildProvenance, BuildReport,
    BuildResultHash,
};
use crate::generators::natural::{
    NaturalFormationBundleArtifact, ReliefSpecArtifact, ResolvedTectonicInput,
    ResolvedTectonicInputArtifact,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::view::{
    DisplayRangeMode, FieldCatalog, FieldPayloadRef, FieldViewError, OwnedViewDiagnostic,
    SphericalPresentationSource,
};
use crate::world::fields::{FieldId, FieldRegistry, ValueRange};
use crate::world::natural::{
    annual_local_runoff_mm_field_id, circulation_annual_evaporation_mm_field_id,
    circulation_annual_precipitation_mm_field_id,
    circulation_mean_absorbed_shortwave_w_m2_field_id, circulation_mean_air_temperature_c_field_id,
    circulation_mean_outgoing_longwave_w_m2_field_id, circulation_prevailing_wind_m_s_field_id,
    circulation_surface_albedo_field_id, climatological_annual_total_mm,
    climatological_monthly_mean, coastal_deposition_m_field_id,
    coastal_deposition_rate_m_per_year_field_id, coastal_erosion_m_field_id,
    coastal_erosion_rate_m_per_year_field_id, crust_kind_field_id, crust_thickness_field_id,
    drainage_area_km2_field_id, fluvial_erosion_depth_m_field_id,
    fluvial_erosion_rate_m_per_year_field_id, hillslope_deposition_m_field_id,
    hillslope_deposition_rate_m_per_year_field_id, hillslope_erosion_m_field_id,
    hillslope_erosion_rate_m_per_year_field_id, isostatic_response_m_field_id,
    isostatic_response_rate_m_per_year_field_id, lake_depth_m_field_id, land_ocean_field_id,
    mean_annual_discharge_m3_s_field_id, ocean_age_myr_field_id, plate_id_field_id,
    primary_elevation_m_field_id, routed_sediment_deposition_m_field_id,
    routed_sediment_deposition_rate_m_per_year_field_id, sediment_deposition_thickness_m_field_id,
    spherical_formation_field_registry, strahler_stream_order_field_id,
    surface_elevation_m_field_id, surface_water_kind_field_id, tectonic_displacement_m_field_id,
    tectonic_displacement_rate_m_per_year_field_id, ClimateBudgetReport, GlobalCirculationFields,
    NaturalFieldRegistryError, NaturalFormationBundleValidationError, PrimaryReliefValidationError,
    SeaLevelPolicy, SphericalTectonicValidationError, SurfaceFormationValidationError,
    CLIMATE_MONTH_COUNT,
};
use crate::world::spatial::{
    canonical_east_north_basis, SphericalSurfaceSnapshot, SphericalSurfaceValidationError,
    SurfaceRef,
};
use crate::world::RootSeed;

const SPHERICAL_FORMATION_GRAPH_CONTRACT_VERSION: u16 = 3;

/// Stable provenance identity for one complete formation-product document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SphericalFormationBuildIdentity {
    root_seed: RootSeed,
    surface_ref: SurfaceRef,
    build_result_hash: BuildResultHash,
    graph_contract_version: u16,
}

impl SphericalFormationBuildIdentity {
    fn new(provenance: &BuildProvenance, surface_ref: SurfaceRef) -> Self {
        Self {
            root_seed: provenance.root_seed(),
            surface_ref,
            build_result_hash: *provenance.result_hash(),
            graph_contract_version: SPHERICAL_FORMATION_GRAPH_CONTRACT_VERSION,
        }
    }

    /// Returns the root seed that drove the authoritative graph.
    pub(super) const fn root_seed(&self) -> RootSeed {
        self.root_seed
    }

    /// Returns the exact authoritative surface identity.
    pub(super) const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the engine-audited result hash of the complete build.
    pub(super) const fn build_result_hash(&self) -> &BuildResultHash {
        &self.build_result_hash
    }

    /// Returns the presentation contract version of the formation graph.
    pub(super) const fn graph_contract_version(&self) -> u16 {
        self.graph_contract_version
    }
}

/// Read-only water and energy budget copied from the final P5 climate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct P4WaterEnergySummary {
    evaporation_global_mean_mm_day: f64,
    precipitation_global_mean_mm_day: f64,
    evaporation_minus_precipitation_global_mean_mm_day: f64,
    evaporation_precipitation_relative_imbalance: f64,
    absorbed_shortwave_global_mean_w_m2: f64,
    outgoing_longwave_global_mean_w_m2: f64,
    toa_net_radiation_global_mean_w_m2: f64,
    planetary_albedo_global_mean: f64,
}

impl P4WaterEnergySummary {
    pub(super) fn from_budget_report(report: &ClimateBudgetReport) -> Self {
        Self {
            evaporation_global_mean_mm_day: report.evaporation_global_mean_mm_day(),
            precipitation_global_mean_mm_day: report.precipitation_global_mean_mm_day(),
            evaporation_minus_precipitation_global_mean_mm_day: report
                .evaporation_minus_precipitation_global_mean_mm_day(),
            evaporation_precipitation_relative_imbalance: report
                .evaporation_precipitation_relative_imbalance(),
            absorbed_shortwave_global_mean_w_m2: report.absorbed_shortwave_global_mean_w_m2(),
            outgoing_longwave_global_mean_w_m2: report.outgoing_longwave_global_mean_w_m2(),
            toa_net_radiation_global_mean_w_m2: report.toa_net_radiation_global_mean_w_m2(),
            planetary_albedo_global_mean: report.planetary_albedo_global_mean(),
        }
    }

    pub const fn evaporation_global_mean_mm_day(&self) -> f64 {
        self.evaporation_global_mean_mm_day
    }

    pub const fn precipitation_global_mean_mm_day(&self) -> f64 {
        self.precipitation_global_mean_mm_day
    }

    pub const fn evaporation_minus_precipitation_global_mean_mm_day(&self) -> f64 {
        self.evaporation_minus_precipitation_global_mean_mm_day
    }

    pub const fn evaporation_precipitation_relative_imbalance(&self) -> f64 {
        self.evaporation_precipitation_relative_imbalance
    }

    pub const fn absorbed_shortwave_global_mean_w_m2(&self) -> f64 {
        self.absorbed_shortwave_global_mean_w_m2
    }

    pub const fn outgoing_longwave_global_mean_w_m2(&self) -> f64 {
        self.outgoing_longwave_global_mean_w_m2
    }

    pub const fn toa_net_radiation_global_mean_w_m2(&self) -> f64 {
        self.toa_net_radiation_global_mean_w_m2
    }

    pub const fn planetary_albedo_global_mean(&self) -> f64 {
        self.planetary_albedo_global_mean
    }
}

/// Build-time authoring-compliance measurements for the formation product.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormationAreaSummary {
    authored_continental_fraction: f32,
    evolved_continental_fraction: f64,
    target_land_fraction: f32,
    actual_land_fraction: f64,
    sea_level_m: f32,
    sea_level_policy: SeaLevelPolicy,
    water_inventory_ratio: f64,
    p4_water_energy: P4WaterEnergySummary,
}

impl FormationAreaSummary {
    pub(super) const fn new(
        authored_continental_fraction: f32,
        evolved_continental_fraction: f64,
        target_land_fraction: f32,
        actual_land_fraction: f64,
        sea_level_m: f32,
        sea_level_policy: SeaLevelPolicy,
        water_inventory_ratio: f64,
        p4_water_energy: P4WaterEnergySummary,
    ) -> Self {
        Self {
            authored_continental_fraction,
            evolved_continental_fraction,
            target_land_fraction,
            actual_land_fraction,
            sea_level_m,
            sea_level_policy,
            water_inventory_ratio,
            p4_water_energy,
        }
    }

    /// Returns the author-requested initial continental crust area fraction.
    pub const fn authored_continental_fraction(&self) -> f32 {
        self.authored_continental_fraction
    }

    /// Returns the area-weighted evolved continental crust fraction.
    pub const fn evolved_continental_fraction(&self) -> f64 {
        self.evolved_continental_fraction
    }

    /// Returns the authored nominal land fraction carried by the built relief spec.
    pub const fn target_land_fraction(&self) -> f32 {
        self.target_land_fraction
    }

    /// Returns the area-weighted land fraction of the published surface.
    pub const fn actual_land_fraction(&self) -> f64 {
        self.actual_land_fraction
    }

    /// Returns the published global sea level selected by the active driver.
    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    /// Returns the sea-level driver used by the published build.
    pub const fn sea_level_policy(&self) -> SeaLevelPolicy {
        self.sea_level_policy
    }

    /// Returns P3 inventory relative to the area-scaled Earth ocean reference.
    pub const fn water_inventory_ratio(&self) -> f64 {
        self.water_inventory_ratio
    }

    /// Returns the final formation climate's authoritative P4 budget.
    pub const fn p4_water_energy(&self) -> P4WaterEnergySummary {
        self.p4_water_energy
    }
}

/// Owned display arrays derived once from the formation product's own
/// published end-state circulation, so the climate on screen is consistent
/// with the terrain on screen.
struct FormationDisplayCache {
    annual_evaporation_mm: Vec<f32>,
    annual_precipitation_mm: Vec<f32>,
    mean_absorbed_shortwave_w_m2: Vec<f32>,
    mean_air_temperature_c: Vec<f32>,
    mean_outgoing_longwave_w_m2: Vec<f32>,
    prevailing_wind_m_s: Vec<[f32; 2]>,
}

impl FormationDisplayCache {
    fn build(
        surface: &SphericalSurfaceSnapshot,
        fields: &GlobalCirculationFields,
    ) -> Result<Self, SphericalFormationDisplayError> {
        let cell_count = surface.cells().len();
        if fields.cell_count() != cell_count {
            return Err(SphericalFormationDisplayError::CirculationCardinality {
                circulation_cells: fields.cell_count(),
                surface_cells: cell_count,
            });
        }
        let monthly_precipitation = fields.monthly_precipitation_mm_day().values();
        let monthly_evaporation = fields.monthly_evaporation_mm_day().values();
        let monthly_absorbed_shortwave = fields.monthly_absorbed_shortwave_w_m2().values();
        let monthly_temperature = fields.monthly_air_temperature_c().values();
        let monthly_outgoing_longwave = fields.monthly_outgoing_longwave_w_m2().values();
        let monthly_wind = fields.near_surface_wind_m_s().values();

        let mut annual_evaporation_mm = Vec::with_capacity(cell_count);
        let mut annual_precipitation_mm = Vec::with_capacity(cell_count);
        let mut mean_absorbed_shortwave_w_m2 = Vec::with_capacity(cell_count);
        let mut mean_air_temperature_c = Vec::with_capacity(cell_count);
        let mut mean_outgoing_longwave_w_m2 = Vec::with_capacity(cell_count);
        let mut prevailing_wind_m_s = Vec::with_capacity(cell_count);
        for (index, cell) in surface.cells().iter().enumerate() {
            annual_evaporation_mm.push(display_annual_water_total_mm(
                "circulation_annual_evaporation_mm",
                index,
                &monthly_evaporation[index],
            )?);
            annual_precipitation_mm.push(display_annual_water_total_mm(
                "circulation_annual_precipitation_mm",
                index,
                &monthly_precipitation[index],
            )?);
            mean_absorbed_shortwave_w_m2.push(climatological_monthly_mean(
                &monthly_absorbed_shortwave[index],
            ));
            mean_air_temperature_c.push(climatological_monthly_mean(&monthly_temperature[index]));
            mean_outgoing_longwave_w_m2.push(climatological_monthly_mean(
                &monthly_outgoing_longwave[index],
            ));
            let mut mean_wind = [0.0_f64; 3];
            for month in &monthly_wind[index] {
                for (axis, component) in month.iter().enumerate() {
                    mean_wind[axis] += f64::from(*component);
                }
            }
            for component in &mut mean_wind {
                *component /= CLIMATE_MONTH_COUNT as f64;
            }
            let (east, north) = canonical_east_north_basis(cell.centroid);
            let east_component =
                east[0] * mean_wind[0] + east[1] * mean_wind[1] + east[2] * mean_wind[2];
            let north_component =
                north[0] * mean_wind[0] + north[1] * mean_wind[1] + north[2] * mean_wind[2];
            prevailing_wind_m_s.push([east_component as f32, north_component as f32]);
        }
        Ok(Self {
            annual_evaporation_mm,
            annual_precipitation_mm,
            mean_absorbed_shortwave_w_m2,
            mean_air_temperature_c,
            mean_outgoing_longwave_w_m2,
            prevailing_wind_m_s,
        })
    }
}

fn display_annual_water_total_mm(
    field: &'static str,
    cell: usize,
    monthly_mm_day: &[f32; CLIMATE_MONTH_COUNT],
) -> Result<f32, SphericalFormationDisplayError> {
    let annual = climatological_annual_total_mm(monthly_mm_day) as f32;
    if !annual.is_finite() {
        return Err(SphericalFormationDisplayError::ReductionOverflow { field, cell });
    }
    Ok(annual)
}

/// Projection-free, immutable document for one complete formation world.
pub struct SphericalFormationFieldDocument {
    pub(super) surface: Arc<SphericalSurfaceArtifact>,
    resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
    pub(super) formation: Arc<NaturalFormationBundleArtifact>,
    registry: FieldRegistry,
    diagnostics: Vec<OwnedViewDiagnostic>,
    cache: FormationDisplayCache,
    elevation_display_radius_m: Option<f32>,
    area_summary: FormationAreaSummary,
    identity: SphericalFormationBuildIdentity,
}

impl SphericalFormationFieldDocument {
    /// Extracts shared artifacts and builds a fully cross-validated document.
    pub fn from_build_outcome(
        outcome: &BuildOutcome,
    ) -> Result<Self, SphericalFormationDisplayError> {
        let provenance = match outcome.verified_provenance() {
            Ok(provenance) => *provenance,
            Err(BuildOutcomeIntegrityError::MissingReportResultHash) => {
                return Err(SphericalFormationDisplayError::MissingBuildResultHash)
            }
            Err(error) => return Err(SphericalFormationDisplayError::BuildOutcomeIntegrity(error)),
        };
        Self::build(
            provenance,
            outcome.artifacts.get::<SphericalSurfaceArtifact>()?,
            outcome.artifacts.get::<ResolvedTectonicInputArtifact>()?,
            outcome.artifacts.get::<ReliefSpecArtifact>()?,
            outcome.artifacts.get::<NaturalFormationBundleArtifact>()?,
            &outcome.report,
        )
    }

    fn build(
        provenance: BuildProvenance,
        surface: Arc<SphericalSurfaceArtifact>,
        resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
        relief_spec: Arc<ReliefSpecArtifact>,
        formation: Arc<NaturalFormationBundleArtifact>,
        report: &BuildReport,
    ) -> Result<Self, SphericalFormationDisplayError> {
        surface.snapshot().validate()?;
        let authoritative = SurfaceRef::for_spherical(surface.snapshot());
        let bundle = formation.bundle();
        bundle.validate()?;
        bundle
            .tectonics()
            .compatibility()
            .validate_against(surface.snapshot())?;
        let formation_snapshot = bundle.surface_formation();
        formation_snapshot.validate()?;
        if formation_snapshot.surface_ref() != authoritative {
            return Err(SphericalFormationDisplayError::FormationSurfaceMismatch {
                snapshot: formation_snapshot.surface_ref(),
                authoritative,
            });
        }
        bundle
            .primary_relief()
            .validate_against_authoring(surface.snapshot(), relief_spec.spec())?;
        bundle
            .substrate()
            .validate_against_surface(surface.snapshot())
            .map_err(PrimaryReliefValidationError::from)?;

        let compatibility = bundle.tectonics().compatibility();
        let plate_count = u16::try_from(compatibility.plates().len())
            .map_err(|_| SphericalFormationDisplayError::PlateCountOverflow)?;
        let registry = spherical_formation_field_registry(
            plate_count,
            surface.snapshot().total_cell_area().get(),
        )?;
        let cache = FormationDisplayCache::build(surface.snapshot(), bundle.climate().fields())?;

        let terrain = formation_snapshot.terrain_fields();
        let areas = surface.snapshot().cells();
        let total_area = surface.snapshot().total_cell_area().get();
        let crust_kinds = compatibility.crust_kinds().raw_values();
        let land = terrain.land_ocean().raw_values();
        let mut continental_area = 0.0_f64;
        let mut land_area = 0.0_f64;
        for (index, cell) in areas.iter().enumerate() {
            if crust_kinds[index] == 1 {
                continental_area += cell.area.get();
            }
            if land[index] == 1 {
                land_area += cell.area.get();
            }
        }
        let area_summary = FormationAreaSummary::new(
            resolved_tectonic.input().spec().continental_crust_fraction,
            continental_area / total_area,
            relief_spec.spec().target_land_fraction,
            land_area / total_area,
            terrain.sea_level_m(),
            relief_spec.spec().sea_level_policy,
            bundle.primary_relief().water_inventory_ratio(total_area)?,
            P4WaterEnergySummary::from_budget_report(bundle.climate().budget_report()),
        );
        let elevation_display_radius_m =
            elevation_display_radius_m(terrain.sea_level_m(), terrain.current_elevation_m());
        let identity = SphericalFormationBuildIdentity::new(&provenance, authoritative);
        let document = Self {
            surface,
            resolved_tectonic,
            formation,
            registry,
            diagnostics: owned_view_diagnostics(report),
            cache,
            elevation_display_radius_m,
            area_summary,
            identity,
        };
        document.catalog()?;
        Ok(document)
    }

    /// Returns the immutable audited identity of this document.
    pub(super) const fn identity(&self) -> &SphericalFormationBuildIdentity {
        &self.identity
    }

    /// Derives the presentation identity from this document's validated build identity.
    pub fn presentation_source(&self) -> SphericalPresentationSource {
        let identity = self.identity();
        SphericalPresentationSource::new(
            identity.root_seed(),
            identity.surface_ref(),
            *identity.build_result_hash(),
            identity.graph_contract_version(),
        )
    }

    /// Borrows the sole authoritative topology used by every presentation derivative.
    pub fn surface(&self) -> &crate::world::spatial::SphericalSurfaceSnapshot {
        self.surface.snapshot()
    }

    /// Returns the quality tier the published formation product was built at.
    pub fn quality_profile(&self) -> crate::world::natural::NaturalQualityProfile {
        self.formation
            .bundle()
            .surface_formation()
            .checkpoint()
            .quality_profile()
    }

    /// Borrows the resolved tectonic input that authored this world.
    pub fn resolved_tectonic_input(&self) -> &ResolvedTectonicInput {
        self.resolved_tectonic.input()
    }

    /// Borrows the validated catalog used to prepare fill and annotation layers.
    /// Returns the evolved plate-compatibility snapshot (T1 conditioning input).
    pub fn evolved_compatibility(&self) -> &crate::world::natural::SphericalTectonicSnapshot {
        self.formation.bundle().tectonics().compatibility()
    }

    /// Returns the geologic substrate snapshot (T1 erodibility source).
    pub fn substrate(&self) -> &crate::world::natural::GeologicSubstrateSnapshot {
        self.formation.bundle().substrate()
    }

    /// Returns the published formation snapshot (T1 terrain source).
    pub fn formation_snapshot(&self) -> &crate::world::natural::NaturalSurfaceFormationSnapshot {
        self.formation.bundle().surface_formation()
    }

    /// Returns the sibling endpoint P4 snapshot used by UI and T1.
    pub fn formation_climate(&self) -> &crate::world::natural::GlobalCirculationSnapshot {
        self.formation.bundle().climate()
    }

    /// Sea level (m) and the sea-anchored hypsometric display radius (m) the
    /// cell view renders with; the amplified bake reuses both for color parity.
    pub fn amplified_color_anchors(&self) -> Option<(f32, f32)> {
        let radius = self.elevation_display_radius_m?;
        Some((self.area_summary.sea_level_m, radius))
    }

    pub fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        <Self as FieldDocument>::catalog(self)
    }

    /// Borrows immutable document diagnostics used to prepare the shared mask.
    pub fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }

    /// Borrows the build-time cached authoring-compliance measurements in O(1).
    pub const fn area_summary(&self) -> &FormationAreaSummary {
        &self.area_summary
    }

    /// Borrows the field schemas that provide presentation labels and units.
    pub(super) const fn field_registry(&self) -> &FieldRegistry {
        &self.registry
    }

    /// Returns the product-preferred initial fill field.
    pub fn preferred_field(&self) -> Option<FieldId> {
        <Self as FieldDocument>::preferred_field(self)
    }

    /// Returns the document-authoritative preferred range for a field.
    pub fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
        <Self as FieldDocument>::preferred_range(self, field)
    }
}

impl FieldDocument for SphericalFormationFieldDocument {
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
        let bundle = self.formation.bundle();
        let compatibility = bundle.tectonics().compatibility();
        let formation = bundle.surface_formation();
        let terrain = formation.terrain_fields();
        let components = terrain.elevation_components();
        let rates = formation.process_rates();
        let hydrology = formation.hydrology();
        let payloads: Vec<(FieldId, FieldPayloadRef<'_>)> = vec![
            (
                plate_id_field_id(),
                FieldPayloadRef::CategoryU32(compatibility.cell_plates().raw_values()),
            ),
            (
                crust_kind_field_id(),
                FieldPayloadRef::CategoryU32(compatibility.crust_kinds().raw_values()),
            ),
            (
                crust_thickness_field_id(),
                FieldPayloadRef::ScalarF32(compatibility.crust_thickness_km()),
            ),
            (
                ocean_age_myr_field_id(),
                FieldPayloadRef::ScalarF32(compatibility.crust_age_myr()),
            ),
            (
                primary_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(components.primary_elevation_m()),
            ),
            (
                tectonic_displacement_m_field_id(),
                FieldPayloadRef::ScalarF32(components.tectonic_displacement_m()),
            ),
            (
                fluvial_erosion_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(components.fluvial_erosion_m()),
            ),
            (
                hillslope_erosion_m_field_id(),
                FieldPayloadRef::ScalarF32(components.hillslope_erosion_m()),
            ),
            (
                hillslope_deposition_m_field_id(),
                FieldPayloadRef::ScalarF32(components.hillslope_deposition_m()),
            ),
            (
                routed_sediment_deposition_m_field_id(),
                FieldPayloadRef::ScalarF32(components.routed_sediment_deposition_m()),
            ),
            (
                coastal_erosion_m_field_id(),
                FieldPayloadRef::ScalarF32(components.coastal_erosion_m()),
            ),
            (
                coastal_deposition_m_field_id(),
                FieldPayloadRef::ScalarF32(components.coastal_deposition_m()),
            ),
            (
                isostatic_response_m_field_id(),
                FieldPayloadRef::ScalarF32(components.isostatic_response_m()),
            ),
            (
                tectonic_displacement_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.tectonic_displacement_rate_m_per_year()),
            ),
            (
                fluvial_erosion_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.fluvial_erosion_rate_m_per_year()),
            ),
            (
                hillslope_erosion_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.hillslope_erosion_rate_m_per_year()),
            ),
            (
                hillslope_deposition_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.hillslope_deposition_rate_m_per_year()),
            ),
            (
                routed_sediment_deposition_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.routed_sediment_deposition_rate_m_per_year()),
            ),
            (
                coastal_erosion_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.coastal_erosion_rate_m_per_year()),
            ),
            (
                coastal_deposition_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.coastal_deposition_rate_m_per_year()),
            ),
            (
                isostatic_response_rate_m_per_year_field_id(),
                FieldPayloadRef::ScalarF32(rates.isostatic_response_rate_m_per_year()),
            ),
            (
                sediment_deposition_thickness_m_field_id(),
                FieldPayloadRef::ScalarF32(terrain.sediment().sediment_thickness_m()),
            ),
            (
                surface_elevation_m_field_id(),
                FieldPayloadRef::ScalarF32(terrain.current_elevation_m()),
            ),
            (
                land_ocean_field_id(),
                FieldPayloadRef::CategoryU32(terrain.land_ocean().raw_values()),
            ),
            (
                circulation_annual_evaporation_mm_field_id(),
                FieldPayloadRef::ScalarF32(&self.cache.annual_evaporation_mm),
            ),
            (
                circulation_annual_precipitation_mm_field_id(),
                FieldPayloadRef::ScalarF32(&self.cache.annual_precipitation_mm),
            ),
            (
                circulation_mean_absorbed_shortwave_w_m2_field_id(),
                FieldPayloadRef::ScalarF32(&self.cache.mean_absorbed_shortwave_w_m2),
            ),
            (
                circulation_mean_air_temperature_c_field_id(),
                FieldPayloadRef::ScalarF32(&self.cache.mean_air_temperature_c),
            ),
            (
                circulation_mean_outgoing_longwave_w_m2_field_id(),
                FieldPayloadRef::ScalarF32(&self.cache.mean_outgoing_longwave_w_m2),
            ),
            (
                circulation_prevailing_wind_m_s_field_id(),
                FieldPayloadRef::Vector2F32(&self.cache.prevailing_wind_m_s),
            ),
            (
                circulation_surface_albedo_field_id(),
                FieldPayloadRef::ScalarF32(
                    self.formation.bundle().climate().fields().surface_albedo(),
                ),
            ),
            (
                annual_local_runoff_mm_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.annual_local_runoff_mm()),
            ),
            (
                lake_depth_m_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.lake_depth_m()),
            ),
            (
                surface_water_kind_field_id(),
                FieldPayloadRef::CategoryU32(hydrology.surface_water().raw_values()),
            ),
            (
                mean_annual_discharge_m3_s_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.mean_annual_discharge_m3_s()),
            ),
            (
                drainage_area_km2_field_id(),
                FieldPayloadRef::ScalarF32(hydrology.drainage_area_km2()),
            ),
            (
                strahler_stream_order_field_id(),
                FieldPayloadRef::CategoryU32(hydrology.strahler_order().raw_values()),
            ),
        ];
        FieldCatalog::from_payloads(&self.registry, payloads)
    }

    fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }

    fn preferred_field(&self) -> Option<FieldId> {
        Some(surface_elevation_m_field_id())
    }

    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
        formation_preferred_range(
            &self.registry,
            self.formation
                .bundle()
                .surface_formation()
                .terrain_fields()
                .sea_level_m(),
            self.elevation_display_radius_m,
            field,
        )
    }
}

impl crate::app::field_document::SphericalFieldLayerDocument for SphericalFormationFieldDocument {
    fn presentation_source(&self) -> SphericalPresentationSource {
        self.presentation_source()
    }

    fn spherical_cell_count(&self) -> usize {
        self.surface.snapshot().cells().len()
    }

    fn spherical_edge_count(&self) -> usize {
        self.surface.snapshot().edges().len()
    }
}

/// Returns the shared formation-field range preference for the atomic surface.
fn formation_preferred_range(
    registry: &FieldRegistry,
    sea_level_m: f32,
    elevation_display_radius_m: Option<f32>,
    field: &FieldId,
) -> Option<DisplayRangeMode> {
    if [
        annual_local_runoff_mm_field_id(),
        circulation_annual_evaporation_mm_field_id(),
        circulation_annual_precipitation_mm_field_id(),
        circulation_mean_absorbed_shortwave_w_m2_field_id(),
        circulation_mean_air_temperature_c_field_id(),
        circulation_mean_outgoing_longwave_w_m2_field_id(),
        circulation_surface_albedo_field_id(),
        coastal_deposition_m_field_id(),
        coastal_deposition_rate_m_per_year_field_id(),
        coastal_erosion_m_field_id(),
        coastal_erosion_rate_m_per_year_field_id(),
        drainage_area_km2_field_id(),
        fluvial_erosion_rate_m_per_year_field_id(),
        fluvial_erosion_depth_m_field_id(),
        hillslope_deposition_m_field_id(),
        hillslope_deposition_rate_m_per_year_field_id(),
        hillslope_erosion_m_field_id(),
        hillslope_erosion_rate_m_per_year_field_id(),
        isostatic_response_m_field_id(),
        isostatic_response_rate_m_per_year_field_id(),
        lake_depth_m_field_id(),
        mean_annual_discharge_m3_s_field_id(),
        ocean_age_myr_field_id(),
        routed_sediment_deposition_m_field_id(),
        routed_sediment_deposition_rate_m_per_year_field_id(),
        sediment_deposition_thickness_m_field_id(),
        tectonic_displacement_m_field_id(),
        tectonic_displacement_rate_m_per_year_field_id(),
    ]
    .contains(field)
    {
        return Some(DisplayRangeMode::Data);
    }
    (field == &surface_elevation_m_field_id() || field == &primary_elevation_m_field_id())
        .then_some(())?;
    registry.get(field)?;
    let radius = elevation_display_radius_m?;
    ValueRange::new(sea_level_m - radius, sea_level_m + radius)
        .ok()
        .map(DisplayRangeMode::Manual)
}

/// Errors returned while composing a complete formation-product document.
#[derive(Debug, Error)]
pub enum SphericalFormationDisplayError {
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    BuildOutcomeIntegrity(BuildOutcomeIntegrityError),
    #[error(transparent)]
    Surface(#[from] SphericalSurfaceValidationError),
    #[error(transparent)]
    Tectonic(#[from] SphericalTectonicValidationError),
    #[error(transparent)]
    Formation(#[from] SurfaceFormationValidationError),
    #[error(transparent)]
    FormationBundle(#[from] NaturalFormationBundleValidationError),
    #[error(transparent)]
    PrimaryRelief(#[from] PrimaryReliefValidationError),
    #[error(transparent)]
    Registry(#[from] NaturalFieldRegistryError),
    #[error(transparent)]
    FieldView(#[from] FieldViewError),
    #[error("successful formation build report is missing its result hash")]
    MissingBuildResultHash,
    #[error("formation plate count cannot be represented by the field registry")]
    PlateCountOverflow,
    #[error(
        "published circulation covers {circulation_cells} cells but the surface has {surface_cells}"
    )]
    CirculationCardinality {
        circulation_cells: usize,
        surface_cells: usize,
    },
    #[error("{field} reduction at cell {cell} cannot be represented as finite f32")]
    ReductionOverflow { field: &'static str, cell: usize },
    #[error(
        "formation product {snapshot:?} does not match authoritative surface {authoritative:?}"
    )]
    FormationSurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
}

#[cfg(test)]
mod tests {
    use super::{display_annual_water_total_mm, SphericalFormationDisplayError};
    use crate::world::natural::{
        climatological_annual_total_mm, ANNUAL_PRECIPITATION_MAX_MM, CLIMATE_MONTH_COUNT,
    };

    #[test]
    fn displayed_annual_water_total_preserves_values_above_the_legacy_p5_envelope() {
        for (field, monthly) in [
            (
                "circulation_annual_precipitation_mm",
                [100.0; CLIMATE_MONTH_COUNT],
            ),
            (
                "circulation_annual_evaporation_mm",
                [75.0; CLIMATE_MONTH_COUNT],
            ),
        ] {
            let displayed = display_annual_water_total_mm(field, 0, &monthly).unwrap();

            assert!(displayed > ANNUAL_PRECIPITATION_MAX_MM);
            assert_eq!(
                displayed.to_bits(),
                (climatological_annual_total_mm(&monthly) as f32).to_bits()
            );
        }
    }

    #[test]
    fn displayed_annual_water_total_reports_f32_overflow_without_clamping() {
        let monthly = [f32::MAX; CLIMATE_MONTH_COUNT];

        assert!(matches!(
            display_annual_water_total_mm("circulation_annual_precipitation_mm", 7, &monthly),
            Err(SphericalFormationDisplayError::ReductionOverflow {
                field: "circulation_annual_precipitation_mm",
                cell: 7,
            })
        ));
    }
}
