//! Validated input transactions; session history contains recipes, never world products.

use crate::world::{
    natural::{
        GeologicSpec, GeologicSpecError, MantleActivity, NaturalQualityProfile, NaturalSpecError,
        ReliefSpec, ReliefSpecError, SeaLevelPolicy, TectonicActivity, TectonicSpec,
        WorldFormationPreset, WorldFormationSpec, WorldFormationSpecError,
    },
    SphericalSpaceSpec, SphericalSpecError,
};

use super::pipeline::{default_spherical_space_spec, WorldPipeline, PRODUCT_DEFAULT_WORLD_SEED};

/// The interactive formation default, shared by new recipes and legacy imports.
pub(super) fn default_formation_quality_profile() -> NaturalQualityProfile {
    NaturalQualityProfile::Draft
}

/// All authored generation inputs, including values inactive under the selected policy.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthorInputs {
    pub world_seed: u64,
    #[serde(with = "super::pipeline::spherical_space_spec_serde")]
    pub spherical_space_spec: SphericalSpaceSpec,
    pub world_pipeline: WorldPipeline,
    pub formation_quality_profile: NaturalQualityProfile,
    pub formation_spec: WorldFormationSpec,
    pub tectonic_spec: TectonicSpec,
    pub relief_spec: ReliefSpec,
    pub geologic_spec: GeologicSpec,
}

impl Default for AuthorInputs {
    fn default() -> Self {
        Self {
            world_seed: PRODUCT_DEFAULT_WORLD_SEED.raw(),
            spherical_space_spec: default_spherical_space_spec(),
            world_pipeline: WorldPipeline::default(),
            formation_quality_profile: default_formation_quality_profile(),
            formation_spec: WorldFormationSpec::default(),
            tectonic_spec: TectonicSpec::default(),
            relief_spec: ReliefSpec::default(),
            geologic_spec: GeologicSpec::default(),
        }
    }
}

