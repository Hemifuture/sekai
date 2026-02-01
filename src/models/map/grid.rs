use egui::Pos2;
use rand::Rng;

#[derive(Debug, Clone)]
pub struct Grid {
    pub width: u32,
    pub height: u32,
    pub spacing: u32,
    pub points: Vec<Pos2>,
    pub cells_x: u32,
    pub cells_y: u32,
}

impl Default for Grid {
    fn default() -> Self {
        // 减小间距以获得更高分辨率的地形细节
        // spacing=5 → 约 80,000 个点 (之前是 20,000)
        let mut grid = Self::new(2000, 1000, 5);
        grid.generate_points();
        grid
    }
}

impl Grid {
    pub fn new(width: u32, height: u32, spacing: u32) -> Self {
        let cells_x = (width as f32 / spacing as f32).floor() as u32;
        let cells_y = (height as f32 / spacing as f32).floor() as u32;

        Self {
            width,
            height,
            spacing,
            points: vec![],
            cells_x,
            cells_y,
        }
    }

    /// 创建基于期望的网格点数量的网格
    pub fn from_cells_count(width: u32, height: u32, cells_desired: u32) -> Self {
        // 根据期望的单元格数量计算网格间距
        let area = width as f64 * height as f64;
        let spacing = ((area / cells_desired as f64).sqrt() as u32).max(1);

        Self::new(width, height, spacing)
    }

    /// 生成所有点（包括抖动网格点和边界点）
    pub fn generate_points(&mut self) {
        self.generate_jittered_grid();
    }

    /// 生成抖动的网格点
    pub fn generate_jittered_grid(&mut self) {
        let mut rng = rand::rng();
        let mut points = Vec::new();

        // 抖动网格的参数设置
        let radius = self.spacing as f32 / 2.0; // 网格单元半径
        let jittering = radius * 0.9; // 最大偏移量（参考Fantasy-Map-Generator）

        // 网格中每个点加上随机偏移
        for y in (0..self.height).step_by(self.spacing as usize) {
            for x in (0..self.width).step_by(self.spacing as usize) {
                // 添加随机抖动，但确保点不超出地图边界
                let jitter_x = rng.random_range(-jittering..jittering);
                let jitter_y = rng.random_range(-jittering..jittering);

                let x_jittered = (x as f32 + radius + jitter_x)
                    .max(0.0)
                    .min(self.width as f32);
                let y_jittered = (y as f32 + radius + jitter_y)
                    .max(0.0)
                    .min(self.height as f32);

                points.push(Pos2::new(x_jittered as f32, y_jittered as f32));
            }
        }

        self.points = points;
    }

    /// 生成边界点，用于限制Voronoi图的范围
    pub fn generate_boundary_points(&self) -> Vec<Pos2> {
        let mut boundary_points = Vec::new();
        let offset = -(self.spacing as f32); // 边界偏移
        let boundary_spacing = self.spacing as f32 * 2.0;

        // 计算边界范围
        let w = self.width as f32 - offset * 2.0;
        let h = self.height as f32 - offset * 2.0;

        // 计算需要分布多少个点
        let number_x = (w / boundary_spacing).ceil() as i32 - 1;
        let number_y = (h / boundary_spacing).ceil() as i32 - 1;

        // 在四条边上放置点
        for i in 0..number_x {
            let x = w * (i as f32 + 0.5) / number_x as f32 + offset;

            // 上边界
            boundary_points.push(Pos2::new(x, offset));
            // 下边界
            boundary_points.push(Pos2::new(x, h + offset));
        }

        for i in 0..number_y {
            let y = h * (i as f32 + 0.5) / number_y as f32 + offset;

            // 左边界
            boundary_points.push(Pos2::new(offset, y));
            // 右边界
            boundary_points.push(Pos2::new(w + offset, y));
        }

        boundary_points
    }

    /// 获取所有点（包括内部和边界）
    pub fn get_all_points(&self) -> Vec<Pos2> {
        let mut all_points = self.points.clone();
        all_points.extend(self.generate_boundary_points());
        all_points
    }

    /// 在给定坐标处找到对应的网格单元索引
    pub fn find_grid_cell(&self, x: f32, y: f32) -> u32 {
        let cell_x = (x / self.spacing as f32)
            .min((self.cells_x - 1) as f32)
            .floor() as u32;
        let cell_y = (y / self.spacing as f32)
            .min((self.cells_y - 1) as f32)
            .floor() as u32;

        cell_y * self.cells_x + cell_x
    }
}
