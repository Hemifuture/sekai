use std::collections::{BTreeMap, BTreeSet};

use serde::de::Error as _;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_UNIT_SYMBOL_BYTES: usize = 32;
const MAX_DECIMAL_PLACES: u8 = 9;

/// Errors returned while constructing or registering extension-field schemas.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FieldSchemaError {
    /// An identifier or localization-key component violates the V1 syntax.
    #[error(
        "{component} must be 1..={MAX_IDENTIFIER_BYTES} lowercase ASCII bytes, use only a-z, 0-9, '-', '_', or '.', and start and end with an alphanumeric byte"
    )]
    InvalidComponent {
        /// The role of the invalid component.
        component: &'static str,
    },
    /// A field identifier used reserved version zero.
    #[error("field identifier version must be non-zero")]
    ZeroVersion,
    /// A numeric range was non-finite or reversed.
    #[error("value range must have finite bounds with min <= max")]
    InvalidRange,
    /// Display precision exceeded the V1 limit.
    #[error("decimal places must be in 0..={MAX_DECIMAL_PLACES}, got {0}")]
    InvalidDecimalPlaces(
        /// The rejected number of decimal places.
        u8,
    ),
    /// A custom unit symbol was empty or exceeded the V1 byte limit.
    #[error("custom unit symbol must be 1..={MAX_UNIT_SYMBOL_BYTES} UTF-8 bytes")]
    InvalidUnitSymbol,
    /// A non-scalar schema declared a numeric range.
    #[error("only scalar f32 fields may declare a valid range")]
    RangeForNonScalar,
    /// A category schema did not declare any allowed values.
    #[error("category fields require at least one category label")]
    MissingCategoryLabels,
    /// A non-category schema declared category labels.
    #[error("only category fields may declare category labels")]
    CategoryLabelsForNonCategory,
    /// A display palette was incompatible with the schema value type.
    #[error("display palette is incompatible with the field value type")]
    IncompatiblePalette,
    /// A field identifier was registered more than once.
    #[error("field {0:?} is already registered")]
    DuplicateField(
        /// The duplicate field identifier.
        FieldId,
    ),
    /// A schema refers to a dependency absent from the registry.
    #[error("field {field:?} depends on missing field {dependency:?}")]
    MissingDependency {
        /// The schema containing the missing dependency.
        field: FieldId,
        /// The dependency absent from the registry.
        dependency: FieldId,
    },
    /// The registry's dependency graph contains a cycle.
    #[error("field dependency cycle includes {0:?}")]
    DependencyCycle(
        /// A field identifier participating in the cycle.
        FieldId,
    ),
}

/// A stable, versioned identifier for an extension-field schema.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct FieldId {
    namespace: String,
    name: String,
    version: u16,
}

#[derive(Deserialize)]
struct FieldIdWire {
    namespace: String,
    name: String,
    version: u16,
}

impl FieldId {
    /// Creates a validated stable field identifier.
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        version: u16,
    ) -> Result<Self, FieldSchemaError> {
        let id = Self {
            namespace: namespace.into(),
            name: name.into(),
            version,
        };
        id.validate()?;
        Ok(id)
    }

    /// Returns the field namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the field name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the field schema version.
    pub const fn version(&self) -> u16 {
        self.version
    }

    fn validate(&self) -> Result<(), FieldSchemaError> {
        validate_component(&self.namespace, "field namespace")?;
        validate_component(&self.name, "field name")?;
        if self.version == 0 {
            return Err(FieldSchemaError::ZeroVersion);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for FieldId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FieldIdWire::deserialize(deserializer)?;
        Self::new(wire.namespace, wire.name, wire.version).map_err(D::Error::custom)
    }
}

/// A kind of authored world entity that can own field values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    /// A biological species.
    Species,
    /// A culture.
    Culture,
    /// A settlement.
    Settlement,
    /// A polity.
    Polity,
}

/// A stable identifier target that a field payload may reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StableIdKind {
    /// A spatial cell identifier.
    Cell,
    /// A spatial edge identifier.
    Edge,
    /// A species identifier.
    Species,
    /// A culture identifier.
    Culture,
    /// A settlement identifier.
    Settlement,
    /// A polity identifier.
    Polity,
}

/// The collection of world objects over which a field is defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldDomain {
    /// One value for the whole world.
    Global,
    /// One value per spatial cell.
    Cells,
    /// One value per spatial edge.
    Edges,
    /// One value per entity of the selected kind.
    Entities(
        /// The exact entity kind that owns the values.
        EntityKind,
    ),
}

/// The exact payload representation required by a field schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldValueType {
    /// Finite 32-bit floating-point scalar values.
    ScalarF32,
    /// Unsigned category keys declared by the schema.
    CategoryU32,
    /// Boolean values.
    Boolean,
    /// Finite two-component 32-bit floating-point vectors.
    Vector2F32,
    /// Unsigned stable identifiers of the selected target kind.
    StableIdU32(
        /// The exact kind of object referenced by the values.
        StableIdKind,
    ),
}

/// The V1 behavior when a field value is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissingValuePolicy {
    /// Every object in the field domain must have a value.
    Forbidden,
}

