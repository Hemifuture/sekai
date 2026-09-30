//! Recipe-only project transport and explicit generator compatibility.
// T4 connects these narrowly visible entry points to native project operations.

use std::{collections::BTreeMap, fmt};

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::{
    authoring::AuthorInputs,
    pipeline::{WorldPipeline, PRODUCT_RECIPE_GENERATOR_VERSION},
    spherical_presentation::{
        build_spherical_external_artifacts, formation_authored_models, formation_implicit_models,
    },
};
use crate::{
    generators::natural::{
        causal_natural_formation_graph, spherical_natural_foundation_graph,
        AuthorConstraintsArtifact, ClimateSpecArtifact, HydroErosionSpecArtifact,
        RulePackSetArtifact,
    },
    ui::spherical::{SphericalCanvasPreferencesWire, SphericalCanvasState, SphericalUiError},
    world::{natural::NaturalQualityProfile, spatial::float_platform_canary, SphericalSpaceSpec},
};

const PROJECT_MAGIC: &str = "sekai.recipe";
/// The sole project transport schema interpreted by this codec.
pub(super) const PROJECT_FORMAT_VERSION: u16 = 1;
// The full legacy graph and upgrade-source identities, all inputs/preferences and 8 KiB
// of notes measured 13,755 bytes. Four times its next power of two leaves headroom.
// This is a file-parser resource budget, not a scientific or generation threshold.
/// Maximum UTF-8 project file size accepted by the codec.
pub(super) const MAX_PROJECT_FILE_BYTES: usize = 65_536;

/// Canvas preferences and the application's independent loading-motion preference.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectView {
    canvas: SphericalCanvasPreferencesWire,
    reduce_motion: bool,
}

impl ProjectView {
    /// Captures canvas preferences and `TemplateApp::reduce_loading_motion` independently.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_state(canvas: &SphericalCanvasState, reduce_motion: bool) -> Self {
        Self {
            canvas: SphericalCanvasPreferencesWire::from_state(canvas),
            reduce_motion,
        }
    }

    /// Restores validated canvas preferences and returns the independent loading-motion flag.
    /// Returns a UI input error when stored projection, cameras or field IDs are invalid.
    pub fn try_into_state(self) -> Result<(SphericalCanvasState, bool), SphericalUiError> {
        Ok((self.canvas.try_into_state()?, self.reduce_motion))
    }
}

/// Input-only project; generator differences remain available for explicit comparison.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProjectFile {
    pub format_version: u16,
    pub inputs: AuthorInputs,
    pub generator: GeneratorIdentity,
    pub view: ProjectView,
    pub extensions: BTreeMap<String, Value>,
    pub upgraded_from: Option<GeneratorIdentity>,
}

impl ProjectFile {
    /// Creates a current-format project; invalid inputs or compiled definitions return an error.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(inputs: AuthorInputs) -> Result<Self, ProjectFileError> {
        let generator = current_generator_identity(&inputs)?;
        Ok(Self {
            format_version: PROJECT_FORMAT_VERSION,
            inputs,
            generator,
            view: ProjectView::default(),
            extensions: BTreeMap::new(),
            upgraded_from: None,
        })
    }
}

/// Actual registered graph stage identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GeneratorStage {
    pub namespace: String,
    pub id: String,
    pub version: u32,
}

/// Recorded profile policy; historical values stay readable when current policy changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GeneratorProfilePlan {
    pub schema_version: u16,
    pub profile: NaturalQualityProfile,
    pub authoritative_target_cell_count: u32,
    pub authoritative_resolved_cell_count: u32,
    pub tectonic_control_target_cell_count: u32,
    pub tectonic_control_resolved_cell_count: u32,
    pub climate_face_resolution: u16,
    pub global_circulation_formation_cycles_max: u16,
}

/// Semantic generation identity. Application release versions are diagnostic only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GeneratorIdentity {
    pub pipeline: WorldPipeline,
    pub product_recipe_version: u32,
    pub stages: Vec<GeneratorStage>,
    #[serde(deserialize_with = "explicit_option")]
    pub profile_plan: Option<GeneratorProfilePlan>,
    pub fixed_input_hashes: BTreeMap<String, [u8; 32]>,
    /// Environment signal, not proof of bit-identical results on different platforms.
    pub float_platform_canary: [u8; 32],
}

/// Explicit decision before rebuilding a decoded project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Compatibility {
    Exact,
    PlatformConfirmationRequired,
    UpgradeRequired(Vec<String>),
}

