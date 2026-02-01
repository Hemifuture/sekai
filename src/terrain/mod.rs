// 地形生成模块

pub mod plate;
pub mod noise;
pub mod heightmap;
pub mod hydrology;
pub mod template;
pub mod template_executor;
pub mod primitive;
pub mod dsl;
pub mod blob;
pub mod features;

// 模板测试
#[cfg(test)]
mod template_tests;

// 新增：分层地形生成系统
pub mod layers;
pub mod layered_generator;

pub use plate::*;
pub use noise::*;
pub use heightmap::*;
pub use hydrology::*;
pub use template::{
    TerrainTemplate, TerrainCommand, MaskMode, StraitDirection, InvertAxis,
    get_template_by_name, should_use_layered_generation, get_suggested_plate_count,
};
pub use template_executor::*;
pub use primitive::*;
pub use dsl::{parse_template, template_to_dsl};
pub use blob::{BlobGenerator, BlobConfig};
pub use features::{FeatureDetector, Feature, FeatureType};

// 导出分层系统
pub use layers::{
    LayerOutput, Pos2, TerrainLayer, TerrainContext, LegacyTerrainLayer,
    PlateLayer, PlateConfig, PlateType, BoundaryType, Plate,
    TectonicLayer, TectonicConfig, CollisionType,
    RegionalLayer, DetailLayer, PostprocessLayer,
};
pub use layered_generator::LayeredGenerator;
