use thiserror::Error;

use super::super::surface_water_geometry::SurfaceWaterWorkingGeometry;
use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, FormationElevationComponents, FormationSedimentFields,
    FormationTerrainFields, PrimaryReliefSnapshot, SurfaceFormationValidationError,
    WaterVolumeSolveError, ELEVATION_MAX_M, ELEVATION_MIN_M, FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
    SEDIMENT_PROVENANCE_SOURCE_COUNT,
};
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::CellId;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SedimentStockState {
    mass_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

impl SedimentStockState {
    fn empty(cell_count: usize) -> Self {
        Self {
            mass_by_source_kg: vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; cell_count],
        }
    }

    pub(super) fn from_mass_by_source_kg(
        mass_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    ) -> Result<Self, FormationStateError> {
        for (cell, values) in mass_by_source_kg.iter().enumerate() {
            for (source_index, &found) in values.iter().enumerate() {
                if !found.is_finite() || found < 0.0 {
                    return Err(FormationStateError::SedimentMassOutOfRange {
                        cell: CellId::from_raw(cell as u32),
                        source_index,
                        found,
                    });
                }
            }
            let total = values.iter().sum::<f64>();
            if !total.is_finite() {
                return Err(FormationStateError::SedimentMassOutOfRange {
                    cell: CellId::from_raw(cell as u32),
                    source_index: 0,
                    found: total,
                });
            }
        }
        Ok(Self { mass_by_source_kg })
    }

    pub(super) fn apply_transfer(
        &mut self,
        removed_by_source_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
        deposited_by_source_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    ) -> Result<(), FormationStateError> {
        if removed_by_source_kg.len() != self.mass_by_source_kg.len()
            || deposited_by_source_kg.len() != self.mass_by_source_kg.len()
        {
            return Err(FormationStateError::SedimentLengthMismatch {
                expected: self.mass_by_source_kg.len(),
                removed: removed_by_source_kg.len(),
                deposited: deposited_by_source_kg.len(),
            });
        }
        let mut candidate = self.mass_by_source_kg.clone();
        for cell in 0..candidate.len() {
            for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
                let removed = removed_by_source_kg[cell][source];
                let deposited = deposited_by_source_kg[cell][source];
                let next = candidate[cell][source] - removed + deposited;
                if !removed.is_finite()
                    || !deposited.is_finite()
                    || removed < 0.0
                    || deposited < 0.0
                    || !next.is_finite()
                    || next < 0.0
                {
                    return Err(FormationStateError::SedimentMassOutOfRange {
                        cell: CellId::from_raw(cell as u32),
                        source_index: source,
                        found: next,
                    });
                }
                candidate[cell][source] = next;
            }
        }
        self.mass_by_source_kg = candidate;
        Ok(())
    }

    pub(super) fn to_wire_fields(
        &self,
        cell_area_m2: &[f64],
        bulk_density_kg_m3: &[f64],
    ) -> Result<FormationSedimentFields, FormationStateError> {
        let count = self.mass_by_source_kg.len();
        for (field, found) in [
            ("cell_area_m2", cell_area_m2.len()),
            ("bulk_density_kg_m3", bulk_density_kg_m3.len()),
        ] {
            if found != count {
                return Err(FormationStateError::LengthMismatch {
                    field,
                    expected: count,
                    found,
                });
            }
        }
        let mut sediment_thickness_m = Vec::with_capacity(count);
        let mut provenance_fraction = Vec::with_capacity(count);
        for (index, mass_by_source_kg) in self.mass_by_source_kg.iter().enumerate() {
            let area_m2 = cell_area_m2[index];
            let density_kg_m3 = bulk_density_kg_m3[index];
            if !area_m2.is_finite() || area_m2 <= 0.0 {
                return Err(FormationStateError::InvalidSedimentProjectionInput {
                    field: "cell_area_m2",
                    cell: CellId::from_raw(index as u32),
                    found: area_m2,
                });
            }
            if !density_kg_m3.is_finite() || density_kg_m3 <= 0.0 {
                return Err(FormationStateError::InvalidSedimentProjectionInput {
                    field: "bulk_density_kg_m3",
                    cell: CellId::from_raw(index as u32),
                    found: density_kg_m3,
                });
            }
            let total_mass_kg = mass_by_source_kg.iter().sum::<f64>();
            if !total_mass_kg.is_finite() || total_mass_kg < 0.0 {
                return Err(FormationStateError::InvalidSedimentProjectionInput {
                    field: "total_mass_kg",
                    cell: CellId::from_raw(index as u32),
                    found: total_mass_kg,
                });
            }
            let thickness_m = total_mass_kg / (area_m2 * density_kg_m3);
            sediment_thickness_m.push(if total_mass_kg > 0.0 && thickness_m as f32 == 0.0 {
                f32::from_bits(1)
            } else {
                thickness_m as f32
            });
            provenance_fraction.push(if total_mass_kg == 0.0 {
                [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]
            } else {
                mass_by_source_kg.map(|mass_kg| (mass_kg / total_mass_kg) as f32)
            });
        }
        let zero_f64 = vec![0.0; count];
        Ok(FormationSedimentFields::new(
            sediment_thickness_m,
            provenance_fraction,
            zero_f64.clone(),
            zero_f64.clone(),
            zero_f64.clone(),
            zero_f64,
            vec![0.0; count],
        )?)
    }

    pub(super) fn as_slice(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.mass_by_source_kg
    }

    #[cfg(test)]
    fn mass_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.mass_by_source_kg
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::generators::natural) struct FormationState {
    primary_elevation_m: Vec<f64>,
    tectonic_displacement_m: Vec<f64>,
    fluvial_erosion_m: Vec<f64>,
    hillslope_erosion_m: Vec<f64>,
    hillslope_deposition_m: Vec<f64>,
    routed_sediment_deposition_m: Vec<f64>,
    coastal_erosion_m: Vec<f64>,
    coastal_deposition_m: Vec<f64>,
    isostatic_response_m: Vec<f64>,
    current_elevation_m: Vec<f64>,
    sediment_stock: SedimentStockState,
    surface_water_geometry: SurfaceWaterWorkingGeometry,
}

