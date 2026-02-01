// 地形模板执行器
//
// 执行地形模板命令，修改高度图数据

use super::blob::{BlobConfig, BlobGenerator};
use super::template::{
    InvertAxis, MaskMode, StraitDirection, TerrainCommand, TerrainTemplate,
};
use eframe::egui::Pos2;
use rand::{Rng, SeedableRng};

/// 生成模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationMode {
    /// 传统模式：基于距离衰减的几何形状
    Classic,
    /// BFS 模式：基于 BFS 扩散的自然形状（参考 Azgaar）
    BfsBlob,
}

impl Default for GenerationMode {
    fn default() -> Self {
        Self::BfsBlob  // 默认使用 BFS 模式
    }
}

/// 模板执行器
pub struct TemplateExecutor {
    width: u32,
    height: u32,
    seed: u64,
    mode: GenerationMode,
}

impl TemplateExecutor {
    pub fn new(width: u32, height: u32, seed: u64) -> Self {
        Self {
            width,
            height,
            seed,
            mode: GenerationMode::BfsBlob,  // 默认使用 BFS 模式
        }
    }

    /// 使用指定模式创建执行器
    pub fn with_mode(width: u32, height: u32, seed: u64, mode: GenerationMode) -> Self {
        Self {
            width,
            height,
            seed,
            mode,
        }
    }

    /// 设置生成模式
    pub fn set_mode(&mut self, mode: GenerationMode) {
        self.mode = mode;
    }

    /// 执行模板，生成高度图
    pub fn execute(
        &self,
        template: &TerrainTemplate,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> Vec<f32> {
        let mut heights = vec![0.0; cells.len()];
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);

        #[cfg(debug_assertions)]
        println!(
            "执行地形模板: {} - {}",
            template.name, template.description
        );

        for (idx, command) in template.commands.iter().enumerate() {
            #[cfg(debug_assertions)]
            println!("  [{}] 执行命令: {:?}", idx + 1, command);

            self.execute_command(command, &mut heights, cells, neighbors, &mut rng);
        }

        heights
    }

    /// 执行单个命令
    fn execute_command(
        &self,
        command: &TerrainCommand,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        rng: &mut rand::rngs::StdRng,
    ) {
        match command {
            TerrainCommand::Mountain {
                height,
                x,
                y,
                radius,
            } => {
                self.apply_mountain(heights, cells, *height, *x, *y, *radius);
            }

            TerrainCommand::Hill {
                count,
                height,
                x,
                y,
                radius,
            } => {
                for _ in 0..*count {
                    let h = rng.random_range(height.0..=height.1);
                    let px = rng.random_range(x.0..=x.1);
                    let py = rng.random_range(y.0..=y.1);
                    let r = rng.random_range(radius.0..=radius.1);
                    
                    match self.mode {
                        GenerationMode::Classic => {
                            self.apply_hill(heights, cells, h, px, py, r);
                        }
                        GenerationMode::BfsBlob => {
                            self.apply_hill_bfs(heights, cells, neighbors, h, px, py, rng);
                        }
                    }
                }
            }

            TerrainCommand::Pit {
                count,
                depth,
                x,
                y,
                radius,
            } => {
                for _ in 0..*count {
                    let d = rng.random_range(depth.0..=depth.1);
                    let px = rng.random_range(x.0..=x.1);
                    let py = rng.random_range(y.0..=y.1);
                    let r = rng.random_range(radius.0..=radius.1);
                    
                    match self.mode {
                        GenerationMode::Classic => {
                            self.apply_pit(heights, cells, d, px, py, r);
                        }
                        GenerationMode::BfsBlob => {
                            self.apply_pit_bfs(heights, cells, neighbors, d, px, py, rng);
                        }
                    }
                }
            }

            TerrainCommand::Range {
                count,
                height,
                x,
                y,
                length,
                width,
                angle,
            } => {
                for _ in 0..*count {
                    let h = rng.random_range(height.0..=height.1);
                    let px = rng.random_range(x.0..=x.1);
                    let py = rng.random_range(y.0..=y.1);
                    let len = rng.random_range(length.0..=length.1);
                    let w = rng.random_range(width.0..=width.1);
                    let a = rng.random_range(angle.0..=angle.1);
                    
                    match self.mode {
                        GenerationMode::Classic => {
                            self.apply_range(heights, cells, h, px, py, len, w, a);
                        }
                        GenerationMode::BfsBlob => {
                            self.apply_range_bfs(heights, cells, neighbors, h, px, py, len, a, rng);
                        }
                    }
                }
            }

            TerrainCommand::Trough {
                count,
                depth,
                x,
                y,
                length,
                width,
                angle,
            } => {
                for _ in 0..*count {
                    let d = rng.random_range(depth.0..=depth.1);
                    let px = rng.random_range(x.0..=x.1);
                    let py = rng.random_range(y.0..=y.1);
                    let len = rng.random_range(length.0..=length.1);
                    let w = rng.random_range(width.0..=width.1);
                    let a = rng.random_range(angle.0..=angle.1);
                    
                    match self.mode {
                        GenerationMode::Classic => {
                            self.apply_trough(heights, cells, d, px, py, len, w, a);
                        }
                        GenerationMode::BfsBlob => {
                            self.apply_trough_bfs(heights, cells, neighbors, d, px, py, len, a, rng);
                        }
                    }
                }
            }

            TerrainCommand::Strait {
                width,
                direction,
                position,
                depth,
            } => {
                self.apply_strait(heights, cells, *width, *direction, *position, *depth);
            }

            TerrainCommand::Add { value } => {
                for h in heights.iter_mut() {
                    *h += value;
                }
            }

            TerrainCommand::Multiply { factor } => {
                for h in heights.iter_mut() {
                    *h *= factor;
                }
            }

            TerrainCommand::Smooth { iterations } => {
                self.smooth_heights(heights, neighbors, *iterations);
            }

            TerrainCommand::Mask { mode, strength } => {
                self.apply_mask(heights, cells, *mode, *strength);
            }

            TerrainCommand::Invert { axis, probability } => {
                if rng.random::<f32>() < *probability {
                    self.invert_heights(heights, cells, *axis);
                }
            }

            TerrainCommand::Normalize => {
                self.normalize_heights(heights);
            }

            TerrainCommand::SetSeaLevel { level } => {
                // 海平面设置只是一个标记，实际应用在后续处理中
                // 这里可以选择将低于海平面的区域进一步降低
                for h in heights.iter_mut() {
                    if *h < *level {
                        *h = (*h / level) * level * 0.8; // 进一步降低海洋区域
                    }
                }
            }

            TerrainCommand::AdjustSeaRatio { ocean_ratio } => {
                self.adjust_sea_ratio(heights, *ocean_ratio);
            }
        }
    }