/// The semantic unit attached to field values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldUnit {
    /// A dimensionless value.
    Unitless,
    /// A domain-defined unit.
    Custom {
        /// The namespace that owns the unit definition.
        namespace: String,
        /// The stable unit name.
        name: String,
        /// The human-readable unit symbol.
        symbol: String,
    },
}

impl FieldUnit {
    /// Returns the human-readable unit symbol, or an empty string for unitless values.
    pub fn symbol(&self) -> &str {
        match self {
            Self::Unitless => "",
            Self::Custom { symbol, .. } => symbol,
        }
    }
}

/// An inclusive finite range for scalar field values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ValueRange {
    min: f32,
    max: f32,
}

#[derive(Deserialize)]
struct ValueRangeWire {
    min: f32,
    max: f32,
}

impl ValueRange {
    /// Creates an inclusive range with finite, ordered bounds.
    pub fn new(min: f32, max: f32) -> Result<Self, FieldSchemaError> {
        if !min.is_finite() || !max.is_finite() || min > max {
            return Err(FieldSchemaError::InvalidRange);
        }
        Ok(Self { min, max })
    }

    /// Returns the inclusive lower bound.
    pub const fn min(self) -> f32 {
        self.min
    }

    /// Returns the inclusive upper bound.
    pub const fn max(self) -> f32 {
        self.max
    }

    pub(crate) fn contains(self, value: f32) -> bool {
        value >= self.min && value <= self.max
    }
}

impl<'de> Deserialize<'de> for ValueRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ValueRangeWire::deserialize(deserializer)?;
        Self::new(wire.min, wire.max).map_err(D::Error::custom)
    }
}

/// A semantic palette family suitable for displaying a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldPaletteHint {
    /// A monotonic palette for scalar magnitude.
    Sequential,
    /// A two-sided palette for scalar deviation.
    Diverging,
    /// A palette that distinguishes discrete values.
    Categorical,
    /// A two-state palette.
    Boolean,
    /// A vector-oriented display.
    Vector,
}

/// Renderer-independent metadata for presenting a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldDisplayMetadata {
    label_key: String,
    palette: FieldPaletteHint,
    decimal_places: u8,
}

#[derive(Deserialize)]
struct FieldDisplayMetadataWire {
    label_key: String,
    palette: FieldPaletteHint,
    decimal_places: u8,
}

impl FieldDisplayMetadata {
    /// Creates validated semantic display metadata.
    pub fn new(
        label_key: impl Into<String>,
        palette: FieldPaletteHint,
        decimal_places: u8,
    ) -> Result<Self, FieldSchemaError> {
        let metadata = Self {
            label_key: label_key.into(),
            palette,
            decimal_places,
        };
        metadata.validate()?;
        Ok(metadata)
    }

    /// Returns the localization key for the field label.
    pub fn label_key(&self) -> &str {
        &self.label_key
    }

    /// Returns the semantic palette family.
    pub const fn palette(&self) -> FieldPaletteHint {
        self.palette
    }

    /// Returns the suggested number of decimal places.
    pub const fn decimal_places(&self) -> u8 {
        self.decimal_places
    }

    fn validate(&self) -> Result<(), FieldSchemaError> {
        validate_component(&self.label_key, "field label localization key")?;
        if self.decimal_places > MAX_DECIMAL_PLACES {
            return Err(FieldSchemaError::InvalidDecimalPlaces(self.decimal_places));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for FieldDisplayMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FieldDisplayMetadataWire::deserialize(deserializer)?;
        Self::new(wire.label_key, wire.palette, wire.decimal_places).map_err(D::Error::custom)
    }
}

/// A complete schema for one extension field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    /// The stable, versioned field identifier.
    pub id: FieldId,
    /// The objects over which values are defined.
    pub domain: FieldDomain,
    /// The required payload representation.
    pub value_type: FieldValueType,
    /// The semantic unit of the values.
    pub unit: FieldUnit,
    /// The optional inclusive scalar range.
    pub valid_range: Option<ValueRange>,
    /// The missing-value behavior.
    pub missing: MissingValuePolicy,
    /// Other registered fields required by this field.
    pub dependencies: Vec<FieldId>,
    /// Allowed category keys and their localization keys.
    pub category_labels: BTreeMap<u32, String>,
    /// Renderer-independent display metadata.
    pub display: FieldDisplayMetadata,
}

/// A mutable collector that validates candidate schemas before registry construction.
#[derive(Debug, Clone, Default)]
pub struct FieldRegistryBuilder {
    schemas: BTreeMap<FieldId, FieldSchema>,
}

impl FieldRegistryBuilder {
    /// Creates an empty registry builder.
    pub const fn new() -> Self {
        Self {
            schemas: BTreeMap::new(),
        }
    }

    /// Validates, normalizes, and registers one candidate schema.
    pub fn register(&mut self, mut schema: FieldSchema) -> Result<(), FieldSchemaError> {
        validate_schema(&schema)?;
        schema.dependencies.sort();
        schema.dependencies.dedup();

        if self.schemas.contains_key(&schema.id) {
            return Err(FieldSchemaError::DuplicateField(schema.id));
        }
        self.schemas.insert(schema.id.clone(), schema);
        Ok(())
    }

