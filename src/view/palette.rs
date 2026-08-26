use std::collections::BTreeMap;

use thiserror::Error;

use super::{FieldView, FieldViewError};
use crate::world::fields::{FieldId, ValueRange};
use crate::world::CellId;

/// A linear-light RGBA color used consistently by CPU and GPU display paths.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearRgba([f32; 4]);

impl LinearRgba {
    /// Creates a color from linear red, green, blue, and alpha components.
    pub const fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self([red, green, blue, alpha])
    }

    /// Returns the four linear components.
    pub const fn components(self) -> [f32; 4] {
        self.0
    }

    /// Returns whether every component is finite and lies in the unit interval.
    pub fn is_valid(self) -> bool {
        self.0
            .iter()
            .all(|component| component.is_finite() && (0.0..=1.0).contains(component))
    }
}

/// Built-in renderer palettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PaletteId {
    /// Dark water through pale parchment.
    Sequential,
    /// Cool and warm values around a neutral midpoint.
    Diverging,
    /// Twelve discrete colors for stable category indices.
    Categorical,
    /// Sea-anchored terrain colors: water depths below the midpoint, land heights above.
    Hypsometric,
    /// Two fixed semantic colors: ocean water then land.
    LandOcean,
}

/// How a scalar display range is selected.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DisplayRangeMode {
    /// Use the field schema's declared valid range.
    Schema,
    /// Derive the range from finite payload values.
    Data,
    /// Use an explicit validated range.
    Manual(ValueRange),
}

/// A finite, ordered scalar range resolved for display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedDisplayRange {
    min: f32,
    max: f32,
}

impl ResolvedDisplayRange {
    /// Creates a finite range whose bounds may be equal but not reversed.
    pub fn new(min: f32, max: f32) -> Result<Self, DisplayPrepareError> {
        if !min.is_finite() || !max.is_finite() || min > max {
            return Err(DisplayPrepareError::InvalidRange);
        }
        Ok(Self { min, max })
    }

    /// Returns the inclusive display bounds.
    pub const fn bounds(self) -> (f32, f32) {
        (self.min, self.max)
    }

    fn normalize(self, value: f32) -> f32 {
        let width = self.max - self.min;
        if width == 0.0 {
            0.5
        } else {
            ((value - self.min) / width).clamp(0.0, 1.0)
        }
    }
}

impl From<ValueRange> for ResolvedDisplayRange {
    fn from(value: ValueRange) -> Self {
        Self {
            min: value.min(),
            max: value.max(),
        }
    }
}

/// The packed representation of a prepared cell field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedFieldKind {
    /// Raw entries contain `f32::to_bits`.
    Scalar,
    /// Raw entries contain compact category indices.
    Category,
}

/// One validated field payload ready for indexed GPU lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedCellField {
    field_id: FieldId,
    kind: PreparedFieldKind,
    raw_values: Vec<u32>,
    source_range: Option<ResolvedDisplayRange>,
    display_range: Option<ResolvedDisplayRange>,
    category_keys: Vec<u32>,
}

impl PreparedCellField {
    /// Returns the stable source field identifier.
    pub fn field_id(&self) -> &FieldId {
        &self.field_id
    }

    /// Returns the packed value representation.
    pub const fn kind(&self) -> PreparedFieldKind {
        self.kind
    }

    /// Returns packed values in stable cell order.
    pub fn raw_values(&self) -> &[u32] {
        &self.raw_values
    }

    /// Returns the number of prepared cell values.
    pub fn len(&self) -> usize {
        self.raw_values.len()
    }

    /// Returns whether no cell values were prepared.
    pub fn is_empty(&self) -> bool {
        self.raw_values.is_empty()
    }

    /// Returns the finite data range for scalar source values.
    pub const fn source_range(&self) -> Option<ResolvedDisplayRange> {
        self.source_range
    }

    /// Returns the active scalar display range.
    pub const fn display_range(&self) -> Option<ResolvedDisplayRange> {
        self.display_range
    }

    /// Returns sorted raw category keys indexed by the packed values.
    pub fn category_keys(&self) -> &[u32] {
        &self.category_keys
    }