/// Project rejection categories, before any authoring session is replaced.
#[derive(Debug, thiserror::Error)]
pub(super) enum ProjectFileError {
    #[error("invalid project format: {0}")]
    Format(String),
    #[error("project exceeds resource limit: {0}")]
    Limit(String),
    #[error("project checksum mismatch")]
    Checksum,
    #[error("invalid project input: {0}")]
    Input(String),
    #[error("unsupported project schema: {0}")]
    UnsupportedSchema(String),
}

fn input_error(error: impl fmt::Display) -> ProjectFileError {
    ProjectFileError::Input(error.to_string())
}

fn explicit_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

/// Reads graph/profile/fixed-input facts without generating P1 or running any stage.
/// Returns an input error for invalid authored inputs or compiled definitions.
pub(super) fn current_generator_identity(
    inputs: &AuthorInputs,
) -> Result<GeneratorIdentity, ProjectFileError> {
    inputs.validate().map_err(input_error)?;
    let graph = match inputs.world_pipeline {
        WorldPipeline::Formation => causal_natural_formation_graph(),
        WorldPipeline::LegacyFoundation => spherical_natural_foundation_graph(),
    }
    .map_err(input_error)?;
    let stages = graph
        .descriptors()
        .iter()
        .map(|stage| GeneratorStage {
            namespace: stage.namespace().to_owned(),
            id: stage.id().as_str().to_owned(),
            version: stage.version(),
        })
        .collect();
    let mut fixed_input_hashes = BTreeMap::new();
    let profile_plan = match inputs.world_pipeline {
        WorldPipeline::Formation => {
            let profile = inputs.formation_quality_profile;
            // Formation chooses resolution from the profile; the legacy authored cell count
            // is preserved but cannot reject a valid Standard or High project.
            let plan = profile
                .resolve(&SphericalSpaceSpec {
                    radius: inputs.spherical_space_spec.radius,
                    target_cell_count: profile.authoritative_target_cell_count(),
                })
                .map_err(input_error)?;
            let (climate, hydro) = formation_implicit_models().map_err(input_error)?;
            fixed_input_hashes.insert("formation.climate".into(), serialized_hash(&climate)?);
            fixed_input_hashes.insert("formation.hydro-erosion".into(), serialized_hash(&hydro)?);
            let (tectonic, geologic) =
                formation_authored_models(&inputs.tectonic_spec, &inputs.geologic_spec)
                    .map_err(input_error)?;
            // Only interpretation is algorithm identity; editable values belong to RecipeId.
            fixed_input_hashes.insert(
                "formation.authored-models".into(),
                serialized_hash(&(
                    tectonic.model(),
                    tectonic.spec().schema_version,
                    geologic.model(),
                    geologic.spec().schema_version,
                    inputs.formation_spec.schema_version,
                    inputs.relief_spec.schema_version,
                ))?,
            );
            Some(GeneratorProfilePlan {
                schema_version: plan.schema_version(),
                profile: plan.profile(),
                authoritative_target_cell_count: plan.authoritative_target_cell_count(),
                authoritative_resolved_cell_count: plan.authoritative_resolved_cell_count(),
                tectonic_control_target_cell_count: plan.tectonic_control_target_cell_count(),
                tectonic_control_resolved_cell_count: plan.tectonic_control_resolved_cell_count(),
                climate_face_resolution: plan.climate_face_resolution(),
                global_circulation_formation_cycles_max: profile
                    .global_circulation_formation_cycles_max(),
            })
        }
        WorldPipeline::LegacyFoundation => {
            let external = build_spherical_external_artifacts(
                &inputs.spherical_space_spec,
                &inputs.formation_spec,
                &inputs.tectonic_spec,
                &inputs.relief_spec,
                &inputs.geologic_spec,
            )
            .map_err(input_error)?;
            // Only fixed dependencies actually consumed by this graph. The pack artifact
            // includes rule manifests, contributions and dependency content identities.
            fixed_input_hashes.insert(
                "legacy.climate".into(),
                *external
                    .hash::<ClimateSpecArtifact>()
                    .map_err(input_error)?
                    .as_bytes(),
            );
            fixed_input_hashes.insert(
                "legacy.hydro-erosion".into(),
                *external
                    .hash::<HydroErosionSpecArtifact>()
                    .map_err(input_error)?
                    .as_bytes(),
            );
            fixed_input_hashes.insert(
                "legacy.rules".into(),
                *external
                    .hash::<RulePackSetArtifact>()
                    .map_err(input_error)?
                    .as_bytes(),
            );
            fixed_input_hashes.insert(
                "legacy.constraints".into(),
                *external
                    .hash::<AuthorConstraintsArtifact>()
                    .map_err(input_error)?
                    .as_bytes(),
            );
            None
        }
    };
    Ok(GeneratorIdentity {
        pipeline: inputs.world_pipeline,
        product_recipe_version: PRODUCT_RECIPE_GENERATOR_VERSION,
        stages,
        profile_plan,
        fixed_input_hashes,
        float_platform_canary: float_platform_canary(),
    })
}