    /// 调整海陆比例
    /// 通过重新映射高度值，使得指定比例的区域落在海平面以下
    fn adjust_sea_ratio(&self, heights: &mut [f32], ocean_ratio: f32) {
        use super::heightmap::SEA_LEVEL;

        if heights.is_empty() {
            return;
        }

        // 对高度值排序以找到分位数
        let mut sorted: Vec<f32> = heights.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // 找到应该成为海平面的分位数位置
        let percentile_idx = ((sorted.len() as f32) * ocean_ratio.clamp(0.0, 1.0)) as usize;
        let percentile_idx = percentile_idx.min(sorted.len() - 1);
        let threshold = sorted[percentile_idx];

        // 重新映射高度值
        // 低于 threshold 的映射到 0 ~ SEA_LEVEL
        // 高于 threshold 的映射到 SEA_LEVEL ~ 255
        let sea_level = SEA_LEVEL as f32;
        let min_h = sorted[0];
        let max_h = sorted[sorted.len() - 1];

        for h in heights.iter_mut() {
            if *h <= threshold {
                // 海洋区域：映射到 0 ~ SEA_LEVEL
                if (threshold - min_h).abs() > 0.001 {
                    *h = (*h - min_h) / (threshold - min_h) * sea_level;
                } else {
                    *h = sea_level * 0.5;
                }
            } else {
                // 陆地区域：映射到 SEA_LEVEL ~ 255
                if (max_h - threshold).abs() > 0.001 {
                    *h = sea_level + (*h - threshold) / (max_h - threshold) * (255.0 - sea_level);
                } else {
                    *h = sea_level + (255.0 - sea_level) * 0.5;
                }
            }
        }
    }

    /// 应用山脉效果
    fn apply_mountain(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        height: f32,
        center_x: f32,
        center_y: f32,
        radius: f32,
    ) {
        let center = Pos2::new(
            center_x * self.width as f32,
            center_y * self.height as f32,
        );
        let radius_pixels = radius * self.width.max(self.height) as f32;

        for (i, pos) in cells.iter().enumerate() {
            let dist = pos.distance(center);
            if dist < radius_pixels {
                let falloff = 1.0 - (dist / radius_pixels).powi(2);
                let falloff = falloff.max(0.0);
                heights[i] += height * falloff;
            }
        }
    }