    /// Returns owned heap bytes using vector and string capacities with checked arithmetic.
    pub fn resident_bytes(&self) -> Result<usize, super::ResidentBytesError> {
        let context = "prepared cell field";
        let total = self
            .field_id
            .resident_bytes()
            .ok_or(super::ResidentBytesError { context })?;
        let total =
            super::resident::add_capacity::<u32>(total, self.raw_values.capacity(), context)?;
        super::resident::add_capacity::<u32>(total, self.category_keys.capacity(), context)
    }
}

/// Failures returned while preparing renderer-neutral display data.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DisplayPrepareError {
    /// A registered schema and its current payload could not form a field view.
    #[error(transparent)]
    FieldView(#[from] FieldViewError),
    /// A display range was non-finite or reversed.
    #[error("display range must have finite bounds with min <= max")]
    InvalidRange,
    /// Schema range mode was requested for a field without a declared range.
    #[error("field {field:?} has no schema display range")]
    MissingSchemaRange {
        /// The scalar field without a declared range.
        field: FieldId,
    },
    /// A scalar payload had no finite values from which to derive a range.
    #[error("field {field:?} has no finite scalar values")]
    NoFiniteScalarValues {
        /// The scalar field with no finite values.
        field: FieldId,
    },
    /// Prepared values did not match the required cell count.
    #[error("field payload has {actual} cells, expected {expected}")]
    CellCountMismatch {
        /// The required cell count.
        expected: usize,
        /// The supplied field count.
        actual: usize,
    },
    /// Prepared values did not match the required cardinality for their schema domain.
    #[error("field {field:?} payload has {actual} {domain:?} values, expected {expected}")]
    FieldCardinalityMismatch {
        /// The field whose payload length was rejected.
        field: FieldId,
        /// The domain whose cardinality was required.
        domain: crate::world::fields::FieldDomain,
        /// The required value count.
        expected: usize,
        /// The supplied value count.
        actual: usize,
    },
    /// The field cannot be rendered as a V1 cell fill.
    #[error("field {field:?} cannot be rendered as a V1 cell fill")]
    UnsupportedCellFill {
        /// The unsupported field.
        field: FieldId,
    },
    /// The field cannot be rendered by a spherical fill or overlay channel.
    #[error("field {field:?} cannot be rendered by a spherical presentation channel")]
    UnsupportedSphericalChannel {
        /// The unsupported field.
        field: FieldId,
    },
    /// A vector payload component or its magnitude was not finite.
    #[error("field {field:?} has non-finite vector data at index {index}")]
    NonFiniteVector {
        /// The vector field whose values were rejected.
        field: FieldId,
        /// The rejected vector index.
        index: usize,
    },
    /// A category payload contained a key absent from its schema.
    #[error("field {field:?} contains undeclared category key {key}")]
    UnknownCategory {
        /// The category field.
        field: FieldId,
        /// The undeclared raw key.
        key: u32,
    },
    /// The number of category labels cannot be represented by compact `u32` indices.
    #[error("field {field:?} has too many categories: {count}")]
    TooManyCategories {
        /// The category field.
        field: FieldId,
        /// The unrepresentable category count.
        count: usize,
    },
    /// A diagnostic referenced a cell outside the prepared mask.
    #[error("diagnostic cell {cell:?} is outside cell count {cell_count}")]
    DiagnosticCellOutOfRange {
        /// The invalid stable cell identifier.
        cell: CellId,
        /// The number of valid cells.
        cell_count: usize,
    },
    /// The source declared more cells than the display budget.
    #[error("display cell count {actual} exceeds budget {max}")]
    CellBudgetExceeded {
        /// The declared cell count.
        actual: usize,
        /// The configured cell budget.
        max: usize,
    },
    /// A required stable cell had no polygon.
    #[error("cell {cell:?} has no display geometry")]
    MissingCellGeometry {
        /// The cell without a polygon.
        cell: CellId,
    },
    /// A present polygon was too short, degenerate, clockwise, or non-convex.
    #[error("cell {cell:?} has malformed display geometry")]
    MalformedCellGeometry {
        /// The cell with an invalid polygon.
        cell: CellId,
    },
    /// A polygon point lay outside the declared world rectangle.
    #[error("cell {cell:?} has a point outside display bounds")]
    CoordinateOutOfBounds {
        /// The cell with an out-of-bounds point.
        cell: CellId,
    },
    /// A normalized coordinate could not be represented as finite `f32`.
    #[error("cell {cell:?} has a coordinate that cannot be represented for display")]
    CoordinateConversionFailed {
        /// The cell with an unrepresentable point.
        cell: CellId,
    },
    /// Origin-shifted local dimensions could not be represented as finite positive `f32`.
    #[error("local display extent {width} x {height} cannot be represented")]
    LocalExtentOutOfRange {
        /// World width.
        width: f64,
        /// World height.
        height: f64,
    },
    /// Prepared vertices exceeded the configured budget.
    #[error("display vertex count {actual} exceeds budget {max}")]
    VertexBudgetExceeded {
        /// Required vertices.
        actual: usize,
        /// Configured vertex budget.
        max: usize,
    },
    /// Prepared indices exceeded the configured budget.
    #[error("display index count {actual} exceeds budget {max}")]
    IndexBudgetExceeded {
        /// Required indices.
        actual: usize,
        /// Configured index budget.
        max: usize,
    },
    /// Picking-bin references exceeded the configured budget.
    #[error("display picker reference count {actual} exceeds budget {max}")]
    PickerBudgetExceeded {
        /// Required picker references.
        actual: usize,
        /// Configured picker-reference budget.
        max: usize,
    },
    /// Checked display arithmetic could not represent a result.
    #[error("integer overflow while computing {context}")]
    IntegerOverflow {
        /// The checked operation's stable context.
        context: &'static str,
    },
    /// A display revision used reserved value zero.
    #[error("display revision must be non-zero")]
    ZeroRevision,
    /// The process-local display revision clock would wrap.
    #[error("display revision clock overflow")]
    RevisionOverflow,
    /// A packet palette was empty, non-finite, or outside linear RGBA bounds.
    #[error("display palette must be non-empty finite linear RGBA")]
    InvalidPalette,
    /// No available scalar or category cell field could be prepared.
    #[error("no renderable cell field is available")]
    NoRenderableField,
    /// A reference image used a zero width or height.
    #[error("reference image dimensions must be non-zero, got {width} x {height}")]
    InvalidReferenceImageDimensions {
        /// Requested image width.
        width: u32,
        /// Requested image height.
        height: u32,
    },
    /// A checked reference-image allocation could not be reserved.
    #[error("could not allocate {bytes} bytes for the reference image")]
    ReferenceImageAllocationFailed {
        /// Requested RGBA8 byte count.
        bytes: usize,
    },
}