fn serialized_hash(value: &impl Serialize) -> Result<[u8; 32], ProjectFileError> {
    Ok(*blake3::hash(&serde_json::to_vec(value).map_err(input_error)?).as_bytes())
}

/// Compares semantic components first, then the environment canary.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn compare_generator(
    saved: &GeneratorIdentity,
    current: &GeneratorIdentity,
) -> Compatibility {
    let mut differences = Vec::new();
    for (changed, label) in [
        (saved.pipeline != current.pipeline, "pipeline"),
        (
            saved.product_recipe_version != current.product_recipe_version,
            "product recipe version",
        ),
        (saved.stages != current.stages, "stage namespace/id/version"),
        (
            saved.profile_plan != current.profile_plan,
            "profile resolution plan",
        ),
        (
            saved.fixed_input_hashes != current.fixed_input_hashes,
            "fixed model/rule/dependency content",
        ),
    ] {
        if changed {
            differences.push(label.to_owned());
        }
    }
    if !differences.is_empty() {
        Compatibility::UpgradeRequired(differences)
    } else if saved.float_platform_canary != current.float_platform_canary {
        Compatibility::PlatformConfirmationRequired
    } else {
        Compatibility::Exact
    }
}

/// Copies an already decoded/validated project after an explicit upgrade choice.
/// Preserves the original project and records its saved generator identity in the copy.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn upgrade_copy(file: &ProjectFile, current: GeneratorIdentity) -> ProjectFile {
    let mut upgraded = file.clone();
    upgraded.upgraded_from = Some(file.generator.clone());
    upgraded.generator = current;
    upgraded
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectWire {
    magic: String,
    format_version: u16,
    inputs: AuthorInputs,
    generator: GeneratorIdentity,
    view: ProjectView,
    extensions: BTreeMap<String, Value>,
    upgraded_from: Option<GeneratorIdentity>,
    checksum: [u8; 32],
}

#[derive(Serialize)]
struct ChecksumPayload<'a> {
    magic: &'static str,
    format_version: u16,
    inputs: Vec<u8>,
    generator: &'a GeneratorIdentity,
    view: &'a ProjectView,
    extensions: &'a BTreeMap<String, Value>,
    upgraded_from: &'a Option<GeneratorIdentity>,
}

fn checksum(file: &ProjectFile) -> Result<[u8; 32], ProjectFileError> {
    serialized_hash(&ChecksumPayload {
        magic: PROJECT_MAGIC,
        format_version: file.format_version,
        // T1 supplies the canonical field order, finite validation and negative-zero policy.
        inputs: file.inputs.canonical_bytes().map_err(input_error)?,
        generator: &file.generator,
        view: &file.view,
        extensions: &file.extensions,
        upgraded_from: &file.upgraded_from,
    })
}

fn validate_file(file: &ProjectFile) -> Result<(), ProjectFileError> {
    if file.format_version != PROJECT_FORMAT_VERSION {
        return Err(ProjectFileError::UnsupportedSchema(format!(
            "format version {}",
            file.format_version
        )));
    }
    file.inputs.validate().map_err(input_error)?;
    file.view.clone().try_into_state().map_err(input_error)?;
    if file.generator.pipeline != file.inputs.world_pipeline {
        return Err(ProjectFileError::Input(
            "recipe/generator pipeline mismatch".into(),
        ));
    }
    for (key, value) in &file.extensions {
        validate_metadata(key, value)?;
    }
    Ok(())
}

fn validate_metadata(key: &str, value: &Value) -> Result<(), ProjectFileError> {
    let lower = key.to_ascii_lowercase();
    if [
        "world",
        "world_bundle",
        "snapshot",
        "world_snapshot",
        "cache",
        "stage_cache",
        "history",
        "mesh",
        "trajectory",
        "dense_arrays",
        "stages",
        "fields",
        "gpu",
        "generator",
        "inputs",
        "cells",
        "edges",
        "vertices",
        "solver_state",
        "surface",
    ]
    .contains(&lower.as_str())
    {
        return Err(ProjectFileError::UnsupportedSchema(format!(
            "reserved extension {key}"
        )));
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        Value::Array(tags) if tags.iter().all(Value::is_string) => Ok(()),
        Value::Object(metadata) => {
            for (key, value) in metadata {
                validate_metadata(key, value)?;
            }
            Ok(())
        }
        Value::Array(_) => Err(ProjectFileError::UnsupportedSchema(format!(
            "extension {key} arrays must contain text tags"
        ))),
    }
}

