#![warn(clippy::all, rust_2018_idioms)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::let_and_return)]
#![allow(clippy::derivable_impls)]

mod app;
pub mod delaunay;
mod gpu;
mod map_layer;
pub mod models;
mod resource;
pub mod spatial;
pub mod terrain;
mod ui;
pub use app::TemplateApp;