const SEQUENTIAL: [LinearRgba; 5] = [
    LinearRgba::new(0.025, 0.045, 0.070, 1.0),
    LinearRgba::new(0.055, 0.155, 0.205, 1.0),
    LinearRgba::new(0.180, 0.350, 0.285, 1.0),
    LinearRgba::new(0.520, 0.555, 0.300, 1.0),
    LinearRgba::new(0.900, 0.790, 0.515, 1.0),
];

const DIVERGING: [LinearRgba; 5] = [
    LinearRgba::new(0.055, 0.120, 0.260, 1.0),
    LinearRgba::new(0.210, 0.390, 0.500, 1.0),
    LinearRgba::new(0.720, 0.680, 0.560, 1.0),
    LinearRgba::new(0.570, 0.260, 0.190, 1.0),
    LinearRgba::new(0.260, 0.055, 0.045, 1.0),
];

const CATEGORICAL: [LinearRgba; 12] = [
    LinearRgba::new(0.670, 0.190, 0.150, 1.0),
    LinearRgba::new(0.110, 0.390, 0.540, 1.0),
    LinearRgba::new(0.300, 0.500, 0.190, 1.0),
    LinearRgba::new(0.610, 0.410, 0.100, 1.0),
    LinearRgba::new(0.380, 0.210, 0.520, 1.0),
    LinearRgba::new(0.100, 0.500, 0.440, 1.0),
    LinearRgba::new(0.710, 0.310, 0.080, 1.0),
    LinearRgba::new(0.500, 0.145, 0.330, 1.0),
    LinearRgba::new(0.245, 0.315, 0.620, 1.0),
    LinearRgba::new(0.455, 0.440, 0.120, 1.0),
    LinearRgba::new(0.120, 0.330, 0.255, 1.0),
    LinearRgba::new(0.540, 0.255, 0.095, 1.0),
];

