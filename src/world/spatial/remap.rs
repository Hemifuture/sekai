use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{SurfaceRef, SurfaceRefError};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT};

/// The only supported conservative spherical-surface map schema.
pub const CONSERVATIVE_SURFACE_MAP_SCHEMA_V1: u16 = 1;

const MAX_REMAP_CELL_AREAS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_REMAP_ROW_OFFSETS: usize = MAX_REMAP_CELL_AREAS + 1;
const MAX_REMAP_OVERLAPS: usize = MAX_REMAP_CELL_AREAS * 24;
const MAX_MARGIN_RELATIVE_ERROR: f64 = 1.0e-10;
const MAX_BALANCE_ITERATIONS: u16 = 96;
const MAX_RELATIVE_GEOMETRIC_ADJUSTMENT: f64 = 1.0e-4;
const TANGENT_COEFFICIENT_TOLERANCE: f64 = 32.0 * f64::EPSILON;

/// A source east/north to target east/north tangent-plane transform.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TangentTransform {
    coefficients: [f64; 4],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TangentTransformWire {
    coefficients: [f64; 4],
}

impl TangentTransform {
    /// Constructs a transform from row-major two-by-two coefficients.
    pub fn new(coefficients: [f64; 4]) -> Result<Self, ConservativeSurfaceMapError> {
        if let Some((index, found)) = coefficients.iter().copied().enumerate().find(|(_, value)| {
            !value.is_finite() || value.abs() > 1.0 + TANGENT_COEFFICIENT_TOLERANCE
        }) {
            return Err(ConservativeSurfaceMapError::InvalidTangentCoefficient { index, found });
        }
        Ok(Self { coefficients })
    }

    /// Returns the exact identity transform.
    pub const fn identity() -> Self {
        Self {
            coefficients: [1.0, 0.0, 0.0, 1.0],
        }
    }

    /// Returns the row-major transform coefficients.
    pub const fn coefficients(self) -> [f64; 4] {
        self.coefficients
    }

    /// Applies the transform to source east/north components.
    pub fn apply(self, source: [f64; 2]) -> [f64; 2] {
        [
            self.coefficients[0] * source[0] + self.coefficients[1] * source[1],
            self.coefficients[2] * source[0] + self.coefficients[3] * source[1],
        ]
    }

    fn validate(self) -> Result<(), ConservativeSurfaceMapError> {
        Self::new(self.coefficients).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for TangentTransform {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = TangentTransformWire::deserialize(deserializer)?;
        Self::new(wire.coefficients).map_err(D::Error::custom)
    }
}

/// One positive source-cell overlap stored inside a canonical target row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SurfaceOverlapWeight {
    source_cell: CellId,
    area_m2: f64,
    tangent_transform: TangentTransform,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceOverlapWeightWire {
    source_cell: CellId,
    area_m2: f64,
    tangent_transform: TangentTransform,
}

impl SurfaceOverlapWeight {
    /// Constructs one finite, strictly positive overlap.
    pub fn new(
        source_cell: CellId,
        area_m2: f64,
        tangent_transform: TangentTransform,
    ) -> Result<Self, ConservativeSurfaceMapError> {
        if !area_m2.is_finite() || area_m2 <= 0.0 {
            return Err(ConservativeSurfaceMapError::InvalidOverlapArea {
                target_cell: None,
                source_cell,
                found: area_m2,
            });
        }
        tangent_transform.validate()?;
        Ok(Self {
            source_cell,
            area_m2,
            tangent_transform,
        })
    }

    /// Returns the source cell contributing this overlap.
    pub const fn source_cell(self) -> CellId {
        self.source_cell
    }

    /// Returns the physical overlap area in square metres.
    pub const fn area_m2(self) -> f64 {
        self.area_m2
    }

    /// Returns the precomputed tangent transform for this overlap.
    pub const fn tangent_transform(self) -> TangentTransform {
        self.tangent_transform
    }

