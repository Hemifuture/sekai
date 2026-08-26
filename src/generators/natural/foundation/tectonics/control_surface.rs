//! Transient low-resolution control surface and one-shot authoritative projection.
//!
//! The Cortial-style evolution runs on a bounded geodesic control mesh. Only
//! its final current state is transported onto the caller's authoritative
//! surface; the control topology is never published or retained as history.

use thiserror::Error;

use super::kinematics::{walk_nearest_cell, KinematicsError};
use super::model::{ActivePlate, TectonicModelError, TectonicState};
use super::resample::interpolate_dense_control_material;
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceBuildError};
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::{CellId, SphericalSpaceSpec};

pub(super) const TECTONIC_CONTROL_TARGET_CELL_COUNT: u32 = 5_000;

pub(super) fn requires_control_surface(authoritative_cell_count: usize) -> bool {
    authoritative_cell_count > TECTONIC_CONTROL_TARGET_CELL_COUNT as usize
}

pub(super) fn build_control_surface(
    authoritative: &SphericalSurfaceSnapshot,
) -> Result<SphericalSurfaceSnapshot, ControlSurfaceError> {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: authoritative.radius(),
        target_cell_count: TECTONIC_CONTROL_TARGET_CELL_COUNT,
    })
    .map_err(Into::into)
}