/// The fixed hypsometric display half-range around sea level in metres.
///
/// Elevation classes are absolute (atlas practice — Imhof's cartographic
/// relief school and the ETOPO ramps): a colour always means the same
/// metres-above-sea, worlds are comparable, and one extreme trench or
/// peak can no longer compress everything else (values beyond the range
/// clamp to the end classes). Hypsometric display ranges must stay
/// symmetric around sea level so the water-to-land break sits exactly at
/// t = 0.5.
pub const HYPSOMETRIC_DISPLAY_RADIUS_M: f32 = 6_000.0;
/// Entry granularity of the expanded class table in metres. Every class
/// boundary below must be a multiple of this lattice.
const HYPSOMETRIC_STEP_M: f32 = 100.0;
/// Entry count: one per lattice step across ±HYPSOMETRIC_DISPLAY_RADIUS_M.
const HYPSOMETRIC_ENTRIES: usize = 121;

/// Sea-anchored hypsometric elevation classes: `(lower bound in metres,
/// class colour)`, deep abyss to summit snow. Boundaries follow classic
/// atlas banding (finer near sea level on both sides, where most of the
/// world lives; coarser toward the extremes).
const HYPSOMETRIC_CLASSES: [(f32, LinearRgba); 18] = [
    (-6_000.0, LinearRgba::new(0.008, 0.020, 0.065, 1.0)),
    (-4_000.0, LinearRgba::new(0.015, 0.045, 0.120, 1.0)),
    (-3_000.0, LinearRgba::new(0.030, 0.090, 0.200, 1.0)),
    (-2_000.0, LinearRgba::new(0.055, 0.140, 0.280, 1.0)),
    (-1_000.0, LinearRgba::new(0.085, 0.195, 0.350, 1.0)),
    (-500.0, LinearRgba::new(0.120, 0.260, 0.420, 1.0)),
    (-200.0, LinearRgba::new(0.170, 0.330, 0.490, 1.0)),
    (-100.0, LinearRgba::new(0.230, 0.410, 0.550, 1.0)),
    (0.0, LinearRgba::new(0.155, 0.310, 0.135, 1.0)),
    (100.0, LinearRgba::new(0.220, 0.340, 0.140, 1.0)),
    (200.0, LinearRgba::new(0.290, 0.365, 0.145, 1.0)),
    (500.0, LinearRgba::new(0.375, 0.390, 0.155, 1.0)),
    (1_000.0, LinearRgba::new(0.430, 0.400, 0.170, 1.0)),
    (1_500.0, LinearRgba::new(0.465, 0.370, 0.165, 1.0)),
    (2_000.0, LinearRgba::new(0.470, 0.330, 0.160, 1.0)),
    (3_000.0, LinearRgba::new(0.380, 0.240, 0.140, 1.0)),
    (4_000.0, LinearRgba::new(0.420, 0.360, 0.320, 1.0)),
    (5_000.0, LinearRgba::new(0.880, 0.870, 0.850, 1.0)),
];

/// The class table expanded onto the uniform lattice `sample_palette`
/// consumes: entry i sits at −radius + i·step and carries its class
/// colour. Class interiors stay flat; each boundary gets one lattice
/// step of blend (the linear segment between the two adjacent entries),
/// a soft edge in place of a hard contour line. Entry 60 is elevation 0,
/// so the water-to-land break lands exactly at t = 0.5.
const HYPSOMETRIC: [LinearRgba; HYPSOMETRIC_ENTRIES] = build_stepped_hypsometric();

const fn build_stepped_hypsometric() -> [LinearRgba; HYPSOMETRIC_ENTRIES] {
    let mut table = [HYPSOMETRIC_CLASSES[0].1; HYPSOMETRIC_ENTRIES];
    let mut entry = 0;
    while entry < HYPSOMETRIC_ENTRIES {
        let elevation_m = -HYPSOMETRIC_DISPLAY_RADIUS_M + entry as f32 * HYPSOMETRIC_STEP_M;
        let mut class = 0;
        let mut candidate = 0;
        while candidate < HYPSOMETRIC_CLASSES.len() {
            if elevation_m >= HYPSOMETRIC_CLASSES[candidate].0 {
                class = candidate;
            }
            candidate += 1;
        }
        table[entry] = HYPSOMETRIC_CLASSES[class].1;
        entry += 1;
    }
    table
}