    fn validate(self) -> Result<(), ConservativeSurfaceMapError> {
        Self::new(self.source_cell, self.area_m2, self.tangent_transform).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for SurfaceOverlapWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SurfaceOverlapWeightWire::deserialize(deserializer)?;
        Self::new(wire.source_cell, wire.area_m2, wire.tangent_transform).map_err(D::Error::custom)
    }
}

/// Deterministic closure and geometric-adjustment evidence from map construction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RemapSolveStats {
    balance_iterations: u16,
    max_source_margin_relative_error: f64,
    max_target_margin_relative_error: f64,
    max_relative_geometric_adjustment: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemapSolveStatsWire {
    balance_iterations: u16,
    max_source_margin_relative_error: f64,
    max_target_margin_relative_error: f64,
    max_relative_geometric_adjustment: f64,
}

impl RemapSolveStats {
    fn new(
        balance_iterations: u16,
        max_source_margin_relative_error: f64,
        max_target_margin_relative_error: f64,
        max_relative_geometric_adjustment: f64,
    ) -> Result<Self, ConservativeSurfaceMapError> {
        if balance_iterations > MAX_BALANCE_ITERATIONS {
            return Err(ConservativeSurfaceMapError::TooManyBalanceIterations {
                found: balance_iterations,
                max: MAX_BALANCE_ITERATIONS,
            });
        }
        for (field, value, max) in [
            (
                "max_source_margin_relative_error",
                max_source_margin_relative_error,
                MAX_MARGIN_RELATIVE_ERROR,
            ),
            (
                "max_target_margin_relative_error",
                max_target_margin_relative_error,
                MAX_MARGIN_RELATIVE_ERROR,
            ),
            (
                "max_relative_geometric_adjustment",
                max_relative_geometric_adjustment,
                MAX_RELATIVE_GEOMETRIC_ADJUSTMENT,
            ),
        ] {
            if !value.is_finite() || !(0.0..=max).contains(&value) {
                return Err(ConservativeSurfaceMapError::InvalidSolveStat {
                    field,
                    found: value,
                    max,
                });
            }
        }
        Ok(Self {
            balance_iterations,
            max_source_margin_relative_error,
            max_target_margin_relative_error,
            max_relative_geometric_adjustment,
        })
    }

    /// Returns the number of complete row/column balancing iterations.
    pub const fn balance_iterations(self) -> u16 {
        self.balance_iterations
    }

    /// Returns the maximum source-column relative closure error.
    pub const fn max_source_margin_relative_error(self) -> f64 {
        self.max_source_margin_relative_error
    }

    /// Returns the maximum target-row relative closure error.
    pub const fn max_target_margin_relative_error(self) -> f64 {
        self.max_target_margin_relative_error
    }

    /// Returns the largest relative change from a raw geometric overlap.
    pub const fn max_relative_geometric_adjustment(self) -> f64 {
        self.max_relative_geometric_adjustment
    }
}

impl<'de> Deserialize<'de> for RemapSolveStats {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RemapSolveStatsWire::deserialize(deserializer)?;
        Self::new(
            wire.balance_iterations,
            wire.max_source_margin_relative_error,
            wire.max_target_margin_relative_error,
            wire.max_relative_geometric_adjustment,
        )
        .map_err(D::Error::custom)
    }
}

/// A validated sparse conservative map between two exact spherical surfaces.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConservativeSurfaceMap {
    schema_version: u16,
    source_ref: SurfaceRef,
    target_ref: SurfaceRef,
    source_cell_areas_m2: Vec<f64>,
    target_cell_areas_m2: Vec<f64>,
    target_row_offsets: Vec<u32>,
    weights: Vec<SurfaceOverlapWeight>,
    solve_stats: RemapSolveStats,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConservativeSurfaceMapWire {
    schema_version: u16,
    source_ref: SurfaceRef,
    target_ref: SurfaceRef,
    #[serde(deserialize_with = "deserialize_remap_cell_areas")]
    source_cell_areas_m2: Vec<f64>,
    #[serde(deserialize_with = "deserialize_remap_cell_areas")]
    target_cell_areas_m2: Vec<f64>,
    #[serde(deserialize_with = "deserialize_remap_row_offsets")]
    target_row_offsets: Vec<u32>,
    #[serde(deserialize_with = "deserialize_remap_weights")]
    weights: Vec<SurfaceOverlapWeight>,
    solve_stats: RemapSolveStats,
}

fn deserialize_remap_cell_areas<'de, D>(deserializer: D) -> Result<Vec<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_REMAP_CELL_AREAS>(deserializer)
}

fn deserialize_remap_row_offsets<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_REMAP_ROW_OFFSETS>(deserializer)
}

fn deserialize_remap_weights<'de, D>(deserializer: D) -> Result<Vec<SurfaceOverlapWeight>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_REMAP_OVERLAPS>(deserializer)
}

