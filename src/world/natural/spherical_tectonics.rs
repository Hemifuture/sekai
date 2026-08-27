use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    classify_boundary_kinematics, BoundaryClassification, BoundaryKind, BoundaryKinematics,
    BoundaryRecord, CrustKind, CrustKindField, PlateIdField, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, ELEVATION_MAX_M, ELEVATION_MIN_M, MAX_PLATE_COUNT,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceEdge, SphericalSurfaceSnapshot, SphericalSurfaceValidationError,
    SurfaceGeometryKind, SurfaceRef, SurfaceRefError, UnitVector3,
};
use crate::world::{
    BoundarySegmentId, CellId, EdgeId, Meters, PlateId, SurfaceVertexId, MAX_SPHERICAL_CELL_COUNT,
    MAX_SPHERICAL_EDGE_COUNT,
};

/// The supported schema for present-day, surface-bound spherical tectonic snapshots.
pub const TECTONIC_SNAPSHOT_SCHEMA_V3: u16 = 3;
/// The canonical age stored for continental crust, whose formation age is not modeled here.
pub const CONTINENTAL_CRUST_AGE_SENTINEL_MYR: f32 = -1.0;
/// The canonical age stored when no current orogeny is present.
pub const NO_OROGENY_AGE_SENTINEL_MYR: f32 = -1.0;
/// The oldest oceanic crust or active orogeny representable by the current-state contract.
pub const MAX_CRUST_AGE_MYR: f32 = 512.0;
/// The maximum supported local rigid-plate speed, in millimeters per year.
pub const MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR: f64 = 120.0;
/// The largest representable angular rate, sized for 120 mm/year on a one-meter sphere.
pub const MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR: u64 = 120_000_000_000;
/// Cloos (1993): oceanic lithosphere becomes negatively buoyant relative to the
/// asthenosphere after about 10 Myr. G1d uses this only as the necessary
/// (not sufficient) age for intra-ocean spontaneous subduction initiation;
/// complete passive margins stay closed because of continental lithosphere
/// strength (McKenzie 1977; Stern 2004), not because of this number.
pub const CLOOS_OCEANIC_NEGATIVE_BUOYANCY_AGE_MYR: f32 = 10.0;
/// No plate may hold more than this share of the sphere. Inherited V5
/// publication bound (evolved-tectonics-v5 design): Earth's largest plate,
/// the Pacific, holds about a fifth of the surface (Bird 2003), and a
/// single-plate Pangea with its shelves stayed below half. The opening
/// supercontinent plate of dispersal-phase morphologies and terrane transfer
/// both respect it.
pub const MAXIMUM_PLATE_AREA_FRACTION: f64 = 0.45;
/// Ranking placeholder: slab-pull force per metre of trench. Conrad &
/// Lithgow-Bertelloni (2002) make slab pull the leading driving term (about
/// half of net driving force). The absolute unit is not Earth-SI; G1d task 4
/// must re-pin it from production-operator speeds. Sign is toward subduction.
pub const PLATE_SLAB_PULL_FORCE_PER_M: f64 = 0.75;
/// Ranking placeholder: slab-suction force per metre of trench acting on the
/// overriding plate, directed toward the trench. Conrad & Lithgow-Bertelloni
/// (2004, JGR 109, B10407) find slab suction comparable to direct slab pull in
/// the Cenozoic torque budget and the main driver of plates without slabs; it
/// is also what pulls a supercontinent apart toward its subduction girdle
/// (Gurnis 1988). Re-pinned by G1e task 4 from production-operator speeds.
pub const PLATE_SLAB_SUCTION_FORCE_PER_M: f64 = 0.5;
/// Ranking placeholder: ridge-push force per metre of spreading ridge. Conrad
/// & Lithgow-Bertelloni (2002) give ridge push about 5–10% of slab pull; 0.08
/// is the mid-range ratio, not a morphological fit. Sign is away from the ridge.
pub const PLATE_RIDGE_PUSH_FORCE_PER_M: f64 = 0.06;
/// Ranking placeholder: oceanic basal-drag density in force per square metre
/// per (metre/year). Forsyth & Uyeda (1975) put linear drag on the left-hand
/// side of the torque balance. Absolute scale is unpinned until G1d task 4.
pub const PLATE_OCEAN_BASAL_DRAG_PER_M2: f64 = 1.0e-6;
/// Ranking placeholder: continental basal-drag density. Forsyth & Uyeda (1975)
/// find plates with continental lithosphere significantly slower; this is
/// stronger than [`PLATE_OCEAN_BASAL_DRAG_PER_M2`] by ranking, not a fitted
/// Earth-table copy.
pub const PLATE_CONTINENT_BASAL_DRAG_PER_M2: f64 = 4.0e-6;
/// Ranking placeholder: collision dashpot per metre of continent–continent
/// convergent boundary. Spec §3.2: collision resistance opposes convergence so
/// Continents cannot suture by inertia when no trench is present. Pin after
/// production measurement (G1d task 4).
pub const PLATE_COLLISION_RESISTANCE_PER_M: f64 = 60.0;
/// Ranking placeholder: dashpot per metre of interplate convergent boundary
/// whose descending candidate is still positively buoyant (Cloos 1993, younger
/// than [`CLOOS_OCEANIC_NEGATIVE_BUOYANCY_AGE_MYR`]). Such a boundary can
/// neither consume nor thicken, so the convergence must be resisted in the
/// torque balance instead of being absorbed by resampling (G1e §3.3). Pinned
/// from the residual convergence measured by G1e task 4.
pub const PLATE_LOCKED_MARGIN_RESISTANCE_PER_M: f64 = 2000.0;

const PRAD_TO_RAD: f64 = 1.0e-12;
const METERS_TO_MILLIMETERS: f64 = 1_000.0;
const UNIT_NORM_TOLERANCE: f64 = 16.0 * f64::EPSILON;
const SPEED_TOLERANCE_MM_PER_YEAR: f64 = 1.0e-9;
const WEAK_RELATIVE_SPEED_MM_PER_YEAR: f64 = 8.0;
const MAX_RELATIVE_SPEED_MM_PER_YEAR: f32 = 240.0;
const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;
const MAX_SPHERICAL_PLATES: usize = MAX_PLATE_COUNT as usize;
const LINEATION_NORM_TOLERANCE: f32 = 1.0e-4;

/// One rigid spherical plate rotation about an Euler pole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalPlateRotation {
    pole: UnitVector3,
    angular_rate_prad_per_year: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalPlateRotationWire {
    pole: [f64; 3],
    angular_rate_prad_per_year: u64,
}

impl SphericalPlateRotation {
    /// Constructs a nonzero, fixed-point Euler rotation inside the numerical budget.
    pub fn new(
        pole: UnitVector3,
        angular_rate_prad_per_year: u64,
    ) -> Result<Self, SphericalTectonicValidationError> {
        if !(1..=MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR)
            .contains(&angular_rate_prad_per_year)
        {
            return Err(SphericalTectonicValidationError::AngularRateOutOfRange {
                found: angular_rate_prad_per_year,
                min: 1,
                max: MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
            });
        }
        Ok(Self {
            pole,
            angular_rate_prad_per_year,
        })
    }