    /// Validates dependency closure and acyclicity, then freezes the registry.
    pub fn build(self) -> Result<FieldRegistry, FieldSchemaError> {
        validate_dependencies(&self.schemas)?;
        Ok(FieldRegistry {
            schemas: self.schemas,
        })
    }
}

/// An immutable, validated collection of field schemas.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldRegistry {
    schemas: BTreeMap<FieldId, FieldSchema>,
}

impl FieldRegistry {
    /// Returns the schema registered for an identifier.
    pub fn get(&self, id: &FieldId) -> Option<&FieldSchema> {
        self.schemas.get(id)
    }

    /// Iterates through schemas in stable identifier order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&FieldId, &FieldSchema)> {
        self.schemas.iter()
    }

    /// Returns the number of registered schemas.
    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    /// Returns whether the registry contains no schemas.
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }
}

impl Serialize for FieldRegistry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.schemas.len()))?;
        for schema in self.schemas.values() {
            sequence.serialize_element(schema)?;
        }
        sequence.end()
    }
}

impl<'de> Deserialize<'de> for FieldRegistry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let schemas = Vec::<FieldSchema>::deserialize(deserializer)?;
        let mut builder = FieldRegistryBuilder::new();
        for schema in schemas {
            builder.register(schema).map_err(D::Error::custom)?;
        }
        builder.build().map_err(D::Error::custom)
    }
}

fn validate_component(value: &str, component: &'static str) -> Result<(), FieldSchemaError> {
    let bytes = value.as_bytes();
    if !(1..=MAX_IDENTIFIER_BYTES).contains(&bytes.len())
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
        || !bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(FieldSchemaError::InvalidComponent { component });
    }
    Ok(())
}

fn validate_schema(schema: &FieldSchema) -> Result<(), FieldSchemaError> {
    schema.id.validate()?;
    schema.display.validate()?;
    for dependency in &schema.dependencies {
        dependency.validate()?;
    }

    if let FieldUnit::Custom {
        namespace,
        name,
        symbol,
    } = &schema.unit
    {
        validate_component(namespace, "custom unit namespace")?;
        validate_component(name, "custom unit name")?;
        if symbol.is_empty() || symbol.len() > MAX_UNIT_SYMBOL_BYTES {
            return Err(FieldSchemaError::InvalidUnitSymbol);
        }
    }

    if schema.valid_range.is_some() && schema.value_type != FieldValueType::ScalarF32 {
        return Err(FieldSchemaError::RangeForNonScalar);
    }

    match schema.value_type {
        FieldValueType::CategoryU32 => {
            if schema.category_labels.is_empty() {
                return Err(FieldSchemaError::MissingCategoryLabels);
            }
            for label in schema.category_labels.values() {
                validate_component(label, "category label localization key")?;
            }
        }
        _ if !schema.category_labels.is_empty() => {
            return Err(FieldSchemaError::CategoryLabelsForNonCategory);
        }
        _ => {}
    }

    let palette_is_compatible = matches!(
        (schema.value_type, schema.display.palette),
        (
            FieldValueType::ScalarF32,
            FieldPaletteHint::Sequential | FieldPaletteHint::Diverging
        ) | (
            FieldValueType::CategoryU32 | FieldValueType::StableIdU32(_),
            FieldPaletteHint::Categorical
        ) | (FieldValueType::Boolean, FieldPaletteHint::Boolean)
            | (FieldValueType::Vector2F32, FieldPaletteHint::Vector)
    );
    if !palette_is_compatible {
        return Err(FieldSchemaError::IncompatiblePalette);
    }

    Ok(())
}

fn validate_dependencies(schemas: &BTreeMap<FieldId, FieldSchema>) -> Result<(), FieldSchemaError> {
    for (field, schema) in schemas {
        for dependency in &schema.dependencies {
            if !schemas.contains_key(dependency) {
                return Err(FieldSchemaError::MissingDependency {
                    field: field.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut active = BTreeSet::new();
    for root in schemas.keys() {
        if visited.contains(root) {
            continue;
        }

        active.insert(root.clone());
        let mut stack = vec![(root.clone(), 0_usize)];
        while !stack.is_empty() {
            let next_dependency = {
                let (field, dependency_index) = stack
                    .last_mut()
                    .expect("the stack is known to be non-empty");
                let dependency = schemas[field].dependencies.get(*dependency_index).cloned();
                if dependency.is_some() {
                    *dependency_index += 1;
                }
                dependency
            };

            if let Some(dependency) = next_dependency {
                if visited.contains(&dependency) {
                    continue;
                }
                if !active.insert(dependency.clone()) {
                    return Err(FieldSchemaError::DependencyCycle(dependency));
                }
                stack.push((dependency, 0));
            } else {
                let (field, _) = stack.pop().expect("the stack is known to be non-empty");
                active.remove(&field);
                visited.insert(field);
            }
        }
    }

    Ok(())
}