/// Validates and encodes V1. Returns input/schema/limit errors before producing a file.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn encode_project(file: &ProjectFile) -> Result<Vec<u8>, ProjectFileError> {
    validate_file(file)?;
    let wire = ProjectWire {
        magic: PROJECT_MAGIC.to_owned(),
        format_version: file.format_version,
        inputs: file.inputs.clone(),
        generator: file.generator.clone(),
        view: file.view.clone(),
        extensions: file.extensions.clone(),
        upgraded_from: file.upgraded_from.clone(),
        checksum: checksum(file)?,
    };
    let bytes = serde_json::to_vec(&wire).map_err(input_error)?;
    check_size(bytes.len())?;
    Ok(bytes)
}

fn check_size(size: usize) -> Result<(), ProjectFileError> {
    if size > MAX_PROJECT_FILE_BYTES {
        return Err(ProjectFileError::Limit(format!("{size} bytes")));
    }
    Ok(())
}

/// Decodes only recognized input schemas; unknown algorithms remain saved identities to compare.
/// Rejects duplicate keys, unknown fields, invalid inputs, checksums and resource excesses.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn decode_project(bytes: &[u8]) -> Result<ProjectFile, ProjectFileError> {
    check_size(bytes.len())?;
    // Duplicate-key rejection also covers extension maps. Default serde recursion limits stay on.
    let strict: UniqueJson = serde_json::from_slice(bytes).map_err(parse_error)?;
    let object = strict
        .0
        .as_object()
        .ok_or_else(|| ProjectFileError::Format("object required".into()))?;
    if object.get("magic").and_then(Value::as_str) != Some(PROJECT_MAGIC) {
        return Err(ProjectFileError::Format("unrecognized magic".into()));
    }
    if object.get("format_version").and_then(Value::as_u64)
        != Some(u64::from(PROJECT_FORMAT_VERSION))
    {
        return Err(ProjectFileError::UnsupportedSchema("format version".into()));
    }
    // Serde normally allows absent Option fields. V1 deliberately requires them explicitly.
    for required in [
        "upgraded_from",
        "extensions",
        "view",
        "generator",
        "inputs",
        "checksum",
    ] {
        if !object.contains_key(required) {
            return Err(ProjectFileError::Format(format!(
                "missing field {required}"
            )));
        }
    }
    drop(strict);
    let wire: ProjectWire = serde_json::from_slice(bytes).map_err(parse_error)?;
    let file = ProjectFile {
        format_version: wire.format_version,
        inputs: wire.inputs,
        generator: wire.generator,
        view: wire.view,
        extensions: wire.extensions,
        upgraded_from: wire.upgraded_from,
    };
    validate_file(&file)?;
    if checksum(&file)? != wire.checksum {
        return Err(ProjectFileError::Checksum);
    }
    Ok(file)
}

fn parse_error(error: serde_json::Error) -> ProjectFileError {
    let message = error.to_string();
    if message.contains("recursion limit") {
        ProjectFileError::Limit(message)
    } else if message.contains("unknown") || message.contains("unsupported") {
        ProjectFileError::UnsupportedSchema(message)
    } else if error.is_data() && !message.contains("duplicate") && !message.contains("missing") {
        ProjectFileError::Input(message)
    } else {
        ProjectFileError::Format(message)
    }
}

