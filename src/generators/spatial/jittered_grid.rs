use rand::Rng;

use crate::world::{Meters, PlanarSpaceSpec, SpecError, WorldPoint};

const SLOT_INSET: f64 = 1.0e-9;
const MAX_JITTER: f64 = 0.45;

/// A deterministic y-major grid of independently jittered planar sites.
#[derive(Debug, Clone, PartialEq)]
pub struct JitteredGridSites {
    columns: usize,
    rows: usize,
    sites: Vec<WorldPoint>,
}

impl JitteredGridSites {
    /// Validates a planar space and fills every derived grid slot using the caller's RNG.
    pub fn generate<R>(space: &PlanarSpaceSpec, rng: &mut R) -> Result<Self, SpecError>
    where
        R: Rng + ?Sized,
    {
        space.validate()?;
        Ok(Self::generate_validated(space, rng))
    }

    /// Returns the number of grid columns.
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// Returns the number of grid rows.
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// Returns the generated sites in y-major then x-major order.
    pub fn sites(&self) -> &[WorldPoint] {
        &self.sites
    }

    pub(super) fn generate_validated<R>(space: &PlanarSpaceSpec, rng: &mut R) -> Self
    where
        R: Rng + ?Sized,
    {
        let width = space.width.get();
        let height = space.height.get();
        let target = space.target_cell_count as usize;
        let aspect_ratio = width / height;
        let columns = ((target as f64 * aspect_ratio).sqrt().ceil() as usize).max(1);
        let rows = target.div_ceil(columns);
        let cell_width = width / columns as f64;
        let cell_height = height / rows as f64;
        let mut sites = Vec::with_capacity(columns * rows);

        for row in 0..rows {
            for column in 0..columns {
                let jitter_x = rng.random::<f64>() * (2.0 * MAX_JITTER) - MAX_JITTER;
                let jitter_y = rng.random::<f64>() * (2.0 * MAX_JITTER) - MAX_JITTER;
                let local_x = (0.5 + jitter_x).clamp(SLOT_INSET, 1.0 - SLOT_INSET);
                let local_y = (0.5 + jitter_y).clamp(SLOT_INSET, 1.0 - SLOT_INSET);
                let x = (column as f64 + local_x) * cell_width;
                let y = (row as f64 + local_y) * cell_height;
                sites.push(WorldPoint::new(
                    Meters::new(x).expect("validated width yields finite site coordinates"),
                    Meters::new(y).expect("validated height yields finite site coordinates"),
                ));
            }
        }

        debug_assert!(sites.len() >= target);
        debug_assert!(sites.len() < target + columns);
        Self {
            columns,
            rows,
            sites,
        }
    }
}