    /// 应用丘陵效果
    fn apply_hill(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        height: f32,
        center_x: f32,
        center_y: f32,
        radius: f32,
    ) {
        self.apply_mountain(heights, cells, height, center_x, center_y, radius);
    }

    /// 应用坑洞效果
    fn apply_pit(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        depth: f32,
        center_x: f32,
        center_y: f32,
        radius: f32,
    ) {
        self.apply_mountain(heights, cells, -depth, center_x, center_y, radius);
    }

    /// 应用山脉效果
    fn apply_range(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        height: f32,
        center_x: f32,
        center_y: f32,
        length: f32,
        width: f32,
        angle: f32,
    ) {
        let center = Pos2::new(
            center_x * self.width as f32,
            center_y * self.height as f32,
        );
        let length_pixels = length * self.width.max(self.height) as f32;
        let width_pixels = width * self.width.max(self.height) as f32;

        // 山脉方向向量
        let dir = Pos2::new(angle.cos(), angle.sin());
        let perp = Pos2::new(-angle.sin(), angle.cos());

        for (i, pos) in cells.iter().enumerate() {
            let relative = *pos - center;

            // 计算沿山脉方向和垂直方向的距离
            let along = relative.x * dir.x + relative.y * dir.y;
            let across = relative.x * perp.x + relative.y * perp.y;

            // 检查是否在山脉范围内
            if along.abs() < length_pixels / 2.0 && across.abs() < width_pixels {
                let along_falloff = 1.0 - (along.abs() / (length_pixels / 2.0)).powi(2);
                let across_falloff = 1.0 - (across.abs() / width_pixels).powi(2);
                let falloff = (along_falloff * across_falloff).max(0.0);
                heights[i] += height * falloff;
            }
        }
    }

    /// 应用海沟效果
    fn apply_trough(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        depth: f32,
        center_x: f32,
        center_y: f32,
        length: f32,
        width: f32,
        angle: f32,
    ) {
        self.apply_range(
            heights, cells, -depth, center_x, center_y, length, width, angle,
        );
    }

    /// 应用海峡效果
    fn apply_strait(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        width: f32,
        direction: StraitDirection,
        position: f32,
        depth: f32,
    ) {
        let width_pixels = width * self.width.min(self.height) as f32;

        for (i, pos) in cells.iter().enumerate() {
            let dist = match direction {
                StraitDirection::Vertical => {
                    (pos.x - position * self.width as f32).abs()
                }
                StraitDirection::Horizontal => {
                    (pos.y - position * self.height as f32).abs()
                }
            };

            if dist < width_pixels {
                let falloff = 1.0 - (dist / width_pixels);
                heights[i] -= depth * falloff;
            }
        }
    }

    /// 应用遮罩效果
    fn apply_mask(&self, heights: &mut [f32], cells: &[Pos2], mode: MaskMode, strength: f32) {
        let center = Pos2::new(self.width as f32 / 2.0, self.height as f32 / 2.0);
        let max_dist = (self.width as f32 / 2.0)
            .hypot(self.height as f32 / 2.0);

        for (i, pos) in cells.iter().enumerate() {
            let dist = pos.distance(center);
            let normalized_dist = (dist / max_dist).clamp(0.0, 1.0);

            let factor = match mode {
                MaskMode::EdgeFade => {
                    // 边缘降低
                    1.0 - normalized_dist * strength
                }
                MaskMode::CenterBoost => {
                    // 中心升高，边缘降低
                    1.0 + (1.0 - normalized_dist) * strength - normalized_dist * strength
                }
                MaskMode::RadialGradient => {
                    // 径向渐变
                    1.0 - normalized_dist * strength
                }
            };

            heights[i] *= factor;
        }
    }

    /// 反转高度图
    fn invert_heights(&self, heights: &mut [f32], cells: &[Pos2], axis: InvertAxis) {
        let center_x = self.width as f32 / 2.0;
        let center_y = self.height as f32 / 2.0;

        // 创建位置到索引的映射
        let mut pos_to_idx: std::collections::HashMap<(i32, i32), usize> =
            std::collections::HashMap::new();
        for (i, pos) in cells.iter().enumerate() {
            let key = (pos.x as i32, pos.y as i32);
            pos_to_idx.insert(key, i);
        }

        let original = heights.to_vec();

        for (i, pos) in cells.iter().enumerate() {
            let mirrored_pos = match axis {
                InvertAxis::X => Pos2::new(2.0 * center_x - pos.x, pos.y),
                InvertAxis::Y => Pos2::new(pos.x, 2.0 * center_y - pos.y),
                InvertAxis::Both => Pos2::new(2.0 * center_x - pos.x, 2.0 * center_y - pos.y),
            };

            // 查找镜像位置的索引
            let key = (mirrored_pos.x as i32, mirrored_pos.y as i32);
            if let Some(&mirrored_idx) = pos_to_idx.get(&key) {
                heights[i] = original[mirrored_idx];
            }
        }
    }

