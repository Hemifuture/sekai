//! TerrainLayer trait 定义分层地形生成的核心抽象

use std::collections::HashMap;

/// 2D 位置坐标
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos2 {
    pub x: f32,
    pub y: f32,
}

impl Pos2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 层输出结果，包含高度图和可选的元数据
#[derive(Debug, Clone, Default)]
pub struct LayerOutput {
    /// 每个单元格的高度值
    pub heights: Vec<f32>,
    /// 板块 ID（用于板块构造层）
    pub plate_ids: Option<Vec<u16>>,
    /// 边界单元格索引（板块边界、海岸线等）
    pub boundary_cells: Option<Vec<u32>>,
    /// 自定义元数据，键值对形式存储额外数据
    pub metadata: HashMap<String, Vec<f32>>,
}

impl LayerOutput {
    /// 创建空的层输出
    pub fn empty() -> Self {
        Self::default()
    }

    /// 创建指定大小的层输出，高度初始化为 0
    pub fn with_size(cell_count: usize) -> Self {
        Self {
            heights: vec![0.0; cell_count],
            ..Default::default()
        }
    }
}

/// 地形生成层 trait（基于单元格批量处理）
/// 
/// 每个层负责地形生成的一个方面（如板块、侵蚀、河流等）
/// 层按顺序执行，每层可以读取前一层的输出并生成新的输出
pub trait TerrainLayer: Send + Sync {
    /// 生成该层的地形数据
    /// 
    /// # 参数
    /// - `cells`: 所有单元格的位置
    /// - `neighbors`: 每个单元格的邻居索引列表
    /// - `previous`: 前一层的输出结果
    /// 
    /// # 返回
    /// 该层生成的输出结果
    fn generate(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        previous: &LayerOutput,
    ) -> LayerOutput;

    /// 返回该层的名称，用于调试和日志
    fn name(&self) -> &'static str;
}

/// 地形上下文（用于旧版点采样层）
#[derive(Debug, Clone)]
pub struct TerrainContext {
    /// World X coordinate
    pub x: f64,
    /// World Y coordinate  
    pub y: f64,
    /// Current elevation value (modified by layers)
    pub elevation: f64,
    /// Base continental value (-1 = deep ocean, +1 = continental core)
    pub continental: f64,
    /// Distance to nearest coastline (0 = at coast, positive = inland, negative = ocean)
    pub coast_distance: f64,
    /// Whether this point is land (elevation > sea level)
    pub is_land: bool,
}

impl TerrainContext {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            elevation: 0.0,
            continental: 0.0,
            coast_distance: 0.0,
            is_land: false,
        }
    }
    
    /// Update land status based on current elevation
    pub fn update_land_status(&mut self, sea_level: f64) {
        self.is_land = self.elevation > sea_level;
    }
}

/// 旧版地形层 trait（基于单点采样）
/// 用于 DetailLayer、RegionalLayer 等现有实现
pub trait LegacyTerrainLayer: Send + Sync {
    /// Layer name for debugging/logging
    fn name(&self) -> &'static str;
    
    /// Apply this layer's modification to the terrain context
    fn apply(&self, ctx: &mut TerrainContext);
    
    /// Get the raw contribution of this layer without applying
    fn sample(&self, ctx: &TerrainContext) -> f64;
}