pub(super) fn project_current_state(
    control: &SphericalSurfaceSnapshot,
    control_topology: &NaturalTopologyIndex,
    authoritative: &SphericalSurfaceSnapshot,
    authoritative_topology: &NaturalTopologyIndex,
    state: TectonicState,
) -> Result<TectonicState, ControlSurfaceError> {
    if state.samples.len() != control.cells().len() {
        return Err(ControlSurfaceError::ControlCardinalityMismatch {
            samples: state.samples.len(),
            cells: control.cells().len(),
        });
    }

    let next_lineage_raw = state.next_lineage_raw();
    let mut dense_control = vec![None; control.cells().len()];
    for (sample_index, sample) in state.samples.into_iter().enumerate() {
        let cell_index = sample.anchor.raw() as usize;
        let Some(slot) = dense_control.get_mut(cell_index) else {
            return Err(ControlSurfaceError::InvalidControlAnchor {
                sample: sample_index,
                anchor: sample.anchor,
                cells: control.cells().len(),
            });
        };
        if slot.replace(sample).is_some() {
            return Err(ControlSurfaceError::DuplicateControlAnchor {
                anchor: sample.anchor,
            });
        }
    }
    let dense_control = dense_control
        .into_iter()
        .enumerate()
        .map(|(index, sample)| {
            sample.ok_or(ControlSurfaceError::MissingControlAnchor {
                anchor: CellId::from_raw(index as u32),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let control_start = CellId::from_raw(0);
    let mut samples = Vec::with_capacity(authoritative.cells().len());
    let mut local_candidates = Vec::with_capacity(32);
    for cell in authoritative.cells() {
        let control_cell =
            walk_nearest_cell(control, control_topology, control_start, cell.centroid)?;
        let mut projected = interpolate_dense_control_material(
            cell.centroid,
            control_cell,
            control_topology,
            &dense_control,
            &mut local_candidates,
        );
        projected.position = cell.centroid;
        projected.anchor = cell.id;
        samples.push(projected);
    }

    let authoritative_start = CellId::from_raw(0);
    let mut plates = Vec::with_capacity(state.plates.len());
    for plate in state.plates {
        let representative = control.cell(plate.representative).ok_or(
            ControlSurfaceError::InvalidControlRepresentative {
                representative: plate.representative,
            },
        )?;
        let authoritative_representative = walk_nearest_cell(
            authoritative,
            authoritative_topology,
            authoritative_start,
            representative.centroid,
        )?;
        plates.push(ActivePlate::new(
            plate.lineage,
            authoritative_representative,
            plate.rotation,
        ));
    }

    TectonicState::new(samples, plates, next_lineage_raw).map_err(Into::into)
}

#[derive(Debug, Error)]
pub(super) enum ControlSurfaceError {
    #[error("transient tectonic control surface construction failed: {0}")]
    Build(#[from] SphericalSurfaceBuildError),
    #[error("control state has {samples} samples for {cells} cells")]
    ControlCardinalityMismatch { samples: usize, cells: usize },
    #[error("control sample {sample} has invalid anchor {anchor:?} for {cells} cells")]
    InvalidControlAnchor {
        sample: usize,
        anchor: CellId,
        cells: usize,
    },
    #[error("more than one control sample is anchored to {anchor:?}")]
    DuplicateControlAnchor { anchor: CellId },
    #[error("the control state has no sample anchored to {anchor:?}")]
    MissingControlAnchor { anchor: CellId },
    #[error("control plate representative {representative:?} is invalid")]
    InvalidControlRepresentative { representative: CellId },
    #[error("control-to-authoritative cell location failed: {0}")]
    Location(#[from] KinematicsError),
    #[error("projected current state is invalid: {0}")]
    Model(#[from] TectonicModelError),
}

#[cfg(test)]
mod tests {
    use super::{project_current_state, requires_control_surface};
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::foundation::tectonics::initial_state::build_initial_state;
    use crate::generators::natural::foundation::tectonics::runner::canonicalize_evolved_state;
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        ResolvedWorldFormationPreset, TectonicSpec, MAX_PLATE_COUNT, MIN_PLATE_COUNT,
    };
    use crate::world::spatial::SphericalNaturalSurface;
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    fn surface(target_cell_count: u32) -> crate::world::spatial::SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count,
        })
        .unwrap()
    }

    fn topology(surface: &crate::world::spatial::SphericalSurfaceSnapshot) -> NaturalTopologyIndex {
        let view = SphericalNaturalSurface::from_validated(surface).unwrap();
        NaturalTopologyIndex::from_surface(&view)
    }

    #[test]
    fn control_surface_is_used_only_above_its_bounded_transient_budget() {
        assert!(!requires_control_surface(4_842));
        assert!(requires_control_surface(20_252));
    }

    #[test]
    fn current_control_state_projects_once_to_authoritative_unit_cells() {
        let control = surface(42);
        let authoritative = surface(162);
        let control_topology = topology(&control);
        let authoritative_topology = topology(&authoritative);
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("control-surface-test", 1, "sekai.test"),
        ));
        let streams = LabeledSubstreams::capture(&mut rng);
        let state = build_initial_state(
            &control,
            &control_topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams,
        )
        .unwrap();
        let source_thickness_bits = state
            .samples
            .iter()
            .map(|sample| sample.thickness_km.to_bits())
            .collect::<std::collections::BTreeSet<_>>();
        let source_categories = state
            .samples
            .iter()
            .map(|sample| (sample.owner, sample.kind, sample.orogeny))
            .collect::<Vec<_>>();

        let projected = project_current_state(
            &control,
            &control_topology,
            &authoritative,
            &authoritative_topology,
            state,
        )
        .unwrap();

        assert_eq!(projected.samples.len(), authoritative.cells().len());
        for (cell, sample) in authoritative.cells().iter().zip(&projected.samples) {
            assert_eq!(sample.anchor, cell.id);
            assert_eq!(sample.position, cell.centroid);
            assert!((sample.position.norm() - 1.0).abs() <= 1.0e-12);
            assert!(source_categories.contains(&(sample.owner, sample.kind, sample.orogeny)));
        }
        assert!(
            projected
                .samples
                .iter()
                .any(|sample| !source_thickness_bits.contains(&sample.thickness_km.to_bits())),
            "continuous crust attributes must use spherical interpolation instead of block copies"
        );
        let canonical = canonicalize_evolved_state(&authoritative, projected).unwrap();
        assert_eq!(canonical.samples.len(), authoritative.cells().len());
        assert!(
            (usize::from(MIN_PLATE_COUNT)..=usize::from(MAX_PLATE_COUNT))
                .contains(&canonical.plates.len())
        );
    }
}