struct UniqueJson(Value);
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> de::Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| UniqueJson(Value::Number(number)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<UniqueJson>()? {
                    values.push(value.0);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!("duplicate key {key}")));
                    }
                    values.insert(key, map.next_value::<UniqueJson>()?.0);
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(pipeline: WorldPipeline) -> ProjectFile {
        ProjectFile::new(AuthorInputs {
            world_pipeline: pipeline,
            ..AuthorInputs::default()
        })
        .unwrap()
    }
    fn mutate_json(file: &ProjectFile, mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut wire: Value = serde_json::from_slice(&encode_project(file).unwrap()).unwrap();
        mutate(&mut wire);
        serde_json::to_vec(&wire).unwrap()
    }

    #[test]
    fn recipe_round_trip_preserves_inactive_authored_values() {
        let mut file = project(WorldPipeline::Formation);
        file.inputs.relief_spec.target_land_fraction = f32::from_bits(0.375_f32.to_bits() + 1);
        file.inputs.spherical_space_spec.target_cell_count += 1;
        file.extensions.insert("name".into(), "Recipe A".into());
        file.extensions
            .insert("tags".into(), serde_json::json!(["test", "author"]));
        let decoded = decode_project(&encode_project(&file).unwrap()).unwrap();
        assert_eq!(decoded, file);
        assert_eq!(
            decoded.inputs.recipe_id().unwrap(),
            file.inputs.recipe_id().unwrap()
        );
    }

    #[test]
    fn whitespace_and_object_order_preserve_checksum_and_recipe_identity() {
        let file = project(WorldPipeline::LegacyFoundation);
        let bytes = encode_project(&file).unwrap();
        let sorted: Value = serde_json::from_slice(&bytes).unwrap();
        let reordered = serde_json::to_vec_pretty(&sorted).unwrap();
        assert_ne!(bytes, reordered);
        let decoded = decode_project(&reordered).unwrap();
        assert_eq!(
            decoded.inputs.recipe_id().unwrap(),
            file.inputs.recipe_id().unwrap()
        );
        assert_eq!(checksum(&decoded).unwrap(), checksum(&file).unwrap());
    }

    #[test]
    fn checksum_excludes_itself_and_detects_corruption() {
        let file = project(WorldPipeline::Formation);
        let wire: ProjectWire = serde_json::from_slice(&encode_project(&file).unwrap()).unwrap();
        assert_eq!(wire.checksum, checksum(&file).unwrap());
        for bytes in [
            mutate_json(&file, |wire| {
                wire["checksum"][0] = (wire["checksum"][0].as_u64().unwrap() ^ 1).into()
            }),
            mutate_json(&file, |wire| wire["inputs"]["world_seed"] = 987.into()),
            mutate_json(&file, |wire| wire["extensions"]["name"] = "Changed".into()),
        ] {
            assert!(matches!(
                decode_project(&bytes),
                Err(ProjectFileError::Checksum)
            ));
        }
    }

    #[test]
    fn unknown_core_versions_fields_enums_and_missing_fields_fail() {
        let file = project(WorldPipeline::Formation);
        for bytes in [
            mutate_json(&file, |wire| wire["future_core"] = true.into()),
            mutate_json(&file, |wire| wire["format_version"] = 99.into()),
            mutate_json(&file, |wire| {
                wire["inputs"]["formation_spec"]["schema_version"] = 99.into()
            }),
            mutate_json(&file, |wire| {
                wire["inputs"]["world_pipeline"] = "Unknown".into()
            }),
            mutate_json(&file, |wire| {
                wire["inputs"]["tectonic_spec"]["extra"] = 1.into()
            }),
            mutate_json(&file, |wire| {
                wire["inputs"]["relief_spec"]["extra"] = 1.into()
            }),
            mutate_json(&file, |wire| {
                wire["inputs"]["geologic_spec"]["extra"] = 1.into()
            }),
            mutate_json(&file, |wire| {
                wire["inputs"]["formation_spec"]["extra"] = 1.into()
            }),
            mutate_json(&file, |wire| {
                wire["generator"]["stages"][0]["extra"] = 1.into()
            }),
            mutate_json(&file, |wire| {
                wire["view"]["canvas"]["map_cameras"]["extra"] = 1.into()
            }),
            mutate_json(&file, |wire| {
                wire.as_object_mut().unwrap().remove("upgraded_from");
            }),
        ] {
            assert!(
                decode_project(&bytes).is_err(),
                "accepted {}",
                String::from_utf8_lossy(&bytes)
            );
        }
    }

    #[test]
    fn duplicate_keys_truncation_and_depth_fail() {
        let text =
            String::from_utf8(encode_project(&project(WorldPipeline::Formation)).unwrap()).unwrap();
        for duplicate in [
            text.replacen(
                "\"format_version\":1",
                "\"format_version\":1,\"format_version\":1",
                1,
            ),
            text.replacen(
                "\"extensions\":{}",
                "\"extensions\":{\"name\":\"a\",\"name\":\"b\"}",
                1,
            ),
            text.replacen(
                "\"world_seed\":42",
                "\"world_seed\":42,\"world_seed\":42",
                1,
            ),
        ] {
            assert!(matches!(
                decode_project(duplicate.as_bytes()),
                Err(ProjectFileError::Format(_))
            ));
        }
        assert!(matches!(
            decode_project(&text.as_bytes()[..text.len() - 1]),
            Err(ProjectFileError::Format(_))
        ));
        let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        assert!(matches!(
            decode_project(nested.as_bytes()),
            Err(ProjectFileError::Limit(_))
        ));
    }

    #[test]
    fn nonfinite_inputs_fail_and_negative_zero_uses_t1_identity() {
        let file = project(WorldPipeline::Formation);
        for invalid in [f32::NAN, f32::INFINITY] {
            let mut changed = file.clone();
            changed.inputs.relief_spec.water_inventory_ratio = invalid;
            assert!(matches!(
                encode_project(&changed),
                Err(ProjectFileError::Input(_))
            ));
        }
        // Current author float domains all exclude zero. T1 validation must run before
        // encoding, so neither sign of zero can be used to bypass the domain contract.
        for zero in [-0.0, 0.0] {
            let mut changed = file.clone();
            changed.inputs.tectonic_spec.continental_crust_fraction = zero;
            assert!(changed.inputs.recipe_id().is_err());
            assert!(matches!(
                encode_project(&changed),
                Err(ProjectFileError::Input(_))
            ));
        }
    }

    #[test]
    fn algorithm_differences_require_upgrade_and_platform_only_requires_confirmation() {
        let current = project(WorldPipeline::Formation).generator;
        assert_eq!(compare_generator(&current, &current), Compatibility::Exact);
        let mut saved = current.clone();
        saved.float_platform_canary[0] ^= 1;
        assert_eq!(
            compare_generator(&saved, &current),
            Compatibility::PlatformConfirmationRequired
        );
        for saved in [
            {
                let mut v = current.clone();
                v.stages[0].version += 1;
                v
            },
            {
                let mut v = current.clone();
                v.stages[0].id = "unknown.algorithm".into();
                v
            },
            {
                let mut v = current.clone();
                v.product_recipe_version += 1;
                v
            },
            {
                let mut v = current.clone();
                v.fixed_input_hashes.values_mut().next().unwrap()[0] ^= 1;
                v
            },
            {
                let mut v = current.clone();
                v.profile_plan.as_mut().unwrap().climate_face_resolution += 1;
                v
            },
        ] {
            assert!(
                matches!(compare_generator(&saved,&current), Compatibility::UpgradeRequired(reasons) if !reasons.is_empty())
            );
        }
        let legacy = project(WorldPipeline::LegacyFoundation).generator;
        let mut changed = legacy.clone();
        changed.fixed_input_hashes.get_mut("legacy.rules").unwrap()[0] ^= 1;
        assert!(matches!(
            compare_generator(&changed, &legacy),
            Compatibility::UpgradeRequired(_)
        ));
    }

    #[test]
    fn standard_profile_uses_authoritative_resolution_and_keeps_authored_count() {
        let inputs = AuthorInputs {
            world_pipeline: WorldPipeline::Formation,
            formation_quality_profile: NaturalQualityProfile::Standard,
            ..AuthorInputs::default()
        };
        let file = ProjectFile::new(inputs.clone()).unwrap();
        let plan = file.generator.profile_plan.as_ref().unwrap();
        assert_eq!(
            plan.authoritative_target_cell_count,
            inputs
                .formation_quality_profile
                .authoritative_target_cell_count()
        );
        assert_ne!(
            plan.authoritative_target_cell_count,
            inputs.spherical_space_spec.target_cell_count
        );
        assert_eq!(
            decode_project(&encode_project(&file).unwrap())
                .unwrap()
                .inputs,
            inputs
        );
    }

    #[test]
    fn author_edits_and_writer_versions_are_not_algorithm_changes() {
        for pipeline in [WorldPipeline::Formation, WorldPipeline::LegacyFoundation] {
            let mut file = project(pipeline);
            let before = file.generator.clone();
            file.inputs.world_seed += 1;
            file.inputs.tectonic_spec.plate_count += 1;
            file.inputs.relief_spec.target_land_fraction = 0.4;
            file.inputs.relief_spec.water_inventory_ratio = 0.8;
            file.inputs.geologic_spec.hotspot_count += 1;
            file.inputs.spherical_space_spec.radius =
                crate::world::Meters::new(7_000_000.0).unwrap();
            file.inputs.spherical_space_spec.target_cell_count += 1;
            assert_eq!(
                compare_generator(&before, &current_generator_identity(&file.inputs).unwrap()),
                Compatibility::Exact
            );
            file.extensions
                .insert("writer_version".into(), env!("CARGO_PKG_VERSION").into());
            assert_eq!(
                compare_generator(
                    &decode_project(&encode_project(&file).unwrap())
                        .unwrap()
                        .generator,
                    &before
                ),
                Compatibility::Exact
            );
        }
    }

    #[test]
    fn explicit_upgrade_copy_preserves_source_identity_and_bytes() {
        let mut file = project(WorldPipeline::Formation);
        let current = file.generator.clone();
        file.generator.stages[0].version += 10;
        file.generator
            .profile_plan
            .as_mut()
            .unwrap()
            .climate_face_resolution += 1;
        let source = encode_project(&file).unwrap();
        let decoded = decode_project(&source).unwrap();
        assert!(matches!(
            compare_generator(&decoded.generator, &current),
            Compatibility::UpgradeRequired(_)
        ));
        let upgraded = upgrade_copy(&decoded, current.clone());
        assert_eq!(upgraded.upgraded_from, Some(file.generator.clone()));
        assert_eq!(upgraded.generator, current);
        assert_eq!(encode_project(&file).unwrap(), source);
        assert_eq!(
            decode_project(&encode_project(&upgraded).unwrap()).unwrap(),
            upgraded
        );
    }

    #[test]
    fn wire_is_recipe_only_and_extensions_reject_generated_state_containers() {
        let file = project(WorldPipeline::Formation);
        let wire: Value = serde_json::from_slice(&encode_project(&file).unwrap()).unwrap();
        let keys: Vec<&str> = wire
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "checksum",
                "extensions",
                "format_version",
                "generator",
                "inputs",
                "magic",
                "upgraded_from",
                "view"
            ]
        );
        assert!(wire["view"]["canvas"].get("selected_entity").is_none());
        assert!(wire["view"]["canvas"].get("vector_phase").is_none());
        for (name, value) in [
            ("world_snapshot", serde_json::json!("opaque")),
            ("cache", serde_json::json!({"x":1})),
            ("measurements", serde_json::json!([0.1, 0.2])),
            ("state", serde_json::json!({"cells":[1,2]})),
        ] {
            let mut changed = file.clone();
            changed.extensions.insert(name.into(), value);
            assert!(matches!(
                encode_project(&changed),
                Err(ProjectFileError::UnsupportedSchema(_))
            ));
        }
    }

    #[test]
    fn measure_complete_parameter_preference_metadata_fixture() {
        let mut file = project(WorldPipeline::LegacyFoundation);
        file.inputs.world_seed = u64::MAX;
        file.upgraded_from = Some(file.generator.clone());
        let mut view: Value = serde_json::to_value(&file.view).unwrap();
        for channel in ["fill_field", "overlay_field"] {
            view["canvas"][channel] =
                serde_json::to_value(crate::world::natural::surface_elevation_m_field_id())
                    .unwrap();
        }
        file.view = serde_json::from_value(view).unwrap();
        file.extensions.insert("name".into(), "完整配方".into());
        file.extensions
            .insert("notes".into(), "n".repeat(8192).into());
        file.extensions
            .insert("tags".into(), serde_json::json!(["author", "sample"]));
        let bytes = encode_project(&file).unwrap();
        eprintln!(
            "complete recipe/preferences/metadata fixture: {} bytes",
            bytes.len()
        );
        assert_eq!(decode_project(&bytes).unwrap(), file);
    }

    #[test]
    fn nested_metadata_round_trips_and_all_optional_core_keys_are_explicit() {
        let mut file = project(WorldPipeline::LegacyFoundation);
        file.extensions.insert(
            "metadata".into(),
            serde_json::json!({
                "author": "A", "tags": ["example"], "description": {"language": "zh"}
            }),
        );
        assert_eq!(
            decode_project(&encode_project(&file).unwrap()).unwrap(),
            file
        );
        let file = project(WorldPipeline::LegacyFoundation);
        for bytes in [
            mutate_json(&file, |wire| {
                wire["generator"]
                    .as_object_mut()
                    .unwrap()
                    .remove("profile_plan");
            }),
            mutate_json(&file, |wire| {
                wire["view"]["canvas"]
                    .as_object_mut()
                    .unwrap()
                    .remove("fill_field");
            }),
            mutate_json(&file, |wire| {
                wire["view"]["canvas"]
                    .as_object_mut()
                    .unwrap()
                    .remove("overlay_field");
            }),
        ] {
            assert!(matches!(
                decode_project(&bytes),
                Err(ProjectFileError::Format(_))
            ));
        }
    }

    #[test]
    fn file_resource_boundary_accepts_exact_limit_and_rejects_excess_before_parsing() {
        let file = project(WorldPipeline::Formation);
        let mut bytes = encode_project(&file).unwrap();
        bytes.resize(MAX_PROJECT_FILE_BYTES, b' ');
        assert_eq!(decode_project(&bytes).unwrap(), file);
        bytes.push(b' ');
        assert!(matches!(
            decode_project(&bytes),
            Err(ProjectFileError::Limit(_))
        ));
        let mut oversized = file;
        oversized
            .extensions
            .insert("notes".into(), "n".repeat(MAX_PROJECT_FILE_BYTES).into());
        assert!(matches!(
            encode_project(&oversized),
            Err(ProjectFileError::Limit(_))
        ));
    }

    #[test]
    fn project_preferences_share_camera_conversion_and_clear_transient_selection() {
        use crate::ui::spherical::{SphericalCanvasAction, SphericalCanvasState};
        use crate::view::{SelectedSurfaceEntity, SphericalProjectionKind, SphericalViewMode};
        let mut state = SphericalCanvasState::default();
        for action in [
            SphericalCanvasAction::PanMap {
                delta: [0.25, -0.5],
            },
            SphericalCanvasAction::ZoomMap {
                factor: 2.0,
                anchor: [0.0, 0.0],
            },
            SphericalCanvasAction::SetProjectionKind(SphericalProjectionKind::Equirectangular),
            SphericalCanvasAction::PanMap {
                delta: [-0.75, 0.5],
            },
            SphericalCanvasAction::SetCentralMeridianRadians(0.5),
            SphericalCanvasAction::ZoomGlobe { factor: 1.25 },
            SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe),
            SphericalCanvasAction::SelectFill(crate::world::natural::surface_elevation_m_field_id()),
            SphericalCanvasAction::SetVectorPaused(true),
            SphericalCanvasAction::SelectEntity(Some(SelectedSurfaceEntity::Cell(
                crate::world::CellId::from_raw(3),
            ))),
        ] {
            state.apply(action).unwrap();
        }
        let mut file = project(WorldPipeline::Formation);
        file.view = ProjectView::from_state(&state, false);
        let (restored, reduce_motion) = decode_project(&encode_project(&file).unwrap())
            .unwrap()
            .view
            .try_into_state()
            .unwrap();
        assert!(!reduce_motion);
        assert_eq!(restored.map_camera(), state.map_camera());
        assert_eq!(restored.globe_camera(), state.globe_camera());
        assert_eq!(restored.projection(), state.projection());
        assert_eq!(restored.view_mode(), state.view_mode());
        assert_eq!(
            restored.field_state().fill_field(),
            state.field_state().fill_field()
        );
        assert!(restored.field_state().vector_paused());
        assert_eq!(restored.field_state().selected_entity(), None);
        for bytes in [
            mutate_json(&file, |wire| {
                wire["view"]["canvas"]["fill_field"]["unknown"] = true.into()
            }),
            mutate_json(&file, |wire| {
                wire["view"]["canvas"]["map_cameras"]["equal_earth_zoom"] = (-1.0).into()
            }),
            mutate_json(&file, |wire| {
                wire["view"]["canvas"]["projection_kind"] = "Unknown".into()
            }),
        ] {
            assert!(decode_project(&bytes).is_err());
        }
    }

    #[test]
    fn global_reduce_motion_is_independent_of_canvas_vector_pause() {
        use crate::ui::spherical::{SphericalCanvasAction, SphericalCanvasState};
        let mut canvas = SphericalCanvasState::default();
        canvas
            .apply(SphericalCanvasAction::SetVectorPaused(true))
            .unwrap();
        let mut file = project(WorldPipeline::Formation);
        file.view = ProjectView::from_state(&canvas, false);
        let wire: Value = serde_json::from_slice(&encode_project(&file).unwrap()).unwrap();
        assert_eq!(wire["view"]["reduce_motion"], false);
        assert_eq!(wire["view"]["canvas"]["vector_paused"], true);
        let (restored, reduce_motion) = decode_project(&encode_project(&file).unwrap())
            .unwrap()
            .view
            .try_into_state()
            .unwrap();
        assert!(!reduce_motion);
        assert!(restored.field_state().vector_paused());
        canvas
            .apply(SphericalCanvasAction::SetVectorPaused(false))
            .unwrap();
        file.view = ProjectView::from_state(&canvas, true);
        let (restored, reduce_motion) = decode_project(&encode_project(&file).unwrap())
            .unwrap()
            .view
            .try_into_state()
            .unwrap();
        assert!(reduce_motion);
        assert!(!restored.field_state().vector_paused());
    }
}
