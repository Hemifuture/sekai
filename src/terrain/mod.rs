// 地形生成模块

pub mod blob;
pub mod dsl;
pub mod features;
pub mod heightmap;
pub mod hydrology;
pub mod noise;
pub mod plate;
pub mod primitive;
pub mod template;
pub mod template_executor;

// 模板测试
#[cfg(test)]
mod template_tests;

// 新增：分层地形生成系统
pub mod layered_generator;
pub mod layers;

pub use blob::{BlobConfig, BlobGenerator};
pub use dsl::{parse_template, template_to_dsl};
pub use features::{Feature, FeatureDetector, FeatureType};
pub use heightmap::*;
pub use hydrology::*;
pub use noise::*;
pub use plate::*;
pub use primitive::*;
pub use template::{
    get_suggested_plate_count, get_suggested_ocean_ratio, get_template_by_name, should_use_layered_generation, InvertAxis,
    MaskMode, StraitDirection, TerrainCommand, TerrainTemplate,
};
pub use template_executor::*;

// 导出分层系统
pub use layered_generator::LayeredGenerator;
pub use layers::{
    BoundaryType, CollisionType, DetailLayer, LayerOutput, LegacyTerrainLayer, Plate, PlateConfig,
    PlateLayer, PlateType, Pos2, PostprocessLayer, RegionalLayer, TectonicConfig, TectonicLayer,
    TerrainContext, TerrainLayer,
};
