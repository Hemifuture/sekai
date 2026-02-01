//! 分层地形生成器
//!
//! 管理和执行多个地形生成层

use crate::terrain::layers::{LayerOutput, Pos2, TerrainLayer};

/// 分层地形生成器
/// 
/// 按顺序执行多个 TerrainLayer，每层可基于前一层的输出进行处理
pub struct LayeredGenerator {
    /// 生成层列表，按执行顺序排列
    layers: Vec<Box<dyn TerrainLayer>>,
    /// 随机种子
    seed: u64,
}

impl LayeredGenerator {
    /// 创建空的分层生成器
    pub fn new() -> Self {
        Self { 
            layers: Vec::new(),
            seed: 0,
        }
    }
    
    /// 设置随机种子
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// 添加一个生成层
    pub fn add_layer<L: TerrainLayer + 'static>(mut self, layer: L) -> Self {
        self.layers.push(Box::new(layer));
        self
    }

    /// 添加一个 boxed 生成层
    pub fn add_boxed_layer(mut self, layer: Box<dyn TerrainLayer>) -> Self {
        self.layers.push(layer);
        self
    }

    /// 执行所有层生成地形
    /// 
    /// # 参数
    /// - `cells`: 所有单元格的位置坐标 (使用 eframe::egui::Pos2)
    /// - `neighbors`: 每个单元格的邻居索引
    /// 
    /// # 返回
    /// 最终的层输出结果
    pub fn generate(&self, cells: &[eframe::egui::Pos2], neighbors: &[Vec<u32>]) -> LayerOutput {
        // Convert eframe Pos2 to our Pos2
        let our_cells: Vec<Pos2> = cells.iter()
            .map(|p| Pos2::new(p.x, p.y))
            .collect();
        
        self.generate_internal(&our_cells, neighbors)
    }
    
    /// Internal generation with our Pos2 type
    fn generate_internal(&self, cells: &[Pos2], neighbors: &[Vec<u32>]) -> LayerOutput {
        let mut output = LayerOutput::with_size(cells.len());

        for layer in &self.layers {
            #[cfg(debug_assertions)]
            println!("执行层: {}", layer.name());
            output = layer.generate(cells, neighbors, &output);
        }

        output
    }

    /// 返回已注册的层数量
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// 返回所有层的名称
    pub fn layer_names(&self) -> Vec<&'static str> {
        self.layers.iter().map(|l| l.name()).collect()
    }
}

impl Default for LayeredGenerator {
    fn default() -> Self {
        Self::new()
    }
}