impl FormationState {
    pub(super) fn from_legacy_primary_wire_for_migration(
        primary: &PrimaryReliefSnapshot,
    ) -> Result<Self, FormationStateError> {
        let primary_elevation_m = primary
            .elevation_m()
            .iter()
            .copied()
            .map(f64::from)
            .collect();
        let count = primary.elevation_m().len();
        let mut state = Self {
            primary_elevation_m,
            tectonic_displacement_m: vec![0.0; count],
            fluvial_erosion_m: vec![0.0; count],
            hillslope_erosion_m: vec![0.0; count],
            hillslope_deposition_m: vec![0.0; count],
            routed_sediment_deposition_m: vec![0.0; count],
            coastal_erosion_m: vec![0.0; count],
            coastal_deposition_m: vec![0.0; count],
            isostatic_response_m: vec![0.0; count],
            current_elevation_m: vec![0.0; count],
            sediment_stock: SedimentStockState::empty(count),
            surface_water_geometry: SurfaceWaterWorkingGeometry::from_wire_for_migration(
                primary.surface_water_geometry(),
            ),
        };
        state.rebuild_and_validate()?;
        Ok(state)
    }

    #[cfg(test)]
    pub(in crate::generators::natural) fn apply_tectonic_displacement_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::TectonicDisplacement, increments_m)
    }

    /// Replaces the exact tectonic component with an analytically integrated target.
    pub(in crate::generators::natural) fn replace_tectonic_displacement_f64(
        &mut self,
        targets_m: &[f64],
    ) -> Result<(), FormationStateError> {
        let expected = self.primary_elevation_m.len();
        if targets_m.len() != expected {
            return Err(FormationStateError::LengthMismatch {
                field: "tectonic_displacement_m",
                expected,
                found: targets_m.len(),
            });
        }
        for (index, &target_m) in targets_m.iter().enumerate() {
            if !target_m.is_finite() {
                return Err(FormationStateError::InvalidComponent {
                    field: "tectonic_displacement_m",
                    cell: CellId::from_raw(index as u32),
                    found: target_m,
                });
            }
            validate_elevation(
                index,
                self.elevation_with(index, ComponentKind::TectonicDisplacement, target_m),
            )?;
        }
        self.tectonic_displacement_m.clone_from_slice(targets_m);
        for index in 0..expected {
            self.current_elevation_m[index] = self.elevation_at(index);
        }
        Ok(())
    }

    pub(in crate::generators::natural) fn apply_fluvial_erosion_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::FluvialErosion, increments_m)
    }

    pub(in crate::generators::natural) fn apply_hillslope_erosion_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::HillslopeErosion, increments_m)
    }

    pub(in crate::generators::natural) fn apply_hillslope_deposition_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::HillslopeDeposition, increments_m)
    }

    pub(in crate::generators::natural) fn apply_routed_sediment_deposition_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::RoutedSedimentDeposition, increments_m)
    }

    pub(in crate::generators::natural) fn apply_coastal_erosion_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::CoastalErosion, increments_m)
    }

    pub(in crate::generators::natural) fn apply_coastal_deposition_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::CoastalDeposition, increments_m)
    }

    pub(in crate::generators::natural) fn apply_isostatic_response_f64(
        &mut self,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        self.apply_component(ComponentKind::IsostaticResponse, increments_m)
    }

    fn apply_component(
        &mut self,
        component: ComponentKind,
        increments_m: &[f64],
    ) -> Result<(), FormationStateError> {
        let expected = self.primary_elevation_m.len();
        if increments_m.len() != expected {
            return Err(FormationStateError::LengthMismatch {
                field: component.field_name(),
                expected,
                found: increments_m.len(),
            });
        }
        for (index, &increment_m) in increments_m.iter().enumerate() {
            let updated = component.value(self, index) + increment_m;
            if !increment_m.is_finite()
                || !updated.is_finite()
                || (component.is_nonnegative() && (increment_m < 0.0 || updated < 0.0))
            {
                return Err(FormationStateError::InvalidComponent {
                    field: component.field_name(),
                    cell: CellId::from_raw(index as u32),
                    found: updated,
                });
            }
            validate_elevation(index, self.elevation_with(index, component, updated))?;
        }
        for (index, &increment_m) in increments_m.iter().enumerate() {
            component.add(self, index, increment_m);
            self.current_elevation_m[index] = self.elevation_at(index);
        }
        Ok(())
    }

    fn elevation_with(&self, index: usize, component: ComponentKind, candidate: f64) -> f64 {
        formation_elevation_from_components(
            self.primary_elevation_m[index],
            component.select(
                ComponentKind::TectonicDisplacement,
                candidate,
                self.tectonic_displacement_m[index],
            ),
            component.select(
                ComponentKind::FluvialErosion,
                candidate,
                self.fluvial_erosion_m[index],
            ),
            component.select(
                ComponentKind::HillslopeErosion,
                candidate,
                self.hillslope_erosion_m[index],
            ),
            component.select(
                ComponentKind::HillslopeDeposition,
                candidate,
                self.hillslope_deposition_m[index],
            ),
            component.select(
                ComponentKind::RoutedSedimentDeposition,
                candidate,
                self.routed_sediment_deposition_m[index],
            ),
            component.select(
                ComponentKind::CoastalErosion,
                candidate,
                self.coastal_erosion_m[index],
            ),
            component.select(
                ComponentKind::CoastalDeposition,
                candidate,
                self.coastal_deposition_m[index],
            ),
            component.select(
                ComponentKind::IsostaticResponse,
                candidate,
                self.isostatic_response_m[index],
            ),
        )
    }

    fn elevation_at(&self, index: usize) -> f64 {
        formation_elevation_from_components(
            self.primary_elevation_m[index],
            self.tectonic_displacement_m[index],
            self.fluvial_erosion_m[index],
            self.hillslope_erosion_m[index],
            self.hillslope_deposition_m[index],
            self.routed_sediment_deposition_m[index],
            self.coastal_erosion_m[index],
            self.coastal_deposition_m[index],
            self.isostatic_response_m[index],
        )
    }

    fn rebuild_and_validate(&mut self) -> Result<(), FormationStateError> {
        for index in 0..self.current_elevation_m.len() {
            let elevation_m = self.elevation_at(index);
            validate_elevation(index, elevation_m)?;
            self.current_elevation_m[index] = elevation_m;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn replace_primary_for_offline_reference(
        &mut self,
        old_primary_elevation_m: &[f64],
        new_primary_elevation_m: &[f64],
    ) -> Result<(), FormationStateError> {
        let expected = self.primary_elevation_m.len();
        for (field, found) in [
            ("old_primary_elevation_m", old_primary_elevation_m.len()),
            ("new_primary_elevation_m", new_primary_elevation_m.len()),
        ] {
            if found != expected {
                return Err(FormationStateError::LengthMismatch {
                    field,
                    expected,
                    found,
                });
            }
        }
        for index in 0..expected {
            if old_primary_elevation_m[index].to_bits() != self.primary_elevation_m[index].to_bits()
            {
                return Err(FormationStateError::PrimaryReferenceMismatch {
                    cell: CellId::from_raw(index as u32),
                    stored: self.primary_elevation_m[index],
                    supplied: old_primary_elevation_m[index],
                });
            }
            let candidate = new_primary_elevation_m[index];
            if !candidate.is_finite() {
                return Err(FormationStateError::InvalidComponent {
                    field: "primary_elevation_m",
                    cell: CellId::from_raw(index as u32),
                    found: candidate,
                });
            }
            let elevation =
                self.current_elevation_m[index] - self.primary_elevation_m[index] + candidate;
            validate_elevation(index, elevation)?;
        }
        for (index, &candidate) in new_primary_elevation_m.iter().enumerate() {
            self.primary_elevation_m[index] = candidate;
            self.current_elevation_m[index] = self.elevation_at(index);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::generators::natural) fn primary_elevation_m(&self) -> &[f64] {
        &self.primary_elevation_m
    }

    #[cfg(test)]
    pub(in crate::generators::natural) fn fluvial_erosion_m(&self) -> &[f64] {
        &self.fluvial_erosion_m
    }

    #[cfg(test)]
    pub(in crate::generators::natural) fn routed_sediment_deposition_m(&self) -> &[f64] {
        &self.routed_sediment_deposition_m
    }

    #[cfg(test)]
    pub(in crate::generators::natural) fn isostatic_response_m(&self) -> &[f64] {
        &self.isostatic_response_m
    }

    pub(in crate::generators::natural) fn current_elevation_exact_m(&self) -> &[f64] {
        &self.current_elevation_m
    }

    /// Returns the exact accumulated tectonic displacement in metres.
    pub(in crate::generators::natural) fn tectonic_displacement_m(&self) -> &[f64] {
        &self.tectonic_displacement_m
    }

    fn wire_components(&self) -> Result<FormationElevationComponents, FormationStateError> {
        let primary_elevation_m = quantize(&self.primary_elevation_m);
        let tectonic_displacement_m = quantize(&self.tectonic_displacement_m);
        let fluvial_erosion_m = quantize(&self.fluvial_erosion_m);
        let hillslope_erosion_m = quantize(&self.hillslope_erosion_m);
        let hillslope_deposition_m = quantize(&self.hillslope_deposition_m);
        let routed_sediment_deposition_m = quantize(&self.routed_sediment_deposition_m);
        let coastal_erosion_m = quantize(&self.coastal_erosion_m);
        let coastal_deposition_m = quantize(&self.coastal_deposition_m);
        let isostatic_response_m = quantize(&self.isostatic_response_m);
        let final_elevation_m = (0..self.current_elevation_m.len())
            .map(|index| {
                formation_elevation_from_components(
                    f64::from(primary_elevation_m[index]),
                    f64::from(tectonic_displacement_m[index]),
                    f64::from(fluvial_erosion_m[index]),
                    f64::from(hillslope_erosion_m[index]),
                    f64::from(hillslope_deposition_m[index]),
                    f64::from(routed_sediment_deposition_m[index]),
                    f64::from(coastal_erosion_m[index]),
                    f64::from(coastal_deposition_m[index]),
                    f64::from(isostatic_response_m[index]),
                ) as f32
            })
            .collect();
        Ok(FormationElevationComponents::new(
            primary_elevation_m,
            tectonic_displacement_m,
            fluvial_erosion_m,
            hillslope_erosion_m,
            hillslope_deposition_m,
            routed_sediment_deposition_m,
            coastal_erosion_m,
            coastal_deposition_m,
            isostatic_response_m,
            final_elevation_m,
        )?)
    }

    /// Projects the accepted exact state into the sole final wire terrain.
    pub(super) fn project_final_terrain(
        &self,
        surface: &SphericalSurfaceSnapshot,
        sediment: FormationSedimentFields,
        cancellation: &BuildCancellation,
    ) -> Result<FormationTerrainFields, FormationStateError> {
        let components = self.wire_components()?;
        let water = self.surface_water_geometry.to_wire(
            surface,
            components.final_elevation_m(),
            cancellation,
        )?;
        Ok(FormationTerrainFields::new(
            FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
            components,
            water,
            self.surface_water_geometry.total_water_volume_m3(),
            sediment,
        )?)
    }

    pub(super) const fn surface_water_geometry(&self) -> &SurfaceWaterWorkingGeometry {
        &self.surface_water_geometry
    }

    pub(super) const fn sediment_stock(&self) -> &SedimentStockState {
        &self.sediment_stock
    }

    pub(super) fn sediment_stock_mut(&mut self) -> &mut SedimentStockState {
        &mut self.sediment_stock
    }

    pub(super) fn replace_surface_water_geometry(
        &mut self,
        surface_water_geometry: SurfaceWaterWorkingGeometry,
    ) {
        self.surface_water_geometry = surface_water_geometry;
    }
}

fn quantize(values: &[f64]) -> Vec<f32> {
    values.iter().copied().map(|value| value as f32).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComponentKind {
    TectonicDisplacement,
    FluvialErosion,
    HillslopeErosion,
    HillslopeDeposition,
    RoutedSedimentDeposition,
    CoastalErosion,
    CoastalDeposition,
    IsostaticResponse,
}

impl ComponentKind {
    const fn field_name(self) -> &'static str {
        match self {
            Self::TectonicDisplacement => "tectonic_displacement_m",
            Self::FluvialErosion => "fluvial_erosion_m",
            Self::HillslopeErosion => "hillslope_erosion_m",
            Self::HillslopeDeposition => "hillslope_deposition_m",
            Self::RoutedSedimentDeposition => "routed_sediment_deposition_m",
            Self::CoastalErosion => "coastal_erosion_m",
            Self::CoastalDeposition => "coastal_deposition_m",
            Self::IsostaticResponse => "isostatic_response_m",
        }
    }

    const fn is_nonnegative(self) -> bool {
        !matches!(self, Self::TectonicDisplacement | Self::IsostaticResponse)
    }

    fn value(self, state: &FormationState, index: usize) -> f64 {
        match self {
            Self::TectonicDisplacement => state.tectonic_displacement_m[index],
            Self::FluvialErosion => state.fluvial_erosion_m[index],
            Self::HillslopeErosion => state.hillslope_erosion_m[index],
            Self::HillslopeDeposition => state.hillslope_deposition_m[index],
            Self::RoutedSedimentDeposition => state.routed_sediment_deposition_m[index],
            Self::CoastalErosion => state.coastal_erosion_m[index],
            Self::CoastalDeposition => state.coastal_deposition_m[index],
            Self::IsostaticResponse => state.isostatic_response_m[index],
        }
    }

    fn add(self, state: &mut FormationState, index: usize, increment_m: f64) {
        match self {
            Self::TectonicDisplacement => state.tectonic_displacement_m[index] += increment_m,
            Self::FluvialErosion => state.fluvial_erosion_m[index] += increment_m,
            Self::HillslopeErosion => state.hillslope_erosion_m[index] += increment_m,
            Self::HillslopeDeposition => state.hillslope_deposition_m[index] += increment_m,
            Self::RoutedSedimentDeposition => {
                state.routed_sediment_deposition_m[index] += increment_m;
            }
            Self::CoastalErosion => state.coastal_erosion_m[index] += increment_m,
            Self::CoastalDeposition => state.coastal_deposition_m[index] += increment_m,
            Self::IsostaticResponse => state.isostatic_response_m[index] += increment_m,
        }
    }

    const fn select(self, selected: Self, candidate: f64, current: f64) -> f64 {
        if self as u8 == selected as u8 {
            candidate
        } else {
            current
        }
    }
}

fn validate_elevation(index: usize, elevation_m: f64) -> Result<(), FormationStateError> {
    if elevation_m.is_finite()
        && (f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&elevation_m)
    {
        Ok(())
    } else {
        Err(FormationStateError::ElevationOutOfRange {
            cell: CellId::from_raw(index as u32),
            found: elevation_m,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(in crate::generators::natural) enum FormationStateError {
    #[error("formation field {field} has length {found}; expected {expected}")]
    LengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("formation component {field} at {cell:?} is invalid: {found}")]
    InvalidComponent {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error(
        "sediment transfer lengths are removed={removed}, deposited={deposited}; expected {expected}"
    )]
    SedimentLengthMismatch {
        expected: usize,
        removed: usize,
        deposited: usize,
    },
    #[error("sediment mass at {cell:?}, source {source_index} is invalid after transfer: {found}")]
    SedimentMassOutOfRange {
        cell: CellId,
        source_index: usize,
        found: f64,
    },
    #[error("sediment projection input {field} at {cell:?} is invalid: {found}")]
    InvalidSedimentProjectionInput {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error(
        "offline reference primary at {cell:?} is {supplied}, but retained state stores {stored}"
    )]
    #[cfg(test)]
    PrimaryReferenceMismatch {
        cell: CellId,
        stored: f64,
        supplied: f64,
    },
    #[error(transparent)]
    InvalidWire(#[from] SurfaceFormationValidationError),
    #[error(transparent)]
    WaterProjection(#[from] WaterVolumeSolveError),
    #[error("formation elevation at {cell:?} is outside the supported domain: {found}")]
    ElevationOutOfRange { cell: CellId, found: f64 },
}

#[cfg(test)]
pub(super) fn formation_state_for_value(value_m: f64) -> FormationState {
    use super::super::surface_water_geometry::solve_physical_sea_level_exact;
    use crate::engine::BuildCancellation;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::{Meters, SphericalSpaceSpec};

    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(10_000.0).expect("test radius is positive"),
        target_cell_count: 42,
    })
    .expect("the production small-sphere fixture is valid");
    let primary_elevation_m = vec![value_m; surface.cells().len()];
    let surface_water_geometry = solve_physical_sea_level_exact(
        &surface,
        &primary_elevation_m,
        0.0,
        &BuildCancellation::new(),
    )
    .expect("the zero-inventory fixture has an exact water solution")
    .into_geometry();
    let count = primary_elevation_m.len();
    FormationState {
        current_elevation_m: primary_elevation_m.clone(),
        primary_elevation_m,
        tectonic_displacement_m: vec![0.0; count],
        fluvial_erosion_m: vec![0.0; count],
        hillslope_erosion_m: vec![0.0; count],
        hillslope_deposition_m: vec![0.0; count],
        routed_sediment_deposition_m: vec![0.0; count],
        coastal_erosion_m: vec![0.0; count],
        coastal_deposition_m: vec![0.0; count],
        isostatic_response_m: vec![0.0; count],
        sediment_stock: SedimentStockState::empty(count),
        surface_water_geometry,
    }
}

#[cfg(test)]
mod tests {
    use super::{formation_state_for_value, FormationStateError, SedimentStockState};

    #[test]
    fn sub_wire_sediment_mass_survives_repeated_steps() {
        let mut stock = SedimentStockState::empty(1);
        for _ in 0..1_000 {
            stock
                .apply_transfer(&[[0.0, 0.0, 0.0, 0.0, 0.0]], &[[0.01, 0.0, 0.0, 0.0, 0.0]])
                .unwrap();
        }

        assert!((stock.mass_by_source_kg()[0][0] - 10.0).abs() <= 1.0e-12);
    }

    #[test]
    fn wire_projection_never_becomes_the_next_stock() {
        let mut stock = SedimentStockState::empty(1);
        stock
            .apply_transfer(
                &[[0.0, 0.0, 0.0, 0.0, 0.0]],
                &[[0.01, 0.02, 0.03, 0.04, 0.05]],
            )
            .unwrap();

        let _wire = stock.to_wire_fields(&[1_000_000.0], &[2_000.0]).unwrap();

        assert_eq!(stock.mass_by_source_kg()[0], [0.01, 0.02, 0.03, 0.04, 0.05]);
    }

    #[test]
    fn sub_ulp_surface_changes_accumulate_without_f32_feedback() {
        let mut state = formation_state_for_value(9_000.0);
        let increment = vec![0.0003; state.current_elevation_exact_m().len()];

        state.apply_fluvial_erosion_f64(&increment).unwrap();
        state.apply_fluvial_erosion_f64(&increment).unwrap();

        assert_eq!(state.fluvial_erosion_m()[0].to_bits(), 0.0006_f64.to_bits());
        assert!(state.current_elevation_exact_m()[0] < 9_000.0);
    }

    #[test]
    fn offline_reference_primary_replacement_preserves_accumulated_components() {
        let mut state = formation_state_for_value(100.0);
        let count = state.current_elevation_exact_m().len();

        state
            .apply_routed_sediment_deposition_f64(&vec![12.0; count])
            .unwrap();
        state
            .replace_primary_for_offline_reference(&vec![100.0; count], &vec![130.0; count])
            .unwrap();

        assert_eq!(state.primary_elevation_m()[0], 130.0);
        assert_eq!(state.routed_sediment_deposition_m()[0], 12.0);
        assert_eq!(state.current_elevation_exact_m()[0], 142.0);
    }

    #[test]
    fn exact_f64_state_rejects_a_true_overflow_before_wire_rounding() {
        let mut state =
            formation_state_for_value(f64::from(crate::world::natural::ELEVATION_MAX_M));
        let count = state.current_elevation_exact_m().len();

        let result = state.apply_tectonic_displacement_f64(&vec![0.000_01; count]);

        assert!(matches!(
            result,
            Err(FormationStateError::ElevationOutOfRange { found, .. })
                if found > f64::from(crate::world::natural::ELEVATION_MAX_M)
        ));
    }
}
