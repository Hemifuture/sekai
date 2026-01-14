// 水系生成（河流、湖泊）

use super::heightmap::SEA_LEVEL;
use std::collections::HashMap;

/// 河流
#[derive(Debug, Clone)]
pub struct River {
    pub id: u16,
    pub cells: Vec<u32>,        // 河流路径（从源头到河口）
    pub mouth_cell: u32,        // 河口单元格
    pub source_cells: Vec<u32>, // 源头单元格
    pub width_km: f32,          // 河口宽度（km）
    pub widths: Vec<u8>,        // 每个点的相对宽度
}

/// 湖泊
#[derive(Debug, Clone)]
pub struct Lake {
    pub id: u16,
    pub cells: Vec<u32>,      // 湖泊单元格
    pub elevation: u8,        // 水面高度
    pub outflow: Option<u32>, // 出水口单元格
}

/// 水系生成器
pub struct HydrologyGenerator {}

impl HydrologyGenerator {
    pub fn new() -> Self {
        Self {}
    }

    /// 计算流向
    /// 返回: 每个单元格的流向（指向最低邻居的索引，海洋单元格为 None）
    pub fn compute_flow_direction(
        &self,
        heights: &[u8],
        is_land: &[bool],
        neighbors: &[Vec<u32>],
    ) -> Vec<Option<u32>> {
        heights
            .iter()
            .enumerate()
            .map(|(i, &h)| {
                if !is_land[i] {
                    return None; // 海洋单元格
                }

                // 找到最低的邻居
                neighbors[i]
                    .iter()
                    .filter(|&&n| {
                        let n = n as usize;
                        heights[n] < h || !is_land[n] // 流向更低或流入海洋
                    })
                    .min_by_key(|&&n| heights[n as usize])
                    .copied()
            })
            .collect()
    }

    /// 计算流量（水流累积）
    pub fn compute_flux(
        &self,
        heights: &[u8],
        is_land: &[bool],
        flow_direction: &[Option<u32>],
        precipitation: Option<&[u8]>,
    ) -> Vec<u16> {
        // 按高度从高到低排序
        let mut sorted: Vec<usize> = (0..heights.len()).filter(|&i| is_land[i]).collect();
        sorted.sort_by(|&a, &b| heights[b].cmp(&heights[a]));

        // 初始化流量
        let mut flux: Vec<u16> = match precipitation {
            Some(precip) => precip.iter().map(|&p| p as u16).collect(),
            None => vec![1; heights.len()],
        };

        // 从高到低累积流量
        for &cell in &sorted {
            if let Some(downstream) = flow_direction[cell] {
                let downstream = downstream as usize;
                flux[downstream] = flux[downstream].saturating_add(flux[cell]);
            }
        }

        flux
    }

    /// 提取河流
    pub fn extract_rivers(
        &self,
        flux: &[u16],
        flow_direction: &[Option<u32>],
        is_land: &[bool],
        threshold: u16,
    ) -> Vec<River> {
        let mut visited = vec![false; flux.len()];
        let mut rivers = Vec::new();

        // 找到所有河口（流量大于阈值且流入海洋）
        let mouths: Vec<usize> = (0..flux.len())
            .filter(|&i| {
                flux[i] >= threshold
                    && is_land[i]
                    && flow_direction[i].map_or(false, |d| !is_land[d as usize])
            })
            .collect();

        for &mouth in &mouths {
            let river =
                self.trace_river_upstream(mouth, flux, flow_direction, &mut visited, threshold);

            if !river.cells.is_empty() {
                rivers.push(river);
            }
        }

        rivers
    }

    /// 从河口向上游追溯河流路径
    fn trace_river_upstream(
        &self,
        mouth: usize,
        flux: &[u16],
        flow_direction: &[Option<u32>],
        visited: &mut [bool],
        threshold: u16,
    ) -> River {
        let mut cells = Vec::new();
        let mut sources = Vec::new();

        // 反向构建流入关系
        let mut flows_into: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, &flow) in flow_direction.iter().enumerate() {
            if let Some(downstream) = flow {
                flows_into
                    .entry(downstream as usize)
                    .or_insert_with(Vec::new)
                    .push(i);
            }
        }

        // BFS 从河口向上游追溯
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(mouth);
        visited[mouth] = true;

        while let Some(current) = queue.pop_front() {
            cells.push(current as u32);

            if let Some(upstreams) = flows_into.get(&current) {
                let mut has_upstream = false;

                for &upstream in upstreams {
                    if flux[upstream] >= threshold && !visited[upstream] {
                        visited[upstream] = true;
                        queue.push_back(upstream);
                        has_upstream = true;
                    }
                }

                if !has_upstream {
                    sources.push(current as u32);
                }
            } else {
                sources.push(current as u32);
            }
        }

        // 反转路径（从源头到河口）
        cells.reverse();

