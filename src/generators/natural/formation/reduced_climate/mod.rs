//! A10 reduced-order diagnostic seasonal climate (design
//! `docs/superpowers/specs/2026-10-25-a10-reduced-order-climate-design.md`, frozen at R5).
//!
//! The production modules land task by task (section 10) and stay off the causal chain until
//! T8 switches it; `prototype` is the test-only offline probe whose physics they port.

mod geometry;
#[cfg(test)]
mod prototype;
mod vector;

pub(in crate::generators::natural) use geometry::ReducedClimateGeometry;