    /// Returns the canonical Euler-pole direction.
    pub const fn pole(self) -> UnitVector3 {
        self.pole
    }

    /// Returns the stored fixed-point angular rate in picoradians per year.
    pub const fn angular_rate_prad_per_year(self) -> u64 {
        self.angular_rate_prad_per_year
    }

    /// Returns the angular-rate magnitude in radians per year.
    pub fn angular_rate_rad_per_year(self) -> f64 {
        self.angular_rate_prad_per_year as f64 * PRAD_TO_RAD
    }

    /// Returns the shared three-dimensional angular-velocity vector in radians per year.
    pub fn angular_velocity_vector_rad_per_year(self) -> [f64; 3] {
        let rate = self.angular_rate_rad_per_year();
        self.pole.components().map(|component| component * rate)
    }

    /// Returns the greatest local linear speed possible at the requested radius.
    pub fn maximum_speed_mm_per_year(
        self,
        radius: Meters,
    ) -> Result<f64, SphericalTectonicValidationError> {
        validate_radius(radius)?;
        Ok(self.angular_rate_rad_per_year() * radius.get() * METERS_TO_MILLIMETERS)
    }

    /// Validates the radius-dependent local-speed envelope.
    pub fn validate_for_radius(
        self,
        radius: Meters,
    ) -> Result<(), SphericalTectonicValidationError> {
        let speed = self.maximum_speed_mm_per_year(radius)?;
        if speed > MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR + SPEED_TOLERANCE_MM_PER_YEAR {
            return Err(SphericalTectonicValidationError::PlateSpeedOutOfRange {
                found_mm_per_year: speed,
                max_mm_per_year: MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR,
            });
        }
        Ok(())
    }

    /// Builds a rotation from an angular-velocity vector in radians per year.
    ///
    /// The pole follows \(\boldsymbol{\omega}\). Magnitude is clamped so local speed
    /// stays inside [`MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR`].
    ///
    /// # Errors
    ///
    /// Returns [`SphericalTectonicValidationError::InvalidRadius`] when `radius`
    /// is not finite and positive. Returns
    /// [`SphericalTectonicValidationError::AngularRateOutOfRange`] when `omega`
    /// is zero or non-finite.
    pub fn from_angular_velocity_rad_per_year(
        omega: [f64; 3],
        radius: Meters,
    ) -> Result<Self, SphericalTectonicValidationError> {
        validate_radius(radius)?;
        if omega.iter().any(|component| !component.is_finite()) {
            return Err(SphericalTectonicValidationError::AngularRateOutOfRange {
                found: 0,
                min: 1,
                max: MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
            });
        }
        let magnitude = (omega[0] * omega[0] + omega[1] * omega[1] + omega[2] * omega[2]).sqrt();
        if magnitude == 0.0 {
            return Err(SphericalTectonicValidationError::AngularRateOutOfRange {
                found: 0,
                min: 1,
                max: MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
            });
        }
        let pole = UnitVector3::new(
            omega[0] / magnitude,
            omega[1] / magnitude,
            omega[2] / magnitude,
        )
        .map_err(
            |_| SphericalTectonicValidationError::AngularRateOutOfRange {
                found: 0,
                min: 1,
                max: MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
            },
        )?;
        let max_rate =
            MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR / (radius.get() * METERS_TO_MILLIMETERS);
        let rate = magnitude.min(max_rate);
        let prad = (rate / PRAD_TO_RAD)
            .round()
            .clamp(1.0, MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR as f64)
            as u64;
        let rotation = Self::new(pole, prad)?;
        rotation.validate_for_radius(radius)?;
        Ok(rotation)
    }

    /// Derives the local tangent velocity from the shared Euler rotation.
    pub fn velocity_mm_per_year(
        self,
        radius: Meters,
        radial: UnitVector3,
    ) -> Result<[f64; 3], SphericalTectonicValidationError> {
        self.validate_for_radius(radius)?;
        let [px, py, pz] = self.pole.components();
        let [rx, ry, rz] = radial.components();
        let scale = self.angular_rate_rad_per_year() * radius.get() * METERS_TO_MILLIMETERS;
        Ok([
            (py * rz - pz * ry) * scale,
            (pz * rx - px * rz) * scale,
            (px * ry - py * rx) * scale,
        ])
    }
}

impl<'de> Deserialize<'de> for SphericalPlateRotation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalPlateRotationWire::deserialize(deserializer)?;
        if wire.pole.iter().any(|component| !component.is_finite()) {
            return Err(D::Error::custom("Euler-pole components must be finite"));
        }
        let squared_norm = wire
            .pole
            .iter()
            .map(|component| component * component)
            .sum::<f64>();
        let norm = squared_norm.sqrt();
        if (norm - 1.0).abs() > UNIT_NORM_TOLERANCE {
            return Err(D::Error::custom(format_args!(
                "Euler pole must be a unit vector, got norm {norm}"
            )));
        }
        let pole = UnitVector3::from_verified_unit_components(wire.pole);
        Self::new(pole, wire.angular_rate_prad_per_year).map_err(D::Error::custom)
    }
}

/// Derives one edge event from the exact spherical geometry and semantic source fields.
pub(crate) fn classify_spherical_boundary_kinematics(
    plates: [PlateId; 2],
    rotations: [SphericalPlateRotation; 2],
    radius: Meters,
    edge: &SphericalSurfaceEdge,
    crust: [CrustKind; 2],
    thickness_km: [f32; 2],
) -> Result<BoundaryClassification, SphericalTectonicValidationError> {
    let first = rotations[0].velocity_mm_per_year(radius, edge.midpoint)?;
    let second = rotations[1].velocity_mm_per_year(radius, edge.midpoint)?;
    let relative = [
        second[0] - first[0],
        second[1] - first[1],
        second[2] - first[2],
    ];
    let speed =
        (relative[0] * relative[0] + relative[1] * relative[1] + relative[2] * relative[2]).sqrt();
    let normal = edge.normal_from_first.components();
    let signed_normal_speed =
        relative[0] * normal[0] + relative[1] * normal[1] + relative[2] * normal[2];
    let normal_speed = signed_normal_speed.abs();
    let tangent_speed = (speed * speed - normal_speed * normal_speed)
        .max(0.0)
        .sqrt();
    Ok(classify_boundary_kinematics(
        plates,
        crust,
        thickness_km,
        BoundaryKinematics {
            speed: speed as f32,
            normal_speed: normal_speed as f32,
            tangent_speed: tangent_speed as f32,
            maximum_relative_speed: MAX_RELATIVE_SPEED_MM_PER_YEAR,
            weak: speed < WEAK_RELATIVE_SPEED_MM_PER_YEAR,
            strong_normal_component: normal_speed >= speed * 0.4,
            converging: signed_normal_speed < 0.0,
        },
    ))
}

