use crate::view::DisplayRevisions;

/// Which immutable GPU inputs must be uploaded for a packet revision change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadPlan {
    /// Upload normalized vertices and triangle indices.
    pub mesh: bool,
    /// Upload packed scalar or category values.
    pub field: bool,
    /// Upload per-cell diagnostic severities.
    pub diagnostics: bool,
    /// Upload base and diagnostic palette colors.
    pub palette: bool,
}

impl UploadPlan {
    /// Plans uploads from the last complete revision set to the next packet.
    pub fn between(previous: Option<DisplayRevisions>, next: DisplayRevisions) -> Self {
        let Some(previous) = previous else {
            return Self::all();
        };
        if previous.mesh != next.mesh {
            return Self::all();
        }
        Self {
            mesh: false,
            field: previous.field != next.field,
            diagnostics: previous.diagnostics != next.diagnostics,
            palette: previous.palette != next.palette,
        }
    }

    /// Returns a plan with no immutable uploads.
    #[cfg(test)]
    pub const fn none() -> Self {
        Self {
            mesh: false,
            field: false,
            diagnostics: false,
            palette: false,
        }
    }

    /// Returns a plan that refreshes every immutable input.
    pub const fn all() -> Self {
        Self {
            mesh: true,
            field: true,
            diagnostics: true,
            palette: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UploadPlan;
    use crate::view::{DisplayRevision, DisplayRevisions};

    fn revisions(mesh: u64, field: u64, diagnostics: u64, palette: u64) -> DisplayRevisions {
        DisplayRevisions::new(
            DisplayRevision::new(mesh).unwrap(),
            DisplayRevision::new(field).unwrap(),
            DisplayRevision::new(diagnostics).unwrap(),
            DisplayRevision::new(palette).unwrap(),
        )
    }

    #[test]
    fn first_packet_uploads_every_buffer() {
        assert_eq!(
            UploadPlan::between(None, revisions(1, 1, 1, 1)),
            UploadPlan::all()
        );
    }

    #[test]
    fn identical_revisions_upload_nothing() {
        let revisions = revisions(4, 5, 6, 7);
        assert_eq!(
            UploadPlan::between(Some(revisions), revisions),
            UploadPlan::none()
        );
    }

    #[test]
    fn palette_revision_change_uploads_only_palette_data() {
        let current = revisions(4, 5, 6, 7);
        let next = revisions(4, 5, 6, 8);
        assert_eq!(
            UploadPlan::between(Some(current), next),
            UploadPlan {
                mesh: false,
                field: false,
                diagnostics: false,
                palette: true,
            }
        );
    }

    #[test]
    fn independent_field_and_diagnostic_revisions_upload_only_their_buffers() {
        let current = revisions(4, 5, 6, 7);
        assert_eq!(
            UploadPlan::between(Some(current), revisions(4, 8, 6, 7)),
            UploadPlan {
                mesh: false,
                field: true,
                diagnostics: false,
                palette: false,
            }
        );
        assert_eq!(
            UploadPlan::between(Some(current), revisions(4, 5, 9, 7)),
            UploadPlan {
                mesh: false,
                field: false,
                diagnostics: true,
                palette: false,
            }
        );
    }

    #[test]
    fn mesh_change_forces_all_indexed_inputs_to_upload() {
        let current = revisions(1, 2, 3, 4);
        let next = revisions(5, 2, 3, 4);
        assert_eq!(UploadPlan::between(Some(current), next), UploadPlan::all());
    }
}