/// Fixed semantic pair for land/ocean category fields: index 0 is ocean water,
/// index 1 is land, matching the stable land/ocean category encoding.
const LAND_OCEAN: [LinearRgba; 2] = [
    LinearRgba::new(0.055, 0.150, 0.290, 1.0),
    LinearRgba::new(0.320, 0.360, 0.180, 1.0),
];

/// Linear-light color for an informational diagnostic.
pub const DIAGNOSTIC_INFO_COLOR: LinearRgba = LinearRgba::new(0.130, 0.430, 0.720, 1.0);
/// Linear-light color for a warning diagnostic.
pub const DIAGNOSTIC_WARNING_COLOR: LinearRgba = LinearRgba::new(0.930, 0.570, 0.080, 1.0);
/// Linear-light color for an error diagnostic.
pub const DIAGNOSTIC_ERROR_COLOR: LinearRgba = LinearRgba::new(0.850, 0.080, 0.065, 1.0);

/// Returns one immutable built-in palette table.
pub const fn built_in_palette(id: PaletteId) -> &'static [LinearRgba] {
    match id {
        PaletteId::Sequential => &SEQUENTIAL,
        PaletteId::Diverging => &DIVERGING,
        PaletteId::Categorical => &CATEGORICAL,
        PaletteId::Hypsometric => &HYPSOMETRIC,
        PaletteId::LandOcean => &LAND_OCEAN,
    }
}

/// Linearly samples a palette after clamping to its unit interval.
pub fn sample_palette(palette: &[LinearRgba], t: f32) -> LinearRgba {
    match palette {
        [] => LinearRgba::new(0.0, 0.0, 0.0, 0.0),
        [only] => *only,
        _ => {
            let t = if t.is_finite() {
                t.clamp(0.0, 1.0)
            } else {
                0.5
            };
            let scaled = t * (palette.len() - 1) as f32;
            let lower = scaled.floor() as usize;
            let upper = (lower + 1).min(palette.len() - 1);
            let amount = scaled - lower as f32;
            let start = palette[lower].components();
            let end = palette[upper].components();
            LinearRgba::new(
                start[0] + (end[0] - start[0]) * amount,
                start[1] + (end[1] - start[1]) * amount,
                start[2] + (end[2] - start[2]) * amount,
                start[3] + (end[3] - start[3]) * amount,
            )
        }
    }
}

/// Resolves the active range for a scalar field.
pub fn resolve_display_range(
    field: &FieldView<'_>,
    mode: DisplayRangeMode,
) -> Result<ResolvedDisplayRange, DisplayPrepareError> {
    let values = field
        .scalar_values()
        .ok_or_else(|| DisplayPrepareError::UnsupportedCellFill {
            field: field.schema().id.clone(),
        })?;
    match mode {
        DisplayRangeMode::Schema => field
            .schema()
            .valid_range
            .map(ResolvedDisplayRange::from)
            .ok_or_else(|| DisplayPrepareError::MissingSchemaRange {
                field: field.schema().id.clone(),
            }),
        DisplayRangeMode::Data => {
            finite_min_max(values).ok_or_else(|| DisplayPrepareError::NoFiniteScalarValues {
                field: field.schema().id.clone(),
            })
        }
        DisplayRangeMode::Manual(range) => Ok(range.into()),
    }
}

/// Packs one scalar or category cell field for indexed GPU lookup.
pub fn prepare_cell_field(
    field: &FieldView<'_>,
    expected_cells: usize,
    range_mode: DisplayRangeMode,
) -> Result<PreparedCellField, DisplayPrepareError> {
    if field.len() != expected_cells {
        return Err(DisplayPrepareError::CellCountMismatch {
            expected: expected_cells,
            actual: field.len(),
        });
    }
    let prepared = prepare_scalar_or_category_field(
        field,
        crate::world::fields::FieldDomain::Cells,
        range_mode,
    )
    .map_err(map_cell_fill_error)?;
    Ok(PreparedCellField {
        field_id: prepared.field_id,
        kind: prepared.kind,
        raw_values: prepared.raw_values,
        source_range: prepared.source_range,
        display_range: prepared.display_range,
        category_keys: prepared.category_keys,
    })
}

