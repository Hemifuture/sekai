use std::collections::BTreeMap;

use thiserror::Error;

use super::{CellFillKind, FieldView, FieldViewError};
use crate::world::fields::{FieldId, ValueRange};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteId {
    /// Dark water through pale parchment.
    Sequential,
    /// Cool and warm values around a neutral midpoint.
    Diverging,
    /// Twelve discrete colors for stable category indices.
    Categorical,
}

/// How a scalar display range is selected.
#[derive(Debug, Clone, Copy, PartialEq)]
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

    /// Clones this prepared payload with a new scalar display range.
    pub fn with_display_range(&self, range: ResolvedDisplayRange) -> Self {
        let mut next = self.clone();
        next.display_range = Some(range);
        next
    }
}

/// Failures returned while preparing renderer-neutral display data.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DisplayPrepareError {
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
    /// The field cannot be rendered as a V1 cell fill.
    #[error("field {field:?} cannot be rendered as a V1 cell fill")]
    UnsupportedCellFill {
        /// The unsupported field.
        field: FieldId,
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

    let kind = field.cell_fill_kind().map_err(|error| match error {
        FieldViewError::UnsupportedCellFill { field, .. }
        | FieldViewError::TypeMismatch { field, .. } => {
            DisplayPrepareError::UnsupportedCellFill { field }
        }
    })?;

    match kind {
        CellFillKind::Scalar => {
            let values = field
                .scalar_values()
                .expect("cell-fill kind guarantees scalar values");
            let source_range = finite_min_max(values).ok_or_else(|| {
                DisplayPrepareError::NoFiniteScalarValues {
                    field: field.schema().id.clone(),
                }
            })?;
            let display_range = resolve_display_range(field, range_mode)?;
            Ok(PreparedCellField {
                field_id: field.schema().id.clone(),
                kind: PreparedFieldKind::Scalar,
                raw_values: values.iter().map(|value| value.to_bits()).collect(),
                source_range: Some(source_range),
                display_range: Some(display_range),
                category_keys: Vec::new(),
            })
        }
        CellFillKind::Category => {
            let values = field
                .category_values()
                .expect("cell-fill kind guarantees category values");
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
            Ok(PreparedCellField {
                field_id,
                kind: PreparedFieldKind::Category,
                raw_values,
                source_range: None,
                display_range: None,
                category_keys,
            })
        }
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