/// A recipe failing an existing domain input contract.
#[derive(Debug, thiserror::Error)]
pub(super) enum AuthorInputError {
    #[error(transparent)]
    Space(#[from] SphericalSpecError),
    #[error(transparent)]
    Formation(#[from] WorldFormationSpecError),
    #[error(transparent)]
    Tectonic(#[from] NaturalSpecError),
    #[error(transparent)]
    Relief(#[from] ReliefSpecError),
    #[error(transparent)]
    Geologic(#[from] GeologicSpecError),
}

/// Input content identity; this does not replace the engine's artifact identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecipeId(pub [u8; 32]);

impl AuthorInputs {
    /// Validates every input, including currently inactive inputs, using domain validators.
    pub fn validate(&self) -> Result<(), AuthorInputError> {
        self.spherical_space_spec.validate()?;
        self.formation_spec.validate()?;
        self.tectonic_spec.validate()?;
        self.relief_spec.validate()?;
        self.geologic_spec.validate()?;
        Ok(())
    }

    /// Returns versioned recipe bytes for identity and project-format consumers.
    ///
    /// V1 uses this fixed field order, little-endian integers, explicit enum tags,
    /// and IEEE float bits with either zero sign encoded as positive zero. Domain
    /// validation runs first, so non-finite and unsupported values cannot be hashed.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AuthorInputError> {
        self.validate()?;
        let mut wire = Vec::with_capacity(80);
        wire.extend_from_slice(b"sekai.author-inputs\0\x01");
        wire.extend_from_slice(&self.world_seed.to_le_bytes());
        wire.extend_from_slice(
            &canonical_f64_bits(self.spherical_space_spec.radius.get()).to_le_bytes(),
        );
        wire.extend_from_slice(&self.spherical_space_spec.target_cell_count.to_le_bytes());
        wire.push(match self.world_pipeline {
            WorldPipeline::Formation => 0,
            WorldPipeline::LegacyFoundation => 1,
        });
        wire.push(match self.formation_quality_profile {
            NaturalQualityProfile::Draft => 0,
            NaturalQualityProfile::Standard => 1,
            NaturalQualityProfile::High => 2,
        });
        wire.extend_from_slice(&self.formation_spec.schema_version.to_le_bytes());
        wire.push(match self.formation_spec.preset {
            WorldFormationPreset::Random => 0,
            WorldFormationPreset::Continents => 1,
            WorldFormationPreset::Archipelago => 2,
            WorldFormationPreset::Supercontinent => 3,
            WorldFormationPreset::GreatIsland => 4,
            WorldFormationPreset::VolcanicIslands => 5,
        });
        wire.extend_from_slice(&self.tectonic_spec.schema_version.to_le_bytes());
        wire.extend_from_slice(&self.tectonic_spec.plate_count.to_le_bytes());
        wire.extend_from_slice(
            &canonical_f32_bits(self.tectonic_spec.continental_crust_fraction).to_le_bytes(),
        );
        wire.push(match self.tectonic_spec.activity {
            TectonicActivity::Quiet => 0,
            TectonicActivity::Moderate => 1,
            TectonicActivity::Active => 2,
        });
        wire.extend_from_slice(&self.relief_spec.schema_version.to_le_bytes());
        wire.extend_from_slice(
            &canonical_f32_bits(self.relief_spec.target_land_fraction).to_le_bytes(),
        );
        wire.push(match self.relief_spec.sea_level_policy {
            SeaLevelPolicy::WaterInventory => 0,
            SeaLevelPolicy::TargetLandFraction => 1,
        });
        wire.extend_from_slice(
            &canonical_f32_bits(self.relief_spec.water_inventory_ratio).to_le_bytes(),
        );
        wire.extend_from_slice(&self.geologic_spec.schema_version.to_le_bytes());
        wire.extend_from_slice(&self.geologic_spec.hotspot_count.to_le_bytes());
        wire.push(match self.geologic_spec.mantle_activity {
            MantleActivity::Quiet => 0,
            MantleActivity::Moderate => 1,
            MantleActivity::Active => 2,
        });
        Ok(wire)
    }

    /// Hashes the validated canonical recipe encoding, without generated products.
    pub fn recipe_id(&self) -> Result<RecipeId, AuthorInputError> {
        Ok(RecipeId(*blake3::hash(&self.canonical_bytes()?).as_bytes()))
    }
}

fn canonical_f32_bits(value: f32) -> u32 {
    if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}

fn canonical_f64_bits(value: f64) -> u64 {
    if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}

/// The recipe and project epoch actually captured by a save operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SaveStamp {
    pub epoch: u64,
    pub recipe_id: RecipeId,
}

/// Publication authority for one captured request in one project epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BuildToken {
    pub epoch: u64,
    pub request_id: u64,
    pub revision: u64,
    pub recipe_id: RecipeId,
}

#[derive(Debug)]
struct InputCommand {
    _label: String,
    before: AuthorInputs,
    after: AuthorInputs,
}

/// The sole committed input source and input-only history for the open project.
#[derive(Debug)]
pub(super) struct AuthoringSession {
    draft: AuthorInputs,
    recipe_id: RecipeId,
    epoch: u64,
    revision: u64,
    request_id: u64,
    active_build: Option<BuildToken>,
    saved_recipe_id: Option<RecipeId>,
    published_recipe_id: Option<RecipeId>,
    history: Vec<InputCommand>,
    cursor: usize,
}

#[cfg_attr(all(not(test), target_arch = "wasm32"), allow(dead_code))]
impl AuthoringSession {
    /// Starts an unsaved session; invalid inputs return an error before any state exists.
    pub fn new(inputs: AuthorInputs) -> Result<Self, AuthorInputError> {
        let recipe_id = inputs.recipe_id()?;
        Ok(Self {
            draft: inputs,
            recipe_id,
            epoch: 0,
            revision: 0,
            request_id: 0,
            active_build: None,
            saved_recipe_id: None,
            published_recipe_id: None,
            history: Vec::new(),
            cursor: 0,
        })
    }

    /// Commits one complete validated recipe. No-ops return false without changing history.
    pub fn commit(&mut self, label: String, after: AuthorInputs) -> Result<bool, AuthorInputError> {
        let recipe_id = after.recipe_id()?;
        if recipe_id == self.recipe_id {
            return Ok(false);
        }
        self.history.truncate(self.cursor);
        self.history.push(InputCommand {
            _label: label,
            before: self.draft.clone(),
            after: after.clone(),
        });
        self.cursor += 1;
        self.replace_draft(after, recipe_id);
        Ok(true)
    }

    fn replace_draft(&mut self, inputs: AuthorInputs, recipe_id: RecipeId) {
        self.draft = inputs;
        self.recipe_id = recipe_id;
        self.revision = self
            .revision
            .checked_add(1)
            .expect("session revision exhausted");
        self.invalidate_build();
    }

    /// Restores the previous recorded input transaction; returns false at the history root.
    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        let inputs = self.history[self.cursor].before.clone();
        let recipe_id = inputs
            .recipe_id()
            .expect("history contains validated inputs");
        self.replace_draft(inputs, recipe_id);
        true
    }

    /// Restores the next recorded transaction, including its already chosen seed.
    pub fn redo(&mut self) -> bool {
        if self.cursor == self.history.len() {
            return false;
        }
        let inputs = self.history[self.cursor].after.clone();
        self.cursor += 1;
        let recipe_id = inputs
            .recipe_id()
            .expect("history contains validated inputs");
        self.replace_draft(inputs, recipe_id);
        true
    }

    /// Borrows the last committed validated recipe.
    pub const fn draft(&self) -> &AuthorInputs {
        &self.draft
    }
    /// Returns the monotonically increasing input revision within this epoch.
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    /// Compares the current recipe with the recipe actually saved, independent of publication.
    pub fn is_dirty(&self) -> bool {
        self.saved_recipe_id != Some(self.recipe_id)
    }
    /// Captures the current recipe identity for an asynchronous save.
    pub fn capture_save(&self) -> SaveStamp {
        SaveStamp {
            epoch: self.epoch,
            recipe_id: self.recipe_id,
        }
    }
    /// Records the written recipe; rejects completions belonging to an earlier project epoch.
    pub fn mark_saved(&mut self, stamp: SaveStamp) -> bool {
        if stamp.epoch != self.epoch {
            return false;
        }
        self.saved_recipe_id = Some(stamp.recipe_id);
        true
    }
    /// Captures a new request and revokes publication authority of every prior request.
    pub fn begin_build(&mut self) -> BuildToken {
        self.request_id = self
            .request_id
            .checked_add(1)
            .expect("build request ID exhausted");
        let token = BuildToken {
            epoch: self.epoch,
            request_id: self.request_id,
            revision: self.revision,
            recipe_id: self.recipe_id,
        };
        self.active_build = Some(token);
        token
    }
    /// Revokes the current request without changing input or save history.
    pub fn invalidate_build(&mut self) {
        self.active_build = None;
    }
    /// Checks epoch, request, input revision, and recipe against the current publication authority.
    pub fn accepts(&self, token: BuildToken) -> bool {
        self.active_build == Some(token)
            && token.epoch == self.epoch
            && token.revision == self.revision
            && token.recipe_id == self.recipe_id
    }
    /// Marks a fresh request published once; publication does not mark the recipe saved.
    pub fn mark_published(&mut self, token: BuildToken) -> bool {
        if !self.accepts(token) {
            return false;
        }
        self.published_recipe_id = Some(token.recipe_id);
        self.invalidate_build();
        true
    }
    /// Validates then replaces a project, clearing history, publication, and pending authority.
    /// `saved` is false for identity-less imports or explicitly upgraded copies.
    pub fn reset_project(
        &mut self,
        inputs: AuthorInputs,
        saved: bool,
    ) -> Result<(), AuthorInputError> {
        let recipe_id = inputs.recipe_id()?;
        self.epoch = self.epoch.checked_add(1).expect("project epoch exhausted");
        self.draft = inputs;
        self.recipe_id = recipe_id;
        self.revision = 0;
        self.saved_recipe_id = saved.then_some(recipe_id);
        self.published_recipe_id = None;
        self.invalidate_build();
        self.history.clear();
        self.cursor = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> AuthoringSession {
        AuthoringSession::new(AuthorInputs::default()).unwrap()
    }

    #[test]
    fn noop_does_not_advance_revision_or_invalidate_build() {
        let mut session = session();
        let token = session.begin_build();
        assert!(!session
            .commit("same".into(), session.draft().clone())
            .unwrap());
        assert_eq!(session.revision(), 0);
        assert!(session.accepts(token));
    }

    #[test]
    fn invalid_inputs_leave_the_draft_and_request_unchanged() {
        let mut session = session();
        let before = session.draft().clone();
        let token = session.begin_build();
        for invalid in [f32::NAN, f32::INFINITY, 0.0] {
            let mut after = before.clone();
            after.relief_spec.target_land_fraction = invalid;
            assert!(session.commit("invalid".into(), after).is_err());
        }
        let mut after = before.clone();
        after.tectonic_spec.plate_count = 0;
        assert!(session.commit("invalid".into(), after).is_err());
        assert_eq!(session.draft(), &before);
        assert_eq!(session.revision(), 0);
        assert!(session.accepts(token));
    }

    #[test]
    fn a_preset_and_its_linked_inputs_are_one_command() {
        let mut session = session();
        let before = session.draft().clone();
        let mut after = before.clone();
        super::super::apply_formation_preset_selection(
            &mut after.formation_spec,
            &mut after.tectonic_spec,
            &mut after.relief_spec,
            crate::world::natural::WorldFormationPreset::Archipelago,
        );
        session.commit("preset".into(), after.clone()).unwrap();
        assert_eq!(session.revision(), 1);
        assert!(session.undo());
        assert_eq!(session.draft(), &before);
        assert!(!session.undo());
        assert!(session.redo());
        assert_eq!(session.draft(), &after);
    }

    #[test]
    fn a_new_command_after_undo_cuts_the_redo_branch() {
        let mut session = session();
        let mut after = session.draft().clone();
        after.world_seed = 71;
        session.commit("seed".into(), after).unwrap();
        assert!(session.undo());
        let mut after = session.draft().clone();
        after.world_seed = 72;
        session.commit("other seed".into(), after).unwrap();
        assert!(!session.redo());
        assert_eq!(session.draft().world_seed, 72);
    }

    #[test]
    fn redo_restores_the_recorded_seed_and_never_revives_an_old_request() {
        let mut session = session();
        let old = session.begin_build();
        let recorded_seed = 919;
        let mut after = session.draft().clone();
        after.world_seed = recorded_seed;
        session.commit("seed".into(), after).unwrap();
        assert!(session.undo());
        assert!(!session.accepts(old));
        assert!(session.redo());
        assert_eq!(session.draft().world_seed, recorded_seed);
        assert_eq!(session.revision(), 3);
    }

    #[test]
    fn saving_a_then_editing_b_marks_the_written_recipe_and_stays_dirty() {
        let mut session = session();
        let stamp = session.capture_save();
        let mut after = session.draft().clone();
        after.world_seed += 1;
        session.commit("seed".into(), after).unwrap();
        assert!(session.mark_saved(stamp));
        assert!(session.is_dirty());
        assert!(session.undo());
        assert!(!session.is_dirty());
    }

    #[test]
    fn resetting_rejects_old_save_and_build_tokens_and_clears_history() {
        let mut session = session();
        let stamp = session.capture_save();
        let token = session.begin_build();
        let inputs = session.draft().clone();
        session.reset_project(inputs.clone(), true).unwrap();
        assert!(!session.mark_saved(stamp));
        assert!(!session.accepts(token));
        assert!(!session.undo());
        assert!(!session.redo());
        assert!(!session.is_dirty());
        session.reset_project(inputs, false).unwrap();
        assert!(session.is_dirty());
    }

    #[test]
    fn only_the_current_request_can_publish_and_publication_does_not_save() {
        let mut session = session();
        let old = session.begin_build();
        let fresh = session.begin_build();
        assert!(!session.mark_published(old));
        assert!(session.mark_published(fresh));
        assert!(!session.accepts(fresh));
        assert!(session.is_dirty());
        let cancelled = session.begin_build();
        session.invalidate_build();
        assert!(!session.accepts(cancelled));
    }

    #[test]
    fn identity_round_trips_losslessly_and_includes_inactive_inputs() {
        let inputs = AuthorInputs::default();
        let encoded = serde_json::to_string(&inputs).unwrap();
        let decoded: AuthorInputs = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, inputs);
        assert_eq!(decoded.recipe_id().unwrap(), inputs.recipe_id().unwrap());
        let mut changed = inputs.clone();
        changed.relief_spec.target_land_fraction =
            f32::from_bits(changed.relief_spec.target_land_fraction.to_bits() + 1);
        assert_ne!(changed.recipe_id().unwrap(), inputs.recipe_id().unwrap());
        changed = inputs.clone();
        changed.spherical_space_spec.target_cell_count += 1;
        assert_ne!(changed.recipe_id().unwrap(), inputs.recipe_id().unwrap());
    }
}