/// A rigid plate on the current spherical world slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalPlate {
    id: PlateId,
    seed_cell: CellId,
    rotation: SphericalPlateRotation,
}

impl SphericalPlate {
    /// Creates one plate record; snapshot validation checks its dense references.
    pub const fn new(id: PlateId, seed_cell: CellId, rotation: SphericalPlateRotation) -> Self {
        Self {
            id,
            seed_cell,
            rotation,
        }
    }

    /// Returns the contiguous stable plate identifier.
    pub const fn id(&self) -> PlateId {
        self.id
    }

    /// Returns one cell guaranteed to be owned by this plate.
    pub const fn seed_cell(&self) -> CellId {
        self.seed_cell
    }

    /// Returns the plate's single authoritative Euler rotation.
    pub const fn rotation(&self) -> SphericalPlateRotation {
        self.rotation
    }
}

/// A connected same-kind portion of a spherical plate boundary.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalBoundarySegment {
    id: BoundarySegmentId,
    plates: [PlateId; 2],
    kind: BoundaryKind,
    member_edges: Vec<EdgeId>,
    mean_strength: f32,
    subducting_plate: Option<PlateId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalBoundarySegmentWire {
    id: BoundarySegmentId,
    plates: [PlateId; 2],
    kind: BoundaryKind,
    #[serde(deserialize_with = "deserialize_boundary_member_edges")]
    member_edges: Vec<EdgeId>,
    mean_strength: f32,
    subducting_plate: Option<PlateId>,
}

impl SphericalBoundarySegment {
    /// Creates a segment whose full partition is checked by its snapshot.
    pub fn new(
        id: BoundarySegmentId,
        plates: [PlateId; 2],
        kind: BoundaryKind,
        member_edges: Vec<EdgeId>,
        mean_strength: f32,
        subducting_plate: Option<PlateId>,
    ) -> Self {
        Self {
            id,
            plates,
            kind,
            member_edges,
            mean_strength,
            subducting_plate,
        }
    }

    /// Returns the contiguous stable segment identifier.
    pub const fn id(&self) -> BoundarySegmentId {
        self.id
    }

    /// Returns the involved plates in ascending identifier order.
    pub const fn plates(&self) -> [PlateId; 2] {
        self.plates
    }

    /// Returns the common edge-event classification.
    pub const fn kind(&self) -> BoundaryKind {
        self.kind
    }

    /// Returns sorted canonical member-edge identifiers.
    pub fn member_edges(&self) -> &[EdgeId] {
        &self.member_edges
    }

    /// Returns the arithmetic mean of member-edge strengths.
    pub const fn mean_strength(&self) -> f32 {
        self.mean_strength
    }

    /// Returns the descending plate for a subduction segment.
    pub const fn subducting_plate(&self) -> Option<PlateId> {
        self.subducting_plate
    }
}

impl<'de> Deserialize<'de> for SphericalBoundarySegment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalBoundarySegmentWire::deserialize(deserializer)?;
        Ok(Self::new(
            wire.id,
            wire.plates,
            wire.kind,
            wire.member_edges,
            wire.mean_strength,
            wire.subducting_plate,
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictBoundaryRecordWire {
    kind: BoundaryKind,
    strength: f32,
    segment_id: Option<BoundarySegmentId>,
    subducting_plate: Option<PlateId>,
}

impl From<StrictBoundaryRecordWire> for BoundaryRecord {
    fn from(wire: StrictBoundaryRecordWire) -> Self {
        Self::new(
            wire.kind,
            wire.strength,
            wire.segment_id,
            wire.subducting_plate,
        )
    }
}

/// The active mountain-building regime recorded at one current crust sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SphericalOrogenyKind {
    /// No active or inherited orogenic signal is stored.
    None,
    /// Oceanic subduction beneath overriding crust produced an Andean-style signal.
    Andean,
    /// Continental collision produced a Himalayan-style signal.
    Himalayan,
}

/// Dense material and tectonic attributes for the one published current crust state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalCrustState {
    kinds: CrustKindField,
    thickness_km: Vec<f32>,
    age_myr: Vec<f32>,
    tectonic_elevation_m: Vec<f32>,
    lineation_east: Vec<f32>,
    lineation_north: Vec<f32>,
    orogeny_kind: Vec<SphericalOrogenyKind>,
    orogeny_age_myr: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalCrustStateWire {
    #[serde(deserialize_with = "deserialize_spherical_cell_u32_values")]
    kinds: Vec<u32>,
    #[serde(deserialize_with = "deserialize_spherical_cell_f32_values")]
    thickness_km: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_cell_f32_values")]
    age_myr: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_cell_f32_values")]
    tectonic_elevation_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_cell_f32_values")]
    lineation_east: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_cell_f32_values")]
    lineation_north: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_cell_orogeny_values")]
    orogeny_kind: Vec<SphericalOrogenyKind>,
    #[serde(deserialize_with = "deserialize_spherical_cell_f32_values")]
    orogeny_age_myr: Vec<f32>,
}

impl SphericalCrustState {
    /// Constructs and validates one complete present-day crust field set.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kinds: CrustKindField,
        thickness_km: Vec<f32>,
        age_myr: Vec<f32>,
        tectonic_elevation_m: Vec<f32>,
        lineation_east: Vec<f32>,
        lineation_north: Vec<f32>,
        orogeny_kind: Vec<SphericalOrogenyKind>,
        orogeny_age_myr: Vec<f32>,
    ) -> Result<Self, SphericalTectonicValidationError> {
        let state = Self {
            kinds,
            thickness_km,
            age_myr,
            tectonic_elevation_m,
            lineation_east,
            lineation_north,
            orogeny_kind,
            orogeny_age_myr,
        };
        state.validate()?;
        Ok(state)
    }

    /// Rechecks dense lengths and all material-state invariants.
    pub fn validate(&self) -> Result<(), SphericalTectonicValidationError> {
        let cell_count = self.kinds.len();
        validate_allocation_limit("crust.kinds", cell_count, MAX_SPHERICAL_CELLS)?;
        for (field, found) in [
            ("thickness_km", self.thickness_km.len()),
            ("age_myr", self.age_myr.len()),
            ("tectonic_elevation_m", self.tectonic_elevation_m.len()),
            ("lineation_east", self.lineation_east.len()),
            ("lineation_north", self.lineation_north.len()),
            ("orogeny_kind", self.orogeny_kind.len()),
            ("orogeny_age_myr", self.orogeny_age_myr.len()),
        ] {
            validate_length(field, found, cell_count)?;
        }

        for index in 0..cell_count {
            let cell = CellId::from_raw(index as u32);
            let raw_kind = self.kinds.raw_values()[index];
            let kind = CrustKind::try_from_raw(raw_kind).map_err(|_| {
                SphericalTectonicValidationError::InvalidCrustKind {
                    cell,
                    found: raw_kind,
                }
            })?;
            let thickness = self.thickness_km[index];
            let (min, max) = crust_thickness_range(kind);
            if !thickness.is_finite() || !(min..=max).contains(&thickness) {
                return Err(SphericalTectonicValidationError::CrustThicknessOutOfRange {
                    cell,
                    kind,
                    found: thickness,
                    min,
                    max,
                });
            }

            let age = self.age_myr[index];
            let valid_age = match kind {
                CrustKind::Oceanic => age.is_finite() && (0.0..=MAX_CRUST_AGE_MYR).contains(&age),
                CrustKind::Continental => age == CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
            };
            if !valid_age {
                return Err(SphericalTectonicValidationError::CrustAgeOutOfRange {
                    cell,
                    kind,
                    found: age,
                });
            }

            let elevation = self.tectonic_elevation_m[index];
            if !elevation.is_finite() || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&elevation) {
                return Err(
                    SphericalTectonicValidationError::TectonicElevationOutOfRange {
                        cell,
                        found: elevation,
                        min: ELEVATION_MIN_M,
                        max: ELEVATION_MAX_M,
                    },
                );
            }

            let east = self.lineation_east[index];
            let north = self.lineation_north[index];
            let is_zero = east == 0.0 && north == 0.0;
            let norm = east.hypot(north);
            if !is_zero
                && (!east.is_finite()
                    || !north.is_finite()
                    || (norm - 1.0).abs() > LINEATION_NORM_TOLERANCE)
            {
                return Err(SphericalTectonicValidationError::InvalidCrustLineation {
                    cell,
                    east,
                    north,
                });
            }

            let orogeny = self.orogeny_kind[index];
            let orogeny_age = self.orogeny_age_myr[index];
            let valid_orogeny_age = match orogeny {
                SphericalOrogenyKind::None => orogeny_age == NO_OROGENY_AGE_SENTINEL_MYR,
                SphericalOrogenyKind::Andean | SphericalOrogenyKind::Himalayan => {
                    orogeny_age.is_finite() && (0.0..=MAX_CRUST_AGE_MYR).contains(&orogeny_age)
                }
            };
            if !valid_orogeny_age {
                return Err(SphericalTectonicValidationError::OrogenyAgeOutOfRange {
                    cell,
                    kind: orogeny,
                    found: orogeny_age,
                });
            }
        }
        Ok(())
    }

    /// Returns the number of current crust samples.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// Returns whether the current crust contains no samples.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// Returns dense crust categories without copying.
    pub const fn kinds(&self) -> &CrustKindField {
        &self.kinds
    }

    /// Returns dense current crust thickness in kilometers.
    pub fn thickness_km(&self) -> &[f32] {
        &self.thickness_km
    }

    /// Returns dense current material age in millions of years.
    pub fn age_myr(&self) -> &[f32] {
        &self.age_myr
    }

    /// Returns dense tectonic elevation in meters.
    pub fn tectonic_elevation_m(&self) -> &[f32] {
        &self.tectonic_elevation_m
    }

    /// Returns the east component of the dense unit-or-zero lineation field.
    pub fn lineation_east(&self) -> &[f32] {
        &self.lineation_east
    }

    /// Returns the north component of the dense unit-or-zero lineation field.
    pub fn lineation_north(&self) -> &[f32] {
        &self.lineation_north
    }

    /// Returns the dense current orogeny category field.
    pub fn orogeny_kind(&self) -> &[SphericalOrogenyKind] {
        &self.orogeny_kind
    }

    /// Returns dense current orogeny age in millions of years.
    pub fn orogeny_age_myr(&self) -> &[f32] {
        &self.orogeny_age_myr
    }
}