    /// 平滑高度
    fn smooth_heights(&self, heights: &mut [f32], neighbors: &[Vec<u32>], iterations: u32) {
        for _ in 0..iterations {
            let original = heights.to_vec();

            for i in 0..heights.len() {
                if neighbors[i].is_empty() {
                    continue;
                }

                let neighbor_avg: f32 = neighbors[i]
                    .iter()
                    .map(|&n| original[n as usize])
                    .sum::<f32>()
                    / neighbors[i].len() as f32;

                heights[i] = heights[i] * 0.5 + neighbor_avg * 0.5;
            }
        }
    }

    /// 归一化高度
    fn normalize_heights(&self, heights: &mut [f32]) {
        if heights.is_empty() {
            return;
        }

        let min = heights.iter().copied().fold(f32::INFINITY, f32::min);
        let max = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);

        if (max - min).abs() < 0.001 {
            return;
        }

        for h in heights.iter_mut() {
            *h = (*h - min) / (max - min) * 255.0;
        }
    }

    // ============================================================================
    // BFS 扩散式方法（参考 Azgaar Fantasy Map Generator）
    // ============================================================================

    /// BFS 扩散式丘陵
    fn apply_hill_bfs(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        height: f32,
        center_x: f32,
        center_y: f32,
        rng: &mut rand::rngs::StdRng,
    ) {
        let blob_config = BlobConfig::from_cell_count(cells.len());
        let blob_gen = BlobGenerator::new(blob_config);

        let x = center_x * self.width as f32;
        let y = center_y * self.height as f32;
        let start_idx = BlobGenerator::find_nearest_cell(cells, x, y);

        blob_gen.add_hill(heights, neighbors, start_idx, height, rng);
    }

    /// BFS 扩散式坑洞
    fn apply_pit_bfs(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        depth: f32,
        center_x: f32,
        center_y: f32,
        rng: &mut rand::rngs::StdRng,
    ) {
        let blob_config = BlobConfig::from_cell_count(cells.len());
        let blob_gen = BlobGenerator::new(blob_config);

        let x = center_x * self.width as f32;
        let y = center_y * self.height as f32;
        let start_idx = BlobGenerator::find_nearest_cell(cells, x, y);

        blob_gen.add_pit(heights, neighbors, start_idx, depth, rng);
    }

    /// BFS 扩散式山脉
    fn apply_range_bfs(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        height: f32,
        center_x: f32,
        center_y: f32,
        length: f32,
        angle: f32,
        rng: &mut rand::rngs::StdRng,
    ) {
        let blob_config = BlobConfig::from_cell_count(cells.len());
        let blob_gen = BlobGenerator::new(blob_config);

        // 计算起点和终点
        let half_len = length * self.width.max(self.height) as f32 / 2.0;
        let cx = center_x * self.width as f32;
        let cy = center_y * self.height as f32;

        let start_x = cx - half_len * angle.cos();
        let start_y = cy - half_len * angle.sin();
        let end_x = cx + half_len * angle.cos();
        let end_y = cy + half_len * angle.sin();

        let start_idx = BlobGenerator::find_nearest_cell(cells, start_x, start_y);
        let end_idx = BlobGenerator::find_nearest_cell(cells, end_x, end_y);

        blob_gen.add_range(heights, cells, neighbors, start_idx, end_idx, height, rng);
    }

    /// BFS 扩散式海沟
    fn apply_trough_bfs(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        depth: f32,
        center_x: f32,
        center_y: f32,
        length: f32,
        angle: f32,
        rng: &mut rand::rngs::StdRng,
    ) {
        let blob_config = BlobConfig::from_cell_count(cells.len());
        let blob_gen = BlobGenerator::new(blob_config);

        // 计算起点和终点
        let half_len = length * self.width.max(self.height) as f32 / 2.0;
        let cx = center_x * self.width as f32;
        let cy = center_y * self.height as f32;

        let start_x = cx - half_len * angle.cos();
        let start_y = cy - half_len * angle.sin();
        let end_x = cx + half_len * angle.cos();
        let end_y = cy + half_len * angle.sin();

        let start_idx = BlobGenerator::find_nearest_cell(cells, start_x, start_y);
        let end_idx = BlobGenerator::find_nearest_cell(cells, end_x, end_y);

        blob_gen.add_trough(heights, cells, neighbors, start_idx, end_idx, depth, rng);
    }
}