        River {
            id: 0, // 将在后续分配
            cells,
            mouth_cell: mouth as u32,
            source_cells: sources,
            width_km: 1.0,
            widths: Vec::new(),
        }
    }

    /// 计算河流宽度
    pub fn calculate_river_widths(&self, rivers: &mut [River], flux: &[u16]) {
        for river in rivers.iter_mut() {
            let mouth_flux = flux[river.mouth_cell as usize] as f32;

            // 河口宽度（基于流量的对数）
            river.width_km = (mouth_flux.ln().max(1.0) * 0.5).max(0.1);

            // 每个点的相对宽度
            river.widths = river
                .cells
                .iter()
                .map(|&cell| {
                    let cell_flux = flux[cell as usize] as f32;
                    let relative = cell_flux / mouth_flux;
                    (relative.sqrt() * river.width_km * 10.0).min(255.0) as u8
                })
                .collect();
        }
    }

    /// 检测湖泊
    pub fn detect_lakes(
        &self,
        heights: &[u8],
        is_land: &[bool],
        neighbors: &[Vec<u32>],
    ) -> Vec<Lake> {
        let mut lakes = Vec::new();
        let mut in_lake = vec![false; heights.len()];

        for start in 0..heights.len() {
            if !is_land[start] || in_lake[start] {
                continue;
            }

            // 检查是否为局部最低点
            let is_depression = neighbors[start]
                .iter()
                .all(|&n| heights[n as usize] >= heights[start]);

            if is_depression {
                let lake = self.fill_depression(start, heights, is_land, neighbors);

                for &cell in &lake.cells {
                    in_lake[cell as usize] = true;
                }

                lakes.push(lake);
            }
        }

        lakes
    }

    /// 填充凹陷形成湖泊
    fn fill_depression(
        &self,
        start: usize,
        heights: &[u8],
        is_land: &[bool],
        neighbors: &[Vec<u32>],
    ) -> Lake {
        let mut cells = Vec::new();
        let mut visited = vec![false; heights.len()];
        let mut queue = std::collections::VecDeque::new();

        queue.push_back(start);
        visited[start] = true;
        let water_level = heights[start];

        while let Some(current) = queue.pop_front() {
            cells.push(current as u32);

            for &neighbor in &neighbors[current] {
                let neighbor = neighbor as usize;
                if !visited[neighbor] && is_land[neighbor] && heights[neighbor] <= water_level {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        // 找到出水口（最低的边界单元格）
        let outflow = cells
            .iter()
            .filter_map(|&cell| {
                neighbors[cell as usize]
                    .iter()
                    .find(|&&n| heights[n as usize] > water_level)
                    .copied()
            })
            .min_by_key(|&n| heights[n as usize]);

        Lake {
            id: 0,
            cells,
            elevation: water_level,
            outflow,
        }
    }
}

/// 分析海陆分布
pub fn classify_land_sea(heights: &[u8]) -> Vec<bool> {
    heights.iter().map(|&h| h >= SEA_LEVEL).collect()
}

/// 连通分量检测（用于识别独立的大陆和岛屿）
pub fn find_landmasses(is_land: &[bool], neighbors: &[Vec<u32>], min_size: usize) -> Vec<Landmass> {
    let mut visited = vec![false; is_land.len()];
    let mut landmasses = Vec::new();

    for start in 0..is_land.len() {
        if visited[start] || !is_land[start] {
            continue;
        }

        // BFS 洪水填充
        let mut cells = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        visited[start] = true;

        while let Some(cell) = queue.pop_front() {
            cells.push(cell as u32);

            for &neighbor in &neighbors[cell] {
                let neighbor = neighbor as usize;
                if !visited[neighbor] && is_land[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        if cells.len() >= min_size {
            let is_continent = cells.len() > min_size * 10;
            landmasses.push(Landmass {
                id: (landmasses.len() + 1) as u16,
                cells,
                is_continent,
            });
        }
    }

    landmasses
}

/// 陆块
#[derive(Debug, Clone)]
pub struct Landmass {
    pub id: u16,
    pub cells: Vec<u32>,
    pub is_continent: bool,
}

/// 提取海岸线
pub fn find_coastline_cells(is_land: &[bool], neighbors: &[Vec<u32>]) -> Vec<u32> {
    (0..is_land.len())
        .filter(|&i| is_land[i] && neighbors[i].iter().any(|&n| !is_land[n as usize]))
        .map(|i| i as u32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_land_sea() {
        let heights = vec![10, 20, 30, 5, 25];
        let is_land = classify_land_sea(&heights);

        assert_eq!(is_land, vec![false, true, true, false, true]);
    }

    #[test]
    fn test_flow_direction() {
        let heights = vec![100, 80, 60, 90, 70];
        let is_land = vec![true, true, true, true, true];
        let neighbors = vec![vec![1], vec![0, 2], vec![1, 4], vec![4], vec![2, 3]];

        let generator = HydrologyGenerator::new();
        let flow_dir = generator.compute_flow_direction(&heights, &is_land, &neighbors);

        // 100 -> 80 -> 60 -> 70 <- 90
        assert_eq!(flow_dir[0], Some(1));
        assert_eq!(flow_dir[1], Some(2));
        assert_eq!(flow_dir[3], Some(4));
    }
}