impl<'de> Deserialize<'de> for SphericalCrustState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalCrustStateWire::deserialize(deserializer)?;
        let kinds = CrustKindField::from_raw(wire.kinds).map_err(D::Error::custom)?;
        Self::new(
            kinds,
            wire.thickness_km,
            wire.age_myr,
            wire.tectonic_elevation_m,
            wire.lineation_east,
            wire.lineation_north,
            wire.orogeny_kind,
            wire.orogeny_age_myr,
        )
        .map_err(D::Error::custom)
    }
}

/// Immutable surface-bound spherical plates, crust, and current boundary events.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalTectonicSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    plates: Vec<SphericalPlate>,
    cell_plates: PlateIdField,
    crust: SphericalCrustState,
    boundaries: Vec<BoundaryRecord>,
    boundary_segments: Vec<SphericalBoundarySegment>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalTectonicSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    #[serde(deserialize_with = "deserialize_spherical_plates")]
    plates: Vec<SphericalPlate>,
    #[serde(deserialize_with = "deserialize_spherical_cell_u32_values")]
    cell_plates: Vec<u32>,
    crust: SphericalCrustState,
    #[serde(deserialize_with = "deserialize_spherical_boundaries")]
    boundaries: Vec<StrictBoundaryRecordWire>,
    #[serde(deserialize_with = "deserialize_boundary_segments")]
    boundary_segments: Vec<SphericalBoundarySegment>,
}

fn deserialize_spherical_plates<'de, D>(deserializer: D) -> Result<Vec<SphericalPlate>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_PLATES>(deserializer)
}

fn deserialize_spherical_cell_u32_values<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_cell_f32_values<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_cell_orogeny_values<'de, D>(
    deserializer: D,
) -> Result<Vec<SphericalOrogenyKind>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_boundaries<'de, D>(
    deserializer: D,
) -> Result<Vec<StrictBoundaryRecordWire>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_spherical_boundaries_with_limit::<_, MAX_SPHERICAL_EDGES>(deserializer)
}

fn deserialize_spherical_boundaries_with_limit<'de, D, const MAX: usize>(
    deserializer: D,
) -> Result<Vec<StrictBoundaryRecordWire>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX>(deserializer)
}

fn deserialize_boundary_member_edges<'de, D>(deserializer: D) -> Result<Vec<EdgeId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_EDGES>(deserializer)
}

fn deserialize_boundary_segments<'de, D>(
    deserializer: D,
) -> Result<Vec<SphericalBoundarySegment>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_boundary_segments_with_limit::<_, MAX_SPHERICAL_EDGES>(deserializer)
}

fn deserialize_boundary_segments_with_limit<'de, D, const MAX: usize>(
    deserializer: D,
) -> Result<Vec<SphericalBoundarySegment>, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoundarySegmentsVisitor<const MAX: usize>;

    impl<'de, const MAX: usize> Visitor<'de> for BoundarySegmentsVisitor<MAX> {
        type Value = Vec<SphericalBoundarySegment>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "at most {MAX} boundary segments containing at most {MAX} total member edges"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if let Some(length) = sequence.size_hint() {
                if length > MAX {
                    return Err(A::Error::invalid_length(length, &self));
                }
            }
            let mut segments = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX));
            let mut member_count = 0_usize;
            while segments.len() < MAX {
                let Some(segment) = sequence.next_element::<SphericalBoundarySegment>()? else {
                    return Ok(segments);
                };
                member_count = member_count
                    .checked_add(segment.member_edges.len())
                    .ok_or_else(|| A::Error::custom("boundary member count overflow"))?;
                if member_count > MAX {
                    return Err(A::Error::custom(format_args!(
                        "boundary segments contain {member_count} member edges; at most \
                         {MAX} are allowed"
                    )));
                }
                segments.push(segment);
            }
            if sequence.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(MAX + 1, &self));
            }
            Ok(segments)
        }
    }

    deserializer.deserialize_seq(BoundarySegmentsVisitor::<MAX>)
}

