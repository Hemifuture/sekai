#![warn(clippy::all, rust_2018_idioms)]

mod app;
pub mod delaunay;
mod gpu;
mod map_layer;
pub mod models;
mod resource;
mod ui;
pub use app::TemplateApp;
