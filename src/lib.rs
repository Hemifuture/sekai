#![warn(clippy::all, rust_2018_idioms)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::let_and_return)]
#![allow(clippy::derivable_impls)]

pub mod app;
pub mod delaunay;
/// Domain-neutral deterministic generation services.
pub mod engine;
/// Deterministic world-generation pipelines.
pub mod generators;
/// GPU presentation backends.
pub mod gpu;
mod map_layer;
pub mod models;
mod resource;
/// Deterministic, data-only rule-pack and author-input contracts.
pub mod rules;
pub mod spatial;
pub mod terrain;
mod ui;
/// Renderer-neutral, read-only world presentation contracts.
pub mod view;
pub mod world;
pub use app::TemplateApp;