/// Shared scalar/category packing used by cell fills and spherical edge overlays.
pub(crate) fn prepare_scalar_or_category_field(
    field: &FieldView<'_>,
    domain: crate::world::fields::FieldDomain,
    range_mode: DisplayRangeMode,
) -> Result<PreparedPackedField, DisplayPrepareError> {
    if field.schema().domain != domain {
        return Err(DisplayPrepareError::UnsupportedSphericalChannel {
            field: field.schema().id.clone(),
        });
    }
    match field.schema().value_type {
        crate::world::fields::FieldValueType::ScalarF32 => {
            let values = field.scalar_values().ok_or_else(|| {
                DisplayPrepareError::UnsupportedSphericalChannel {
                    field: field.schema().id.clone(),
                }
            })?;
            let source_range = finite_min_max(values).ok_or_else(|| {
                DisplayPrepareError::NoFiniteScalarValues {
                    field: field.schema().id.clone(),
                }
            })?;
            let display_range = resolve_display_range(field, range_mode)?;
            Ok(PreparedPackedField {
                field_id: field.schema().id.clone(),
                kind: PreparedFieldKind::Scalar,
                raw_values: values.iter().map(|value| value.to_bits()).collect(),
                source_range: Some(source_range),
                display_range: Some(display_range),
                category_keys: Vec::new(),
            })
        }
        crate::world::fields::FieldValueType::CategoryU32 => {
            let values = field.category_values().ok_or_else(|| {
                DisplayPrepareError::UnsupportedSphericalChannel {
                    field: field.schema().id.clone(),
                }
            })?;
            let field_id = field.schema().id.clone();
            let category_keys: Vec<_> = field.schema().category_labels.keys().copied().collect();
            let compact = category_keys
                .iter()
                .enumerate()
                .map(|(index, key)| {
                    u32::try_from(index)
                        .map(|index| (*key, index))
                        .map_err(|_| DisplayPrepareError::TooManyCategories {
                            field: field_id.clone(),
                            count: category_keys.len(),
                        })
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            let raw_values = values
                .iter()
                .map(|key| {
                    compact
                        .get(key)
                        .copied()
                        .ok_or_else(|| DisplayPrepareError::UnknownCategory {
                            field: field_id.clone(),
                            key: *key,
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(PreparedPackedField {
                field_id,
                kind: PreparedFieldKind::Category,
                raw_values,
                source_range: None,
                display_range: None,
                category_keys,
            })
        }
        _ => Err(DisplayPrepareError::UnsupportedSphericalChannel {
            field: field.schema().id.clone(),
        }),
    }
}

#[derive(Debug)]
pub(crate) struct PreparedPackedField {
    pub(crate) field_id: FieldId,
    pub(crate) kind: PreparedFieldKind,
    pub(crate) raw_values: Vec<u32>,
    pub(crate) source_range: Option<ResolvedDisplayRange>,
    pub(crate) display_range: Option<ResolvedDisplayRange>,
    pub(crate) category_keys: Vec<u32>,
}

fn map_cell_fill_error(error: DisplayPrepareError) -> DisplayPrepareError {
    match error {
        DisplayPrepareError::UnsupportedSphericalChannel { field } => {
            DisplayPrepareError::UnsupportedCellFill { field }
        }
        other => other,
    }
}

/// Colors a scalar through a resolved range and palette.
pub fn scalar_color(
    raw_value: f32,
    range: ResolvedDisplayRange,
    palette: &[LinearRgba],
) -> LinearRgba {
    sample_palette(palette, range.normalize(raw_value))
}

/// Colors a compact category index, cycling through a finite palette.
pub fn category_color(compact_index: u32, palette: &[LinearRgba]) -> LinearRgba {
    if palette.is_empty() {
        return LinearRgba::new(0.0, 0.0, 0.0, 0.0);
    }
    palette[compact_index as usize % palette.len()]
}

fn finite_min_max(values: &[f32]) -> Option<ResolvedDisplayRange> {
    let mut finite = values.iter().copied().filter(|value| value.is_finite());
    let first = finite.next()?;
    let (min, max) = finite.fold((first, first), |(min, max), value| {
        (min.min(value), max.max(value))
    });
    Some(ResolvedDisplayRange { min, max })
}
