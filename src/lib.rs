#![warn(clippy::all, rust_2018_idioms)]

mod app;
pub mod delaunay;
mod gpu;
mod models;
mod resource;
mod ui;
pub use app::TemplateApp;