impl ConservativeSurfaceMap {
    /// Constructs a map only when identities, sparse rows, and both area margins close.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        source_ref: SurfaceRef,
        target_ref: SurfaceRef,
        source_cell_areas_m2: Vec<f64>,
        target_cell_areas_m2: Vec<f64>,
        target_row_offsets: Vec<u32>,
        weights: Vec<SurfaceOverlapWeight>,
        balance_iterations: u16,
        max_relative_geometric_adjustment: f64,
    ) -> Result<Self, ConservativeSurfaceMapError> {
        Self::new_impl(
            schema_version,
            source_ref,
            target_ref,
            source_cell_areas_m2,
            target_cell_areas_m2,
            target_row_offsets,
            weights,
            balance_iterations,
            max_relative_geometric_adjustment,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_cancellable(
        schema_version: u16,
        source_ref: SurfaceRef,
        target_ref: SurfaceRef,
        source_cell_areas_m2: Vec<f64>,
        target_cell_areas_m2: Vec<f64>,
        target_row_offsets: Vec<u32>,
        weights: Vec<SurfaceOverlapWeight>,
        balance_iterations: u16,
        max_relative_geometric_adjustment: f64,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Self, ConservativeSurfaceMapError> {
        Self::new_impl(
            schema_version,
            source_ref,
            target_ref,
            source_cell_areas_m2,
            target_cell_areas_m2,
            target_row_offsets,
            weights,
            balance_iterations,
            max_relative_geometric_adjustment,
            Some(cancelled),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_impl(
        schema_version: u16,
        source_ref: SurfaceRef,
        target_ref: SurfaceRef,
        source_cell_areas_m2: Vec<f64>,
        target_cell_areas_m2: Vec<f64>,
        target_row_offsets: Vec<u32>,
        weights: Vec<SurfaceOverlapWeight>,
        balance_iterations: u16,
        max_relative_geometric_adjustment: f64,
        cancellation: MapCancellation<'_>,
    ) -> Result<Self, ConservativeSurfaceMapError> {
        let (max_source_error, max_target_error) = validate_map_data(
            schema_version,
            source_ref,
            target_ref,
            &source_cell_areas_m2,
            &target_cell_areas_m2,
            &target_row_offsets,
            &weights,
            cancellation,
        )?;
        let solve_stats = RemapSolveStats::new(
            balance_iterations,
            max_source_error,
            max_target_error,
            max_relative_geometric_adjustment,
        )?;
        Ok(Self {
            schema_version,
            source_ref,
            target_ref,
            source_cell_areas_m2,
            target_cell_areas_m2,
            target_row_offsets,
            weights,
            solve_stats,
        })
    }

    /// Rechecks all stable map invariants and stored closure evidence.
    pub fn validate(&self) -> Result<(), ConservativeSurfaceMapError> {
        self.validate_impl(None)
    }

    /// Rechecks all map invariants while polling sparse rows and weights.
    pub fn validate_cancellable(
        &self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), ConservativeSurfaceMapError> {
        self.validate_impl(Some(cancelled))
    }

    fn validate_impl(
        &self,
        cancellation: MapCancellation<'_>,
    ) -> Result<(), ConservativeSurfaceMapError> {
        let (max_source_error, max_target_error) = validate_map_data(
            self.schema_version,
            self.source_ref,
            self.target_ref,
            &self.source_cell_areas_m2,
            &self.target_cell_areas_m2,
            &self.target_row_offsets,
            &self.weights,
            cancellation,
        )?;
        let recalculated = RemapSolveStats::new(
            self.solve_stats.balance_iterations,
            max_source_error,
            max_target_error,
            self.solve_stats.max_relative_geometric_adjustment,
        )?;
        if recalculated != self.solve_stats {
            return Err(ConservativeSurfaceMapError::StaleSolveStats);
        }
        Ok(())
    }

    /// Returns the map schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact source surface identity.
    pub const fn source_ref(&self) -> SurfaceRef {
        self.source_ref
    }

    /// Returns the exact target surface identity.
    pub const fn target_ref(&self) -> SurfaceRef {
        self.target_ref
    }

    /// Returns canonical source cell areas in square metres.
    pub fn source_cell_areas_m2(&self) -> &[f64] {
        &self.source_cell_areas_m2
    }

    /// Returns canonical target cell areas in square metres.
    pub fn target_cell_areas_m2(&self) -> &[f64] {
        &self.target_cell_areas_m2
    }

    /// Returns the target-row CSR offsets.
    pub fn target_row_offsets(&self) -> &[u32] {
        &self.target_row_offsets
    }

    /// Returns every overlap in canonical target/source order.
    pub fn weights(&self) -> &[SurfaceOverlapWeight] {
        &self.weights
    }

    /// Returns the number of stored positive overlaps.
    pub fn overlap_count(&self) -> usize {
        self.weights.len()
    }

    /// Returns one target row, or `None` for an out-of-range target cell.
    pub fn target_row(&self, target: CellId) -> Option<&[SurfaceOverlapWeight]> {
        let index = target.raw() as usize;
        let start = *self.target_row_offsets.get(index)? as usize;
        let end = *self.target_row_offsets.get(index + 1)? as usize;
        self.weights.get(start..end)
    }

    /// Returns deterministic construction and closure evidence.
    pub const fn solve_stats(&self) -> RemapSolveStats {
        self.solve_stats
    }

    /// Fingerprints every semantic map field, including tangent transforms
    /// and solve evidence. Surface identities alone are insufficient because
    /// multiple margin-closing sparse maps can address the same two surfaces.
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint_impl(None)
            .expect("an uncancelled semantic map fingerprint cannot fail")
    }

    pub fn fingerprint_cancellable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<[u8; 32], ConservativeSurfaceMapError> {
        self.fingerprint_impl(Some(cancelled))
    }

    fn fingerprint_impl(
        &self,
        cancellation: Option<&dyn Fn() -> bool>,
    ) -> Result<[u8; 32], ConservativeSurfaceMapError> {
        check_fingerprint_cancelled(cancellation)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.conservative-surface-map.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        hash_surface_ref(&mut hasher, self.source_ref);
        hash_surface_ref(&mut hasher, self.target_ref);
        hasher.update(&(self.source_cell_areas_m2.len() as u64).to_le_bytes());
        for (index, area) in self.source_cell_areas_m2.iter().enumerate() {
            poll_fingerprint_cancelled(index, cancellation)?;
            hasher.update(&area.to_bits().to_le_bytes());
        }
        hasher.update(&(self.target_cell_areas_m2.len() as u64).to_le_bytes());
        for (index, area) in self.target_cell_areas_m2.iter().enumerate() {
            poll_fingerprint_cancelled(index, cancellation)?;
            hasher.update(&area.to_bits().to_le_bytes());
        }
        hasher.update(&(self.target_row_offsets.len() as u64).to_le_bytes());
        for (index, offset) in self.target_row_offsets.iter().enumerate() {
            poll_fingerprint_cancelled(index, cancellation)?;
            hasher.update(&offset.to_le_bytes());
        }
        hasher.update(&(self.weights.len() as u64).to_le_bytes());
        for (index, weight) in self.weights.iter().enumerate() {
            poll_fingerprint_cancelled(index, cancellation)?;
            hasher.update(&weight.source_cell.raw().to_le_bytes());
            hasher.update(&weight.area_m2.to_bits().to_le_bytes());
            for coefficient in weight.tangent_transform.coefficients {
                hasher.update(&coefficient.to_bits().to_le_bytes());
            }
        }
        hasher.update(&self.solve_stats.balance_iterations.to_le_bytes());
        for value in [
            self.solve_stats.max_source_margin_relative_error,
            self.solve_stats.max_target_margin_relative_error,
            self.solve_stats.max_relative_geometric_adjustment,
        ] {
            hasher.update(&value.to_bits().to_le_bytes());
        }
        check_fingerprint_cancelled(cancellation)?;
        Ok(*hasher.finalize().as_bytes())
    }
}

fn poll_fingerprint_cancelled(
    index: usize,
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), ConservativeSurfaceMapError> {
    if index % 256 == 0 {
        check_fingerprint_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_fingerprint_cancelled(
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), ConservativeSurfaceMapError> {
    if cancellation.is_some_and(|cancelled| cancelled()) {
        Err(ConservativeSurfaceMapError::Cancelled)
    } else {
        Ok(())
    }
}

fn hash_surface_ref(hasher: &mut blake3::Hasher, surface: SurfaceRef) {
    let geometry_tag = match surface.geometry_kind() {
        super::SurfaceGeometryKind::PlanarV1 => 0_u8,
        super::SurfaceGeometryKind::SphericalV1 => 1,
        super::SurfaceGeometryKind::SphericalGeodesicV2 => 2,
    };
    hasher.update(&[geometry_tag]);
    hasher.update(&surface.geometry_schema().to_le_bytes());
    hasher.update(&surface.cell_count().to_le_bytes());
    hasher.update(&surface.edge_count().to_le_bytes());
    hasher.update(&surface.fingerprint());
}

impl<'de> Deserialize<'de> for ConservativeSurfaceMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ConservativeSurfaceMapWire::deserialize(deserializer)?;
        let map = Self::new(
            wire.schema_version,
            wire.source_ref,
            wire.target_ref,
            wire.source_cell_areas_m2,
            wire.target_cell_areas_m2,
            wire.target_row_offsets,
            wire.weights,
            wire.solve_stats.balance_iterations,
            wire.solve_stats.max_relative_geometric_adjustment,
        )
        .map_err(D::Error::custom)?;
        if map.solve_stats != wire.solve_stats {
            return Err(D::Error::custom(
                ConservativeSurfaceMapError::StaleSolveStats,
            ));
        }
        Ok(map)
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_map_data(
    schema_version: u16,
    source_ref: SurfaceRef,
    target_ref: SurfaceRef,
    source_areas: &[f64],
    target_areas: &[f64],
    target_offsets: &[u32],
    weights: &[SurfaceOverlapWeight],
    mut cancellation: MapCancellation<'_>,
) -> Result<(f64, f64), ConservativeSurfaceMapError> {
    check_map_cancelled(&mut cancellation)?;
    if schema_version != CONSERVATIVE_SURFACE_MAP_SCHEMA_V1 {
        return Err(ConservativeSurfaceMapError::UnsupportedSchema {
            found: schema_version,
            supported: CONSERVATIVE_SURFACE_MAP_SCHEMA_V1,
        });
    }
    source_ref.validate()?;
    target_ref.validate()?;
    for (role, surface_ref) in [("source", source_ref), ("target", target_ref)] {
        if !surface_ref.geometry_kind().is_spherical() {
            return Err(ConservativeSurfaceMapError::NonSphericalSurface { role });
        }
        if surface_ref.cell_count() > MAX_SPHERICAL_CELL_COUNT {
            return Err(ConservativeSurfaceMapError::CellCountExceedsMaximum {
                role,
                found: surface_ref.cell_count(),
                max: MAX_SPHERICAL_CELL_COUNT,
            });
        }
    }
    validate_area_cardinality(
        "source",
        source_ref.cell_count(),
        source_areas,
        &mut cancellation,
    )?;
    validate_area_cardinality(
        "target",
        target_ref.cell_count(),
        target_areas,
        &mut cancellation,
    )?;
    let expected_offsets = target_areas.len() + 1;
    if target_offsets.len() != expected_offsets {
        return Err(ConservativeSurfaceMapError::RowOffsetCountMismatch {
            found: target_offsets.len(),
            expected: expected_offsets,
        });
    }
    if weights.len() > MAX_REMAP_OVERLAPS {
        return Err(ConservativeSurfaceMapError::TooManyOverlaps {
            found: weights.len(),
            max: MAX_REMAP_OVERLAPS,
        });
    }
    if target_offsets.first() != Some(&0) {
        return Err(ConservativeSurfaceMapError::InvalidFirstRowOffset);
    }
    let expected_end =
        u32::try_from(weights.len()).map_err(|_| ConservativeSurfaceMapError::TooManyOverlaps {
            found: weights.len(),
            max: MAX_REMAP_OVERLAPS,
        })?;
    if target_offsets.last() != Some(&expected_end) {
        return Err(ConservativeSurfaceMapError::InvalidFinalRowOffset {
            found: target_offsets.last().copied(),
            expected: expected_end,
        });
    }

    let mut source_sums = vec![CompensatedSum::default(); source_areas.len()];
    let mut max_target_error = 0.0_f64;
    for target_index in 0..target_areas.len() {
        poll_map_cancelled(target_index, &mut cancellation)?;
        let start = target_offsets[target_index] as usize;
        let end = target_offsets[target_index + 1] as usize;
        if start >= end || end > weights.len() {
            return Err(ConservativeSurfaceMapError::InvalidTargetRow {
                target_cell: CellId::from_raw(target_index as u32),
                start,
                end,
                overlaps: weights.len(),
            });
        }
        let mut row_sum = CompensatedSum::default();
        let mut previous_source = None;
        for (weight_index, weight) in weights[start..end].iter().enumerate() {
            poll_map_cancelled(start + weight_index, &mut cancellation)?;
            weight.validate()?;
            let source_index = weight.source_cell.raw() as usize;
            if source_index >= source_areas.len() {
                return Err(ConservativeSurfaceMapError::SourceCellOutOfRange {
                    target_cell: CellId::from_raw(target_index as u32),
                    source_cell: weight.source_cell,
                    source_cells: source_areas.len(),
                });
            }
            if previous_source.is_some_and(|previous| previous >= weight.source_cell) {
                return Err(ConservativeSurfaceMapError::NonCanonicalSourceOrder {
                    target_cell: CellId::from_raw(target_index as u32),
                    previous: previous_source.expect("checked as present"),
                    found: weight.source_cell,
                });
            }
            previous_source = Some(weight.source_cell);
            row_sum.add(weight.area_m2)?;
            source_sums[source_index].add(weight.area_m2)?;
        }
        max_target_error =
            max_target_error.max(relative_error(row_sum.total()?, target_areas[target_index]));
    }

    let mut max_source_error = 0.0_f64;
    for (index, (sum, &expected)) in source_sums.into_iter().zip(source_areas).enumerate() {
        poll_map_cancelled(index, &mut cancellation)?;
        max_source_error = max_source_error.max(relative_error(sum.total()?, expected));
    }
    if max_source_error > MAX_MARGIN_RELATIVE_ERROR || max_target_error > MAX_MARGIN_RELATIVE_ERROR
    {
        return Err(ConservativeSurfaceMapError::MarginClosureExceeded {
            max_source_relative_error: max_source_error,
            max_target_relative_error: max_target_error,
            max: MAX_MARGIN_RELATIVE_ERROR,
        });
    }
    let source_total = compensated_total(source_areas, &mut cancellation)?;
    let target_total = compensated_total(target_areas, &mut cancellation)?;
    let total_error = relative_error(source_total, target_total);
    if total_error > MAX_MARGIN_RELATIVE_ERROR {
        return Err(ConservativeSurfaceMapError::TotalAreaMismatch {
            source_m2: source_total,
            target_m2: target_total,
            relative_error: total_error,
            max: MAX_MARGIN_RELATIVE_ERROR,
        });
    }
    check_map_cancelled(&mut cancellation)?;
    Ok((max_source_error, max_target_error))
}

fn validate_area_cardinality(
    role: &'static str,
    expected_count: u32,
    areas: &[f64],
    cancellation: &mut MapCancellation<'_>,
) -> Result<(), ConservativeSurfaceMapError> {
    if areas.len() != expected_count as usize {
        return Err(ConservativeSurfaceMapError::AreaCountMismatch {
            role,
            found: areas.len(),
            expected: expected_count as usize,
        });
    }
    for (cell, found) in areas.iter().copied().enumerate() {
        poll_map_cancelled(cell, cancellation)?;
        if !found.is_finite() || found <= 0.0 {
            return Err(ConservativeSurfaceMapError::InvalidCellArea {
                role,
                cell: CellId::from_raw(cell as u32),
                found,
            });
        }
    }
    Ok(())
}

fn compensated_total(
    values: &[f64],
    cancellation: &mut MapCancellation<'_>,
) -> Result<f64, ConservativeSurfaceMapError> {
    let mut sum = CompensatedSum::default();
    for (index, &value) in values.iter().enumerate() {
        poll_map_cancelled(index, cancellation)?;
        sum.add(value)?;
    }
    sum.total()
}

type MapCancellation<'a> = Option<&'a mut dyn FnMut() -> bool>;

fn poll_map_cancelled(
    index: usize,
    cancellation: &mut MapCancellation<'_>,
) -> Result<(), ConservativeSurfaceMapError> {
    if index % 256 == 0 {
        check_map_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_map_cancelled(
    cancellation: &mut MapCancellation<'_>,
) -> Result<(), ConservativeSurfaceMapError> {
    if cancellation
        .as_deref_mut()
        .is_some_and(|cancelled| cancelled())
    {
        Err(ConservativeSurfaceMapError::Cancelled)
    } else {
        Ok(())
    }
}

fn relative_error(found: f64, expected: f64) -> f64 {
    (found - expected).abs() / expected.abs().max(f64::MIN_POSITIVE)
}

#[derive(Debug, Clone, Copy, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) -> Result<(), ConservativeSurfaceMapError> {
        let next = self.sum + value;
        if !next.is_finite() {
            return Err(ConservativeSurfaceMapError::NonFiniteAccumulation);
        }
        let correction = if self.sum.abs() >= value.abs() {
            (self.sum - next) + value
        } else {
            (value - next) + self.sum
        };
        self.sum = next;
        self.correction += correction;
        if !self.correction.is_finite() {
            return Err(ConservativeSurfaceMapError::NonFiniteAccumulation);
        }
        Ok(())
    }

    fn total(self) -> Result<f64, ConservativeSurfaceMapError> {
        let total = self.sum + self.correction;
        total
            .is_finite()
            .then_some(total)
            .ok_or(ConservativeSurfaceMapError::NonFiniteAccumulation)
    }
}

/// Errors returned when a conservative map is malformed or insufficiently closed.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ConservativeSurfaceMapError {
    #[error("conservative surface-map operation was cancelled")]
    Cancelled,
    #[error(
        "unsupported conservative surface-map schema {found}; supported version is {supported}"
    )]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("invalid conservative-map surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    #[error("conservative-map {role} surface is not spherical")]
    NonSphericalSurface { role: &'static str },
    #[error("conservative-map {role} cell count {found} exceeds {max}")]
    CellCountExceedsMaximum {
        role: &'static str,
        found: u32,
        max: u32,
    },
    #[error("conservative-map {role} area count {found} differs from {expected}")]
    AreaCountMismatch {
        role: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("conservative-map {role} cell {cell:?} has invalid area {found}")]
    InvalidCellArea {
        role: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error("tangent transform coefficient {index} is invalid: {found}")]
    InvalidTangentCoefficient { index: usize, found: f64 },
    #[error("target {target_cell:?} source {source_cell:?} has invalid overlap area {found}")]
    InvalidOverlapArea {
        target_cell: Option<CellId>,
        source_cell: CellId,
        found: f64,
    },
    #[error("target row offset count {found} differs from {expected}")]
    RowOffsetCountMismatch { found: usize, expected: usize },
    #[error("the first target row offset must be zero")]
    InvalidFirstRowOffset,
    #[error("final target row offset {found:?} differs from overlap count {expected}")]
    InvalidFinalRowOffset { found: Option<u32>, expected: u32 },
    #[error("target {target_cell:?} has invalid row {start}..{end} for {overlaps} overlaps")]
    InvalidTargetRow {
        target_cell: CellId,
        start: usize,
        end: usize,
        overlaps: usize,
    },
    #[error("target {target_cell:?} source {source_cell:?} is outside {source_cells} cells")]
    SourceCellOutOfRange {
        target_cell: CellId,
        source_cell: CellId,
        source_cells: usize,
    },
    #[error("target {target_cell:?} source order is not strict: {previous:?} then {found:?}")]
    NonCanonicalSourceOrder {
        target_cell: CellId,
        previous: CellId,
        found: CellId,
    },
    #[error("conservative map contains {found} overlaps; maximum is {max}")]
    TooManyOverlaps { found: usize, max: usize },
    #[error("source/target total areas differ: source {source_m2}, target {target_m2}, relative error {relative_error} > {max}")]
    TotalAreaMismatch {
        source_m2: f64,
        target_m2: f64,
        relative_error: f64,
        max: f64,
    },
    #[error("conservative-map margins do not close: source {max_source_relative_error}, target {max_target_relative_error}, maximum {max}")]
    MarginClosureExceeded {
        max_source_relative_error: f64,
        max_target_relative_error: f64,
        max: f64,
    },
    #[error("balance iteration count {found} exceeds {max}")]
    TooManyBalanceIterations { found: u16, max: u16 },
    #[error("solve statistic {field} is {found}; expected finite 0..={max}")]
    InvalidSolveStat {
        field: &'static str,
        found: f64,
        max: f64,
    },
    #[error("stored remap solve statistics do not match recalculated margins")]
    StaleSolveStats,
    #[error("conservative-map accumulation produced a non-finite value")]
    NonFiniteAccumulation,
}