impl SphericalTectonicSnapshot {
    /// Canonicalizes stable-ID tables and validates every self-contained invariant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        mut plates: Vec<SphericalPlate>,
        cell_plates: PlateIdField,
        crust: SphericalCrustState,
        boundaries: Vec<BoundaryRecord>,
        mut boundary_segments: Vec<SphericalBoundarySegment>,
    ) -> Result<Self, SphericalTectonicValidationError> {
        plates.sort_by_key(SphericalPlate::id);
        boundary_segments.sort_by_key(SphericalBoundarySegment::id);
        let snapshot = Self {
            schema_version,
            surface_ref,
            plates,
            cell_plates,
            crust,
            boundaries,
            boundary_segments,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks all invariants that do not require the referenced surface records.
    pub fn validate(&self) -> Result<(), SphericalTectonicValidationError> {
        if self.schema_version != TECTONIC_SNAPSHOT_SCHEMA_V3 {
            return Err(SphericalTectonicValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: TECTONIC_SNAPSHOT_SCHEMA_V3,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(SphericalTectonicValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        validate_allocation_limit(
            "surface_ref.cell_count",
            self.surface_ref.cell_count() as usize,
            MAX_SPHERICAL_CELLS,
        )?;
        validate_allocation_limit(
            "surface_ref.edge_count",
            self.surface_ref.edge_count() as usize,
            MAX_SPHERICAL_EDGES,
        )?;
        validate_allocation_limit("plates", self.plates.len(), MAX_SPHERICAL_PLATES)?;

        let cell_count = self.surface_ref.cell_count() as usize;
        let edge_count = self.surface_ref.edge_count() as usize;
        if self.plates.is_empty() || self.plates.len() > cell_count {
            return Err(SphericalTectonicValidationError::InvalidPlateCount {
                found: self.plates.len(),
                cell_count,
            });
        }
        for (expected, plate) in self.plates.iter().enumerate() {
            if plate.id.raw() as usize != expected {
                return Err(SphericalTectonicValidationError::NonContiguousPlateId {
                    expected: PlateId::from_raw(expected as u32),
                    found: plate.id,
                });
            }
            if plate.seed_cell.raw() as usize >= cell_count {
                return Err(SphericalTectonicValidationError::InvalidPlateSeed {
                    plate: plate.id,
                    seed: plate.seed_cell,
                    cell_count,
                });
            }
            SphericalPlateRotation::new(
                plate.rotation.pole(),
                plate.rotation.angular_rate_prad_per_year(),
            )?;
        }

        validate_length("cell_plates", self.cell_plates.len(), cell_count)?;
        validate_length("crust", self.crust.len(), cell_count)?;
        self.crust.validate()?;
        validate_length("boundaries", self.boundaries.len(), edge_count)?;
        for index in 0..cell_count {
            let cell = CellId::from_raw(index as u32);
            let plate = self
                .cell_plates
                .get(index)
                .expect("dense plate field length was validated");
            if plate.raw() as usize >= self.plates.len() {
                return Err(SphericalTectonicValidationError::InvalidCellPlate { cell, plate });
            }
        }
        self.validate_segments_and_boundaries()
    }

    /// Rechecks identity, speed, ownership, connectivity, and edge incidence.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SphericalTectonicValidationError> {
        surface.validate()?;
        self.validate_against_validated_surface(surface)
    }

    pub(crate) fn validate_against_validated_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SphericalTectonicValidationError> {
        self.validate()?;
        let authoritative = SurfaceRef::from_validated_spherical(surface)?;
        if self.surface_ref != authoritative {
            return Err(SphericalTectonicValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        for plate in &self.plates {
            plate.rotation.validate_for_radius(surface.radius())?;
            if self.plate_for_cell(plate.seed_cell) != Some(plate.id) {
                return Err(SphericalTectonicValidationError::PlateSeedOwnership {
                    plate: plate.id,
                    seed: plate.seed_cell,
                    owner: self.plate_for_cell(plate.seed_cell),
                });
            }
        }
        self.validate_plate_connectivity(surface)?;
        for edge in surface.edges() {
            self.validate_edge_topology(edge, surface.radius())?;
        }
        for segment in &self.boundary_segments {
            self.validate_segment_connectivity(segment, surface)?;
        }
        Ok(())
    }

    /// Returns the V3 current-state schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact authoritative surface identity.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns plates in stable identifier order.
    pub fn plates(&self) -> &[SphericalPlate] {
        &self.plates
    }

    /// Returns the dense plate-identifier field.
    pub const fn cell_plates(&self) -> &PlateIdField {
        &self.cell_plates
    }

    /// Returns the dense crust-category field.
    pub const fn crust_kinds(&self) -> &CrustKindField {
        self.crust.kinds()
    }

    /// Returns crust thickness values in kilometers.
    pub fn crust_thickness_km(&self) -> &[f32] {
        self.crust.thickness_km()
    }

    /// Returns the complete dense current crust state.
    pub const fn crust_state(&self) -> &SphericalCrustState {
        &self.crust
    }

    /// Returns current material ages in millions of years.
    pub fn crust_age_myr(&self) -> &[f32] {
        self.crust.age_myr()
    }

    /// Returns current tectonic elevation in meters.
    pub fn tectonic_elevation_m(&self) -> &[f32] {
        self.crust.tectonic_elevation_m()
    }

    /// Returns east components of current crust lineation.
    pub fn lineation_east(&self) -> &[f32] {
        self.crust.lineation_east()
    }

    /// Returns north components of current crust lineation.
    pub fn lineation_north(&self) -> &[f32] {
        self.crust.lineation_north()
    }

    /// Returns current orogeny categories.
    pub fn orogeny_kind(&self) -> &[SphericalOrogenyKind] {
        self.crust.orogeny_kind()
    }

    /// Returns current orogeny ages in millions of years.
    pub fn orogeny_age_myr(&self) -> &[f32] {
        self.crust.orogeny_age_myr()
    }

    /// Returns edge-aligned current boundary events.
    pub fn boundaries(&self) -> &[BoundaryRecord] {
        &self.boundaries
    }

    /// Returns canonical-vertex-connected boundary segments.
    pub fn boundary_segments(&self) -> &[SphericalBoundarySegment] {
        &self.boundary_segments
    }

    /// Returns the plate owning one surface cell.
    pub fn plate_for_cell(&self, cell: CellId) -> Option<PlateId> {
        self.cell_plates.get(cell.raw() as usize)
    }

    /// Returns the crust kind at one surface cell.
    pub fn crust_kind(&self, cell: CellId) -> Option<CrustKind> {
        self.crust.kinds().get(cell.raw() as usize)
    }

    /// Returns crust thickness at one surface cell in kilometers.
    pub fn crust_thickness_for_cell(&self, cell: CellId) -> Option<f32> {
        self.crust.thickness_km().get(cell.raw() as usize).copied()
    }

    fn validate_segments_and_boundaries(&self) -> Result<(), SphericalTectonicValidationError> {
        let edge_count = self.surface_ref.edge_count() as usize;
        let mut membership = vec![None; edge_count];
        for (expected, segment) in self.boundary_segments.iter().enumerate() {
            let expected_id = BoundarySegmentId::from_raw(expected as u32);
            if segment.id != expected_id {
                return Err(
                    SphericalTectonicValidationError::NonContiguousBoundarySegmentId {
                        expected: expected_id,
                        found: segment.id,
                    },
                );
            }
            validate_segment_header(segment, self.plates.len(), edge_count)?;
            let mut strength_sum = 0.0_f64;
            for &edge in &segment.member_edges {
                let index = edge.raw() as usize;
                if membership[index].replace(segment.id).is_some() {
                    return Err(SphericalTectonicValidationError::DuplicateBoundaryEdge { edge });
                }
                let record = self.boundaries[index];
                if record.kind != segment.kind
                    || record.segment_id != Some(segment.id)
                    || record.subducting_plate != segment.subducting_plate
                {
                    return Err(SphericalTectonicValidationError::BoundarySegmentMismatch {
                        segment: segment.id,
                        edge,
                    });
                }
                strength_sum += f64::from(record.strength);
            }
            let mean = (strength_sum / segment.member_edges.len() as f64) as f32;
            if (mean - segment.mean_strength).abs() > 1.0e-5 {
                return Err(SphericalTectonicValidationError::BoundaryMeanMismatch {
                    segment: segment.id,
                    stored: segment.mean_strength,
                    calculated: mean,
                });
            }
        }

        for (index, record) in self.boundaries.iter().copied().enumerate() {
            let edge = EdgeId::from_raw(index as u32);
            validate_boundary_record(edge, record, self.plates.len())?;
            if membership[index] != record.segment_id {
                return Err(SphericalTectonicValidationError::BoundaryMembershipMismatch { edge });
            }
        }
        Ok(())
    }

    fn validate_plate_connectivity(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SphericalTectonicValidationError> {
        let mut owner_counts = vec![0_usize; self.plates.len()];
        for &owner in self.cell_plates.raw_values() {
            owner_counts[owner as usize] += 1;
        }
        let mut visited = vec![false; self.surface_ref.cell_count() as usize];
        let mut queue = VecDeque::new();
        for plate in &self.plates {
            let seed = plate.seed_cell;
            visited[seed.raw() as usize] = true;
            queue.push_back(seed);
            let mut reached = 0_usize;
            while let Some(cell) = queue.pop_front() {
                reached += 1;
                for &edge in surface
                    .cell_edges(cell)
                    .expect("surface identity guarantees the cell exists")
                {
                    let neighbor = surface
                        .opposite_cell(cell, edge)
                        .expect("closed spherical edges have two owners");
                    let index = neighbor.raw() as usize;
                    if !visited[index] && self.plate_for_cell(neighbor) == Some(plate.id) {
                        visited[index] = true;
                        queue.push_back(neighbor);
                    }
                }
            }
            let expected = owner_counts[plate.id.raw() as usize];
            if reached != expected {
                return Err(SphericalTectonicValidationError::DisconnectedPlate {
                    plate: plate.id,
                    reached,
                    expected,
                });
            }
        }
        Ok(())
    }

    fn validate_edge_topology(
        &self,
        edge: &SphericalSurfaceEdge,
        radius: Meters,
    ) -> Result<(), SphericalTectonicValidationError> {
        let owner_plates = edge.cells.map(|cell| {
            self.plate_for_cell(cell)
                .expect("surface identity guarantees the cell field exists")
        });
        let record = self.boundaries[edge.id.raw() as usize];
        if owner_plates[0] == owner_plates[1] {
            if record != BoundaryRecord::none() {
                return Err(SphericalTectonicValidationError::BoundaryTopologyMismatch {
                    edge: edge.id,
                });
            }
            return Ok(());
        }
        let Some(segment_id) = record.segment_id else {
            return Err(SphericalTectonicValidationError::BoundaryTopologyMismatch {
                edge: edge.id,
            });
        };
        if record.kind == BoundaryKind::None {
            return Err(SphericalTectonicValidationError::BoundaryTopologyMismatch {
                edge: edge.id,
            });
        }
        let Some(segment) = self.boundary_segments.get(segment_id.raw() as usize) else {
            return Err(SphericalTectonicValidationError::BoundaryTopologyMismatch {
                edge: edge.id,
            });
        };
        if segment.plates != normalized_plate_pair(owner_plates[0], owner_plates[1]) {
            return Err(SphericalTectonicValidationError::BoundaryTopologyMismatch {
                edge: edge.id,
            });
        }
        if self.plates[owner_plates[0].raw() as usize].rotation
            == self.plates[owner_plates[1].raw() as usize].rotation
        {
            return Err(SphericalTectonicValidationError::AdjacentPlatesCoMoving {
                plates: normalized_plate_pair(owner_plates[0], owner_plates[1]),
            });
        }
        let cell_indices = edge.cells.map(|cell| cell.raw() as usize);
        let expected = classify_spherical_boundary_kinematics(
            owner_plates,
            owner_plates.map(|plate| self.plates[plate.raw() as usize].rotation),
            radius,
            edge,
            cell_indices.map(|index| {
                self.crust
                    .kinds()
                    .get(index)
                    .expect("validated crust kind field is cell aligned")
            }),
            cell_indices.map(|index| self.crust.thickness_km()[index]),
        )?;
        if record.kind != expected.kind
            || record.subducting_plate != expected.subducting_plate
            || (record.strength - expected.strength).abs() > 1.0e-5
        {
            return Err(
                SphericalTectonicValidationError::BoundaryKinematicsMismatch { edge: edge.id },
            );
        }
        Ok(())
    }

    fn validate_segment_connectivity(
        &self,
        segment: &SphericalBoundarySegment,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SphericalTectonicValidationError> {
        let members: BTreeSet<_> = segment.member_edges.iter().copied().collect();
        let mut by_vertex = BTreeMap::<SurfaceVertexId, Vec<EdgeId>>::new();
        for &edge_id in &segment.member_edges {
            let edge = surface
                .edge(edge_id)
                .expect("validated segment edge lies inside the referenced surface");
            for vertex in edge.vertices {
                by_vertex.entry(vertex).or_default().push(edge_id);
            }
        }
        let first = segment.member_edges[0];
        let mut reached = BTreeSet::from([first]);
        let mut queue = VecDeque::from([first]);
        while let Some(edge_id) = queue.pop_front() {
            let edge = surface
                .edge(edge_id)
                .expect("validated segment edge lies inside the referenced surface");
            for vertex in edge.vertices {
                for &neighbor in &by_vertex[&vertex] {
                    if members.contains(&neighbor) && reached.insert(neighbor) {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        if reached.len() != members.len() {
            return Err(
                SphericalTectonicValidationError::DisconnectedBoundarySegment {
                    segment: segment.id,
                    reached: reached.len(),
                    expected: members.len(),
                },
            );
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for SphericalTectonicSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalTectonicSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.plates,
            PlateIdField::from_raw(wire.cell_plates),
            wire.crust,
            wire.boundaries.into_iter().map(Into::into).collect(),
            wire.boundary_segments,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_allocation_limit(
    field: &'static str,
    found: usize,
    max: usize,
) -> Result<(), SphericalTectonicValidationError> {
    if found > max {
        return Err(SphericalTectonicValidationError::AllocationExceedsLimit { field, found, max });
    }
    Ok(())
}

fn validate_length(
    field: &'static str,
    found: usize,
    expected: usize,
) -> Result<(), SphericalTectonicValidationError> {
    if found != expected {
        return Err(SphericalTectonicValidationError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

fn crust_thickness_range(kind: CrustKind) -> (f32, f32) {
    match kind {
        CrustKind::Oceanic => (
            OCEANIC_CRUST_MIN_THICKNESS_KM,
            OCEANIC_CRUST_MAX_THICKNESS_KM,
        ),
        CrustKind::Continental => (
            CONTINENTAL_CRUST_MIN_THICKNESS_KM,
            CONTINENTAL_CRUST_MAX_THICKNESS_KM,
        ),
    }
}

fn validate_boundary_record(
    edge: EdgeId,
    record: BoundaryRecord,
    plate_count: usize,
) -> Result<(), SphericalTectonicValidationError> {
    if !record.strength.is_finite() || !(0.0..=1.0).contains(&record.strength) {
        return Err(
            SphericalTectonicValidationError::BoundaryStrengthOutOfRange {
                edge,
                found: record.strength,
            },
        );
    }
    match record.kind {
        BoundaryKind::None
            if record.strength == 0.0
                && record.segment_id.is_none()
                && record.subducting_plate.is_none() => {}
        BoundaryKind::None => {
            return Err(SphericalTectonicValidationError::InvalidBoundaryRecord { edge });
        }
        BoundaryKind::Subduction
            if record.segment_id.is_some()
                && record
                    .subducting_plate
                    .is_some_and(|plate| (plate.raw() as usize) < plate_count) => {}
        BoundaryKind::Subduction => {
            return Err(SphericalTectonicValidationError::InvalidBoundaryRecord { edge });
        }
        _ if record.segment_id.is_some() && record.subducting_plate.is_none() => {}
        _ => return Err(SphericalTectonicValidationError::InvalidBoundaryRecord { edge }),
    }
    Ok(())
}

fn validate_segment_header(
    segment: &SphericalBoundarySegment,
    plate_count: usize,
    edge_count: usize,
) -> Result<(), SphericalTectonicValidationError> {
    if segment.plates[0] >= segment.plates[1]
        || segment.plates[1].raw() as usize >= plate_count
        || segment.kind == BoundaryKind::None
        || segment.member_edges.is_empty()
        || !segment.mean_strength.is_finite()
        || !(0.0..=1.0).contains(&segment.mean_strength)
    {
        return Err(SphericalTectonicValidationError::InvalidBoundarySegment {
            segment: segment.id,
        });
    }
    let mut previous = None;
    for &edge in &segment.member_edges {
        if edge.raw() as usize >= edge_count || previous.is_some_and(|value| value >= edge) {
            return Err(SphericalTectonicValidationError::InvalidBoundarySegment {
                segment: segment.id,
            });
        }
        previous = Some(edge);
    }
    match segment.kind {
        BoundaryKind::Subduction
            if segment
                .subducting_plate
                .is_some_and(|plate| segment.plates.contains(&plate)) => {}
        BoundaryKind::Subduction => {
            return Err(SphericalTectonicValidationError::InvalidBoundarySegment {
                segment: segment.id,
            });
        }
        _ if segment.subducting_plate.is_none() => {}
        _ => {
            return Err(SphericalTectonicValidationError::InvalidBoundarySegment {
                segment: segment.id,
            });
        }
    }
    Ok(())
}

fn normalized_plate_pair(first: PlateId, second: PlateId) -> [PlateId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}

fn validate_radius(radius: Meters) -> Result<(), SphericalTectonicValidationError> {
    let radius = radius.get();
    if !radius.is_finite() || radius <= 0.0 {
        return Err(SphericalTectonicValidationError::InvalidRadius { found_m: radius });
    }
    Ok(())
}

/// Failures in surface-bound spherical tectonic contracts.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalTectonicValidationError {
    /// The snapshot schema is not the supported present-day surface-bound contract.
    #[error("unsupported spherical tectonic schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The stored surface identity is internally invalid.
    #[error("invalid spherical tectonic surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The stored identity addresses a non-spherical geometry family.
    #[error("spherical tectonics cannot reference geometry kind {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    /// A surface identity or semantic table exceeds the spherical allocation budget.
    #[error("{field} allocation {found} exceeds spherical limit {max}")]
    AllocationExceedsLimit {
        field: &'static str,
        found: usize,
        max: usize,
    },
    /// The authoritative spherical surface itself failed validation.
    #[error("invalid authoritative spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The stored and authoritative surface identities differ.
    #[error("tectonic surface identity {snapshot:?} does not match {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// The fixed-point Euler angular rate is zero or exceeds the numerical budget.
    #[error("Euler angular rate {found} is outside {min}..={max} prad/year")]
    AngularRateOutOfRange { found: u64, min: u64, max: u64 },
    /// A local velocity query used a non-positive or non-finite sphere radius.
    #[error("spherical tectonic radius must be finite and positive, got {found_m} m")]
    InvalidRadius { found_m: f64 },
    /// A radius and angular rate would exceed the local plate-speed envelope.
    #[error("maximum plate speed {found_mm_per_year} mm/year exceeds {max_mm_per_year} mm/year")]
    PlateSpeedOutOfRange {
        found_mm_per_year: f64,
        max_mm_per_year: f64,
    },
    /// The plate table is empty or larger than the referenced cell allocation.
    #[error("plate count {found} is invalid for {cell_count} cells")]
    InvalidPlateCount { found: usize, cell_count: usize },
    /// Plate identifiers are not the exact contiguous stable range.
    #[error("expected plate ID {expected:?}, found {found:?}")]
    NonContiguousPlateId { expected: PlateId, found: PlateId },
    /// A plate seed lies outside the referenced surface allocation.
    #[error("plate {plate:?} seed {seed:?} lies outside {cell_count} cells")]
    InvalidPlateSeed {
        plate: PlateId,
        seed: CellId,
        cell_count: usize,
    },
    /// A dense field length differs from the surface identity.
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    /// A cell references a plate outside the stable plate table.
    #[error("cell {cell:?} references invalid plate {plate:?}")]
    InvalidCellPlate { cell: CellId, plate: PlateId },
    /// A dense crust category is not one of the stable semantic values.
    #[error("cell {cell:?} stores invalid crust kind {found}")]
    InvalidCrustKind { cell: CellId, found: u32 },
    /// Crust thickness is non-finite or outside its material-specific range.
    #[error("cell {cell:?} {kind:?} crust thickness {found} is outside {min}..={max} km")]
    CrustThicknessOutOfRange {
        cell: CellId,
        kind: CrustKind,
        found: f32,
        min: f32,
        max: f32,
    },
    /// Oceanic age is outside the budget, or continental age is not the canonical sentinel.
    #[error("cell {cell:?} {kind:?} crust age {found} Myr is invalid")]
    CrustAgeOutOfRange {
        cell: CellId,
        kind: CrustKind,
        found: f32,
    },
    /// Current tectonic elevation is non-finite or outside the shared relief range.
    #[error("cell {cell:?} tectonic elevation {found} is outside {min}..={max} m")]
    TectonicElevationOutOfRange {
        cell: CellId,
        found: f32,
        min: f32,
        max: f32,
    },
    /// Current lineation is neither exactly absent nor a finite unit tangent direction.
    #[error("cell {cell:?} crust lineation [{east}, {north}] is neither zero nor unit length")]
    InvalidCrustLineation { cell: CellId, east: f32, north: f32 },
    /// Orogeny age is inconsistent with its current orogeny category.
    #[error("cell {cell:?} {kind:?} orogeny age {found} Myr is invalid")]
    OrogenyAgeOutOfRange {
        cell: CellId,
        kind: SphericalOrogenyKind,
        found: f32,
    },
    /// A boundary strength is non-finite or outside zero to one.
    #[error("edge {edge:?} boundary strength {found} is outside 0..=1")]
    BoundaryStrengthOutOfRange { edge: EdgeId, found: f32 },
    /// A boundary kind, segment, strength, and subduction polarity are inconsistent.
    #[error("edge {edge:?} stores an inconsistent boundary record")]
    InvalidBoundaryRecord { edge: EdgeId },
    /// Segment identifiers are not the exact contiguous stable range.
    #[error("expected boundary segment {expected:?}, found {found:?}")]
    NonContiguousBoundarySegmentId {
        expected: BoundarySegmentId,
        found: BoundarySegmentId,
    },
    /// A segment header, plate pair, kind, mean, or member list is malformed.
    #[error("boundary segment {segment:?} is malformed")]
    InvalidBoundarySegment { segment: BoundarySegmentId },
    /// An edge appears in more than one segment.
    #[error("boundary edge {edge:?} appears in more than one segment")]
    DuplicateBoundaryEdge { edge: EdgeId },
    /// A segment and one of its edge-aligned records disagree.
    #[error("segment {segment:?} disagrees with member edge {edge:?}")]
    BoundarySegmentMismatch {
        segment: BoundarySegmentId,
        edge: EdgeId,
    },
    /// A segment's stored mean does not match its member strengths.
    #[error("segment {segment:?} mean {stored} does not match {calculated}")]
    BoundaryMeanMismatch {
        segment: BoundarySegmentId,
        stored: f32,
        calculated: f32,
    },
    /// An edge record and the canonical segment membership partition disagree.
    #[error("edge {edge:?} boundary membership does not match its segment")]
    BoundaryMembershipMismatch { edge: EdgeId },
    /// A plate's declared seed is not owned by that plate.
    #[error("plate {plate:?} seed {seed:?} is owned by {owner:?}")]
    PlateSeedOwnership {
        plate: PlateId,
        seed: CellId,
        owner: Option<PlateId>,
    },
    /// Cells owned by a plate do not form one connected surface region.
    #[error("plate {plate:?} reaches {reached} of {expected} owned cells")]
    DisconnectedPlate {
        plate: PlateId,
        reached: usize,
        expected: usize,
    },
    /// Edge-aligned tectonic state disagrees with the authoritative owner topology.
    #[error("edge {edge:?} boundary state disagrees with its owner plates")]
    BoundaryTopologyMismatch { edge: EdgeId },
    /// Cached boundary semantics disagree with authoritative Euler motion and crust fields.
    #[error("edge {edge:?} boundary state disagrees with authoritative local kinematics")]
    BoundaryKinematicsMismatch { edge: EdgeId },
    /// Adjacent plates store an identical Euler rotation.
    #[error("adjacent plates {plates:?} have identical Euler rotations")]
    AdjacentPlatesCoMoving { plates: [PlateId; 2] },
    /// Member edges do not form one canonical-vertex-connected segment.
    #[error("segment {segment:?} reaches {reached} of {expected} member edges")]
    DisconnectedBoundarySegment {
        segment: BoundarySegmentId,
        reached: usize,
        expected: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        deserialize_boundary_segments_with_limit, deserialize_spherical_boundaries_with_limit,
        SphericalPlateRotation,
    };
    use crate::world::spatial::UnitVector3;
    use crate::world::Meters;

    #[test]
    fn edge_records_are_rejected_before_a_bounded_sequence_can_grow() {
        let json = r#"[
            {"kind":"None","strength":0.0,"segment_id":null,"subducting_plate":null},
            {"kind":"None","strength":0.0,"segment_id":null,"subducting_plate":null},
            {"kind":"None","strength":0.0,"segment_id":null,"subducting_plate":null}
        ]"#;
        let mut deserializer = serde_json::Deserializer::from_str(json);
        let error = deserialize_spherical_boundaries_with_limit::<_, 2>(&mut deserializer)
            .unwrap_err()
            .to_string();
        assert!(error.contains("at most 2 elements"), "{error}");
    }

    #[test]
    fn boundary_segments_share_one_aggregate_member_budget() {
        let json = r#"[
            {"id":0,"plates":[0,1],"kind":"Weak","member_edges":[0,1],"mean_strength":0.1,"subducting_plate":null},
            {"id":1,"plates":[0,1],"kind":"Weak","member_edges":[2],"mean_strength":0.1,"subducting_plate":null}
        ]"#;
        let mut deserializer = serde_json::Deserializer::from_str(json);
        let error = deserialize_boundary_segments_with_limit::<_, 2>(&mut deserializer)
            .unwrap_err()
            .to_string();
        assert!(error.contains("at most 2 are allowed"), "{error}");
    }

    #[test]
    fn angular_velocity_vector_round_trips_inside_the_speed_envelope() {
        let radius = Meters::new(6_371_000.0).unwrap();
        let omega = [0.0, 0.0, 1.0e-8];
        let rotation =
            SphericalPlateRotation::from_angular_velocity_rad_per_year(omega, radius).unwrap();
        let recovered = rotation.angular_velocity_vector_rad_per_year();
        assert!((recovered[2] - 1.0e-8).abs() < 1.0e-14);
        assert!(recovered[0].abs() < 1.0e-20);
        assert!(recovered[1].abs() < 1.0e-20);
        let pole = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
        assert_eq!(rotation.pole(), pole);
    }
}
