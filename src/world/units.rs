use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Errors returned when constructing validated world units and bounds.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum UnitError {
    /// The supplied value was not finite.
    #[error("value must be finite, got {0}")]
    NonFinite(f64),
    /// The supplied area was negative.
    #[error("area must be non-negative, got {0}")]
    NegativeArea(f64),
    /// The rectangle corners did not form a finite, positive-area rectangle.
    #[error("rectangle max must be greater than min on both axes")]
    InvalidRectangle,
}

/// A finite distance measured in meters.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[repr(transparent)]
pub struct Meters(f64);

impl Meters {
    /// Creates a finite distance in meters.
    pub fn new(value: f64) -> Result<Self, UnitError> {
        value
            .is_finite()
            .then_some(Self(value))
            .ok_or(UnitError::NonFinite(value))
    }

    /// Returns the distance in meters.
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Meters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(f64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A finite, non-negative area measured in square meters.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[repr(transparent)]
pub struct SquareMeters(f64);

impl SquareMeters {
    /// Creates a finite, non-negative area in square meters.
    pub fn new(value: f64) -> Result<Self, UnitError> {
        if !value.is_finite() {
            return Err(UnitError::NonFinite(value));
        }
        if value < 0.0 {
            return Err(UnitError::NegativeArea(value));
        }

        Ok(Self(value))
    }

    /// Returns the area in square meters.
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SquareMeters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(f64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A point in world-space coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldPoint {
    x: Meters,
    y: Meters,
}

impl WorldPoint {
    /// Creates a point from its world-space coordinates.
    pub const fn new(x: Meters, y: Meters) -> Self {
        Self { x, y }
    }

    /// Returns the point's horizontal coordinate.
    pub const fn x(self) -> Meters {
        self.x
    }

    /// Returns the point's vertical coordinate.
    pub const fn y(self) -> Meters {
        self.y
    }
}

/// An axis-aligned, positive-area rectangle in world-space coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WorldRect {
    min: WorldPoint,
    max: WorldPoint,
}

#[derive(Deserialize)]
struct WorldRectWire {
    min: WorldPoint,
    max: WorldPoint,
}

impl<'de> Deserialize<'de> for WorldRect {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorldRectWire::deserialize(deserializer)?;
        Self::new(wire.min, wire.max).map_err(D::Error::custom)
    }
}

impl WorldRect {
    /// Creates a rectangle when `max` lies strictly above and to the right of `min`.
    pub fn new(min: WorldPoint, max: WorldPoint) -> Result<Self, UnitError> {
        let width = max.x.get() - min.x.get();
        let height = max.y.get() - min.y.get();
        if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
            return Err(UnitError::InvalidRectangle);
        }

        Ok(Self { min, max })
    }

    /// Returns the rectangle's minimum corner.
    pub const fn min(self) -> WorldPoint {
        self.min
    }

    /// Returns the rectangle's maximum corner.
    pub const fn max(self) -> WorldPoint {
        self.max
    }

    /// Returns the rectangle's width in meters.
    pub fn width(self) -> Meters {
        Meters(self.max.x.get() - self.min.x.get())
    }

    /// Returns the rectangle's height in meters.
    pub fn height(self) -> Meters {
        Meters(self.max.y.get() - self.min.y.get())
    }

    /// Returns whether a point lies inside or on the rectangle boundary.
    pub fn contains(self, point: WorldPoint) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }
}
