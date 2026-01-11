# Sekai 算法文档

本文档详细描述地图生成器中使用的各种算法。

---

## 一、地形生成算法

### 1.1 噪声函数基础

#### 1.1.1 Perlin/Simplex 噪声

噪声函数是程序化地形生成的基础。

**特点**:
- 连续性：相邻点的值变化平滑
- 可重复性：相同输入产生相同输出
- 伪随机性：看起来随机但可控

**多层叠加 (Fractal Brownian Motion)**:

```rust
fn fbm_noise(x: f64, y: f64, config: &NoiseConfig) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = config.base_frequency;
    let mut max_value = 0.0;
    
    for _ in 0..config.octaves {
        value += noise.get([x * frequency, y * frequency]) * amplitude;
        max_value += amplitude;
        amplitude *= config.persistence;  // 通常 0.5
        frequency *= config.lacunarity;   // 通常 2.0
    }
    
    value / max_value  // 归一化到 [-1, 1]
}
```

**参数说明**:

| 参数 | 说明 | 典型值 |
|------|------|--------|
| octaves | 叠加层数 | 4-8 |
| persistence | 振幅衰减 | 0.5 |
| lacunarity | 频率倍增 | 2.0 |
| base_frequency | 基础频率 | 0.01-0.05 |

### 1.2 高度图生成策略

#### 1.2.1 纯噪声模式

适用于随机生成大陆形状。

```rust
fn generate_noise_heightmap(cells: &[Pos2], config: &Config) -> Vec<u8> {
    cells.par_iter().map(|pos| {
        // 归一化坐标到 [0, 1]
        let nx = pos.x / config.width as f32;
        let ny = pos.y / config.height as f32;
        
        // 基础噪声
        let height = fbm_noise(nx, ny, &config.noise);
        
        // 应用海陆比例调整
        // 通过偏移噪声值来控制海陆比例
        let adjusted = height + config.land_bias;
        
        // 归一化到 0-255
        ((adjusted + 1.0) / 2.0 * 255.0).clamp(0.0, 255.0) as u8
    }).collect()
}
```

#### 1.2.2 模板引导模式

使用预定义模板控制大陆形状。

**模板类型**:

| 模板 | 说明 | 公式 |
|------|------|------|
| 椭圆大陆 | 中心高，边缘低 | `1 - distance_to_center` |
| 群岛 | 多个高点 | `max(island1, island2, ...)` |
| 半球 | 一侧大陆一侧海洋 | `x > 0.5 ? 1 : 0` |
| 边缘海洋 | 边缘必须是海 | `smoothstep(edge_distance)` |

```rust
fn generate_template_heightmap(cells: &[Pos2], template: &Template, config: &Config) -> Vec<u8> {
    cells.par_iter().map(|pos| {
        let nx = pos.x / config.width as f32;
        let ny = pos.y / config.height as f32;
        
        // 噪声值
        let noise = fbm_noise(nx, ny, &config.noise);
        
        // 模板值
        let template_value = template.sample(nx, ny);
        
        // 混合（模板作为权重）
        let height = noise * template_value;
        
        ((height + 1.0) / 2.0 * 255.0).clamp(0.0, 255.0) as u8
    }).collect()
}
```

### 1.3 板块构造模拟（推荐）

板块构造是地球地形形成的根本机制。通过模拟板块运动，可以生成最真实的大陆、山脉和海沟分布。

#### 1.3.1 算法概述

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        板块构造模拟流程                                          │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Step 1: 板块生成                                                                │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  • 在地图上随机放置 N 个板块种子点                                            │ │
│  │  • 使用 Voronoi 图划分板块区域                                                │ │
│  │  • 为每个板块分配类型（大陆板块/海洋板块）                                     │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                              ↓                                                   │
│  Step 2: 运动向量分配                                                            │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  • 为每个板块分配运动方向（角度）                                              │ │
│  │  • 为每个板块分配运动速度                                                     │ │
│  │  • 可选：考虑板块旋转                                                         │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                              ↓                                                   │
│  Step 3: 边界分析（每次迭代）                                                     │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  • 计算相邻板块的相对运动                                                     │ │
│  │  • 判断边界类型：                                                             │ │
│  │    - 汇聚边界（碰撞）→ 造山/俯冲                                              │ │
│  │    - 分离边界（张裂）→ 裂谷/洋脊                                              │ │
│  │    - 转换边界（错动）→ 断层                                                   │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                              ↓                                                   │
│  Step 4: 高度更新                                                                │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  • 碰撞区域隆起（形成山脉）                                                    │ │
│  │  • 俯冲区域下沉（形成海沟）                                                    │ │
│  │  • 分离区域产生新地壳                                                         │ │
│  │  • 应用均衡调整（地壳均衡）                                                   │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                              ↓                                                   │
│  Step 5: 迭代（模拟地质时间）                                                     │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  • 重复 Step 3-4 多次                                                         │ │
│  │  • 每次迭代代表数百万年                                                       │ │
│  │  • 可选：板块合并/分裂                                                        │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                              ↓                                                   │
│  Step 6: 后处理                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  • 添加噪声细节                                                               │ │
│  │  • 平滑处理                                                                   │ │
│  │  • 可选：侵蚀模拟                                                             │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

#### 1.3.2 数据结构

```rust
/// 板块
#[derive(Debug, Clone)]
pub struct TectonicPlate {
    /// 板块 ID
    pub id: u16,
    /// 板块类型
    pub plate_type: PlateType,
    /// 运动方向（弧度，0 = 东，π/2 = 北）
    pub direction: f32,
    /// 运动速度（单位/迭代）
    pub speed: f32,
    /// 板块包含的单元格
    pub cells: Vec<u32>,
    /// 板块边界单元格
    pub boundary_cells: Vec<u32>,
    /// 板块质心
    pub centroid: Pos2,
    /// 板块密度（影响俯冲方向）
    pub density: f32,
}

/// 板块类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlateType {
    /// 大陆板块（密度低，浮力大，不易俯冲）
    Continental,
    /// 海洋板块（密度高，会俯冲到大陆板块下）
    Oceanic,
}

/// 板块边界类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryType {
    /// 汇聚边界 - 板块相向运动
    Convergent {
        /// 碰撞强度 (0.0-1.0)
        intensity: f32,
        /// 俯冲方向（哪个板块俯冲）
        subducting_plate: Option<u16>,
    },
    /// 分离边界 - 板块背向运动
    Divergent {
        /// 分离强度
        intensity: f32,
    },
    /// 转换边界 - 板块平行错动
    Transform {
        /// 错动强度
        intensity: f32,
    },
}

/// 板块边界段
#[derive(Debug, Clone)]
pub struct PlateBoundary {
    /// 边界涉及的两个板块
    pub plate_a: u16,
    pub plate_b: u16,
    /// 边界类型
    pub boundary_type: BoundaryType,
    /// 边界上的单元格
    pub cells: Vec<u32>,
}

/// 板块构造配置
#[derive(Debug, Clone)]
pub struct TectonicConfig {
    /// 板块数量
    pub plate_count: u32,
    /// 大陆板块比例 (0.0-1.0)
    pub continental_ratio: f32,
    /// 模拟迭代次数（地质时间）
    pub iterations: u32,
    /// 碰撞隆起速率
    pub collision_uplift_rate: f32,
    /// 俯冲海沟深度
    pub subduction_depth_rate: f32,
    /// 分离裂谷深度
    pub rift_depth_rate: f32,
    /// 均衡调整速率
    pub isostatic_rate: f32,
    /// 板块边缘影响范围
    pub boundary_width: f32,
    /// 噪声细节强度
    pub noise_strength: f32,
    /// 随机种子
    pub seed: u64,
}

impl Default for TectonicConfig {
    fn default() -> Self {
        Self {
            plate_count: 12,
            continental_ratio: 0.4,
            iterations: 100,
            collision_uplift_rate: 0.5,
            subduction_depth_rate: 0.3,
            rift_depth_rate: 0.2,
            isostatic_rate: 0.1,
            boundary_width: 50.0,
            noise_strength: 0.3,
            seed: 12345,
        }
    }
}
```

#### 1.3.3 板块生成

```rust
/// 生成初始板块
fn generate_plates(
    cells: &[Pos2],
    neighbors: &NeighborMap,
    config: &TectonicConfig,
    rng: &mut impl Rng
) -> Vec<TectonicPlate> {
    let n = cells.len();
    
    // 1. 随机选择板块种子点
    let seeds: Vec<usize> = (0..n)
        .choose_multiple(rng, config.plate_count as usize);
    
    // 2. 使用 Voronoi 式扩张分配单元格到板块
    let mut plate_id = vec![0u16; n];
    let mut queue = VecDeque::new();
    
    for (i, &seed) in seeds.iter().enumerate() {
        plate_id[seed] = (i + 1) as u16;
        queue.push_back(seed);
    }
    
    while let Some(cell) = queue.pop_front() {
        for &neighbor in &neighbors[cell] {
            if plate_id[neighbor] == 0 {
                plate_id[neighbor] = plate_id[cell];
                queue.push_back(neighbor);
            }
        }
    }
    
    // 3. 构建板块对象
    let mut plates: Vec<TectonicPlate> = (1..=config.plate_count as u16)
        .map(|id| {
            let plate_cells: Vec<u32> = plate_id.iter()
                .enumerate()
                .filter(|(_, &p)| p == id)
                .map(|(i, _)| i as u32)
                .collect();
            
            // 确定板块类型
            let plate_type = if rng.gen::<f32>() < config.continental_ratio {
                PlateType::Continental
            } else {
                PlateType::Oceanic
            };
            
            // 随机运动方向和速度
            let direction = rng.gen::<f32>() * std::f32::consts::TAU;
            let speed = rng.gen_range(0.5..2.0);
            
            // 计算质心
            let centroid = calculate_centroid(&plate_cells, cells);
            
            // 密度
            let density = match plate_type {
                PlateType::Continental => 2.7,  // g/cm³
                PlateType::Oceanic => 3.0,
            };
            
            TectonicPlate {
                id,
                plate_type,
                direction,
                speed,
                cells: plate_cells,
                boundary_cells: Vec::new(),  // 稍后计算
                centroid,
                density,
            }
        })
        .collect();
    
    // 4. 计算边界单元格
    for plate in &mut plates {
        plate.boundary_cells = find_plate_boundary_cells(
            &plate.cells, 
            &plate_id, 
            neighbors
        );
    }
    
    plates
}

/// 找到板块边界单元格
fn find_plate_boundary_cells(
    plate_cells: &[u32],
    plate_id: &[u16],
    neighbors: &NeighborMap
) -> Vec<u32> {
    let this_plate = plate_id[plate_cells[0] as usize];
    
    plate_cells.iter()
        .filter(|&&cell| {
            neighbors[cell as usize].iter()
                .any(|&n| plate_id[n] != this_plate)
        })
        .copied()
        .collect()
}
```

#### 1.3.4 边界分析

```rust
/// 分析所有板块边界
fn analyze_boundaries(
    plates: &[TectonicPlate],
    plate_id: &[u16],
    cells: &[Pos2],
    neighbors: &NeighborMap
) -> Vec<PlateBoundary> {
    let mut boundaries: HashMap<(u16, u16), Vec<u32>> = HashMap::new();
    
    // 收集所有边界单元格
    for plate in plates {
        for &cell in &plate.boundary_cells {
            for &neighbor in &neighbors[cell as usize] {
                let neighbor_plate = plate_id[neighbor];
                if neighbor_plate != plate.id && neighbor_plate != 0 {
                    let key = if plate.id < neighbor_plate {
                        (plate.id, neighbor_plate)
                    } else {
                        (neighbor_plate, plate.id)
                    };
                    boundaries.entry(key).or_default().push(cell);
                }
            }
        }
    }
    
    // 分析每个边界的类型
    boundaries.into_iter().map(|((id_a, id_b), boundary_cells)| {
        let plate_a = &plates[id_a as usize - 1];
        let plate_b = &plates[id_b as usize - 1];
        
        let boundary_type = classify_boundary(plate_a, plate_b, &boundary_cells, cells);
        
        PlateBoundary {
            plate_a: id_a,
            plate_b: id_b,
            boundary_type,
            cells: boundary_cells,
        }
    }).collect()
}

/// 判断边界类型
fn classify_boundary(
    plate_a: &TectonicPlate,
    plate_b: &TectonicPlate,
    boundary_cells: &[u32],
    cells: &[Pos2]
) -> BoundaryType {
    // 计算边界中点
    let boundary_center = calculate_centroid(boundary_cells, cells);
    
    // 计算两个板块相对于边界的运动分量
    // 运动向量
    let vel_a = Vec2::from_angle(plate_a.direction) * plate_a.speed;
    let vel_b = Vec2::from_angle(plate_b.direction) * plate_b.speed;
    
    // 边界法向量（从 A 指向 B）
    let normal = (plate_b.centroid - plate_a.centroid).normalized();
    
    // 相对运动在法向的投影
    let approach_a = vel_a.dot(normal);
    let approach_b = vel_b.dot(-normal);
    let relative_approach = approach_a + approach_b;
    
    // 相对运动在切向的投影
    let tangent = Vec2::new(-normal.y, normal.x);
    let shear_a = vel_a.dot(tangent);
    let shear_b = vel_b.dot(tangent);
    let relative_shear = (shear_a - shear_b).abs();
    
    // 判断边界类型
    if relative_approach > 0.3 {
        // 汇聚边界
        let intensity = relative_approach.min(1.0);
        
        // 判断俯冲方向（密度大的俯冲）
        let subducting = if plate_a.density > plate_b.density {
            Some(plate_a.id)
        } else if plate_b.density > plate_a.density {
            Some(plate_b.id)
        } else {
            None  // 大陆-大陆碰撞，无俯冲
        };
        
        BoundaryType::Convergent {
            intensity,
            subducting_plate: subducting,
        }
    } else if relative_approach < -0.3 {
        // 分离边界
        BoundaryType::Divergent {
            intensity: (-relative_approach).min(1.0),
        }
    } else if relative_shear > 0.3 {
        // 转换边界
        BoundaryType::Transform {
            intensity: relative_shear.min(1.0),
        }
    } else {
        // 近乎静止，默认为弱汇聚
        BoundaryType::Convergent {
            intensity: 0.1,
            subducting_plate: None,
        }
    }
}
```

#### 1.3.5 高度更新

```rust
/// 板块构造模拟主循环
fn simulate_tectonics(
    cells: &[Pos2],
    neighbors: &NeighborMap,
    config: &TectonicConfig
) -> Vec<f32> {
    let mut rng = StdRng::seed_from_u64(config.seed);
    
    // 初始化高度（大陆板块高，海洋板块低）
    let plates = generate_plates(cells, neighbors, config, &mut rng);
    let plate_id: Vec<u16> = compute_plate_ids(&plates, cells.len());
    
    let mut heights: Vec<f32> = plate_id.iter().map(|&pid| {
        if pid == 0 { return 0.0; }
        let plate = &plates[pid as usize - 1];
        match plate.plate_type {
            PlateType::Continental => 0.4,  // 海平面以上
            PlateType::Oceanic => 0.15,     // 海平面以下
        }
    }).collect();
    
    // 主模拟循环
    for iteration in 0..config.iterations {
        // 分析当前边界
        let boundaries = analyze_boundaries(&plates, &plate_id, cells, neighbors);
        
        // 应用地质过程
        for boundary in &boundaries {
            apply_boundary_effects(
                &mut heights,
                boundary,
                &plates,
                cells,
                neighbors,
                config
            );
        }
        
        // 应用地壳均衡
        apply_isostasy(&mut heights, neighbors, config.isostatic_rate);
        
        // 可选：进度回调
        if iteration % 10 == 0 {
            log::debug!("Tectonic simulation: {}%", iteration * 100 / config.iterations);
        }
    }
    
    // 后处理
    add_terrain_noise(&mut heights, cells, config.noise_strength, &mut rng);
    smooth_heights(&mut heights, neighbors, 2);
    
    heights
}

/// 应用边界效果
fn apply_boundary_effects(
    heights: &mut [f32],
    boundary: &PlateBoundary,
    plates: &[TectonicPlate],
    cells: &[Pos2],
    neighbors: &NeighborMap,
    config: &TectonicConfig
) {
    match boundary.boundary_type {
        BoundaryType::Convergent { intensity, subducting_plate } => {
            apply_convergent_effects(
                heights, boundary, plates, cells, neighbors,
                intensity, subducting_plate, config
            );
        }
        BoundaryType::Divergent { intensity } => {
            apply_divergent_effects(
                heights, boundary, cells, neighbors,
                intensity, config
            );
        }
        BoundaryType::Transform { intensity } => {
            apply_transform_effects(
                heights, boundary, cells, neighbors,
                intensity, config
            );
        }
    }
}

/// 汇聚边界效果
fn apply_convergent_effects(
    heights: &mut [f32],
    boundary: &PlateBoundary,
    plates: &[TectonicPlate],
    cells: &[Pos2],
    neighbors: &NeighborMap,
    intensity: f32,
    subducting_plate: Option<u16>,
    config: &TectonicConfig
) {
    let plate_a = &plates[boundary.plate_a as usize - 1];
    let plate_b = &plates[boundary.plate_b as usize - 1];
    
    for &cell in &boundary.cells {
        let cell = cell as usize;
        let cell_pos = cells[cell];
        
        // 计算到边界中心的距离，用于衰减
        for distance in 0..config.boundary_width as usize {
            let affected_cells = get_cells_at_distance(cell, distance, neighbors);
            let falloff = 1.0 - (distance as f32 / config.boundary_width);
            
            for affected in affected_cells {
                let affected = affected as usize;
                
                match subducting_plate {
                    Some(subducting_id) => {
                        // 俯冲带：一侧下沉（海沟），另一侧隆起（火山弧）
                        if plate_id_of(affected, plates) == subducting_id {
                            // 俯冲板块侧 - 海沟
                            heights[affected] -= config.subduction_depth_rate 
                                * intensity * falloff * 0.1;
                        } else {
                            // 覆盖板块侧 - 火山弧
                            heights[affected] += config.collision_uplift_rate 
                                * intensity * falloff * 0.1;
                        }
                    }
                    None => {
                        // 大陆-大陆碰撞：两侧都隆起（造山运动）
                        heights[affected] += config.collision_uplift_rate 
                            * intensity * falloff * 0.15;
                    }
                }
            }
        }
    }
}

/// 分离边界效果
fn apply_divergent_effects(
    heights: &mut [f32],
    boundary: &PlateBoundary,
    cells: &[Pos2],
    neighbors: &NeighborMap,
    intensity: f32,
    config: &TectonicConfig
) {
    for &cell in &boundary.cells {
        let cell = cell as usize;
        
        // 分离边界产生裂谷/洋脊
        for distance in 0..config.boundary_width as usize {
            let affected_cells = get_cells_at_distance(cell, distance, neighbors);
            let falloff = 1.0 - (distance as f32 / config.boundary_width);
            
            for affected in affected_cells {
                let affected = affected as usize;
                
                // 裂谷中心下沉，两侧略微隆起（洋脊肩部）
                if distance < 2 {
                    heights[affected] -= config.rift_depth_rate * intensity * 0.1;
                } else if distance < 5 {
                    // 洋脊肩部轻微隆起
                    heights[affected] += config.rift_depth_rate * intensity * falloff * 0.02;
                }
            }
        }
    }
}

/// 转换边界效果
fn apply_transform_effects(
    heights: &mut [f32],
    boundary: &PlateBoundary,
    cells: &[Pos2],
    neighbors: &NeighborMap,
    intensity: f32,
    config: &TectonicConfig
) {
    // 转换边界主要产生断层，高度变化较小
    for &cell in &boundary.cells {
        let cell = cell as usize;
        
        // 添加轻微的噪声扰动模拟断层地形
        heights[cell] += (rand::random::<f32>() - 0.5) * intensity * 0.05;
    }
}

/// 地壳均衡调整
/// 模拟地壳在地幔上的浮力平衡
fn apply_isostasy(heights: &mut [f32], neighbors: &NeighborMap, rate: f32) {
    let original = heights.to_vec();
    
    for i in 0..heights.len() {
        // 计算邻居平均高度
        let neighbor_avg: f32 = neighbors[i].iter()
            .map(|&n| original[n])
            .sum::<f32>() / neighbors[i].len() as f32;
        
        // 向平均值靠拢
        heights[i] += (neighbor_avg - heights[i]) * rate;
    }
}

/// 添加噪声细节
fn add_terrain_noise(
    heights: &mut [f32],
    cells: &[Pos2],
    strength: f32,
    rng: &mut impl Rng
) {
    let noise = Fbm::<Perlin>::new(rng.gen());
    
    for (i, pos) in cells.iter().enumerate() {
        let nx = pos.x * 0.01;
        let ny = pos.y * 0.01;
        let noise_val = noise.get([nx as f64, ny as f64]) as f32;
        heights[i] += noise_val * strength * 0.1;
    }
}

/// 平滑高度
fn smooth_heights(heights: &mut [f32], neighbors: &NeighborMap, iterations: u32) {
    for _ in 0..iterations {
        let original = heights.to_vec();
        for i in 0..heights.len() {
            let sum: f32 = neighbors[i].iter().map(|&n| original[n]).sum();
            let avg = sum / neighbors[i].len() as f32;
            heights[i] = heights[i] * 0.7 + avg * 0.3;
        }
    }
}
```

#### 1.3.6 转换为最终高度图

```rust
/// 将浮点高度转换为 u8 高度图
fn finalize_heightmap(heights: &[f32]) -> Vec<u8> {
    // 找到范围
    let min = heights.iter().copied().fold(f32::INFINITY, f32::min);
    let max = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let range = (max - min).max(0.001);
    
    heights.iter().map(|&h| {
        let normalized = (h - min) / range;
        (normalized * 255.0).clamp(0.0, 255.0) as u8
    }).collect()
}

/// 完整的板块构造高度图生成
pub fn generate_tectonic_heightmap(
    cells: &[Pos2],
    neighbors: &NeighborMap,
    config: &TectonicConfig
) -> Vec<u8> {
    let heights = simulate_tectonics(cells, neighbors, config);
    finalize_heightmap(&heights)
}
```

#### 1.3.7 性能优化

板块构造模拟计算量大，以下是优化策略：

```rust
/// 并行化的边界效果应用
fn apply_boundary_effects_parallel(
    heights: &mut [f32],
    boundaries: &[PlateBoundary],
    plates: &[TectonicPlate],
    cells: &[Pos2],
    neighbors: &NeighborMap,
    config: &TectonicConfig
) {
    // 收集所有需要更新的 (cell, delta) 对
    let updates: Vec<(usize, f32)> = boundaries.par_iter()
        .flat_map(|boundary| {
            calculate_boundary_updates(boundary, plates, cells, neighbors, config)
        })
        .collect();
    
    // 聚合同一单元格的更新
    let mut aggregated: HashMap<usize, f32> = HashMap::new();
    for (cell, delta) in updates {
        *aggregated.entry(cell).or_insert(0.0) += delta;
    }
    
    // 应用更新
    for (cell, delta) in aggregated {
        heights[cell] += delta;
    }
}

/// 使用空间索引加速邻域查询
fn get_cells_in_radius_fast(
    center: usize,
    radius: f32,
    spatial_index: &GridIndex,
    cells: &[Pos2]
) -> Vec<u32> {
    spatial_index.query_radius(cells, cells[center], radius)
}
```

#### 1.3.8 配置示例

```rust
// 类地球配置
let earth_like = TectonicConfig {
    plate_count: 15,
    continental_ratio: 0.3,
    iterations: 200,
    collision_uplift_rate: 0.6,
    subduction_depth_rate: 0.4,
    ..Default::default()
};

// 多山地配置
let mountainous = TectonicConfig {
    plate_count: 20,
    continental_ratio: 0.5,
    iterations: 300,
    collision_uplift_rate: 0.8,
    ..Default::default()
};

// 群岛配置
let archipelago = TectonicConfig {
    plate_count: 25,
    continental_ratio: 0.2,
    iterations: 150,
    rift_depth_rate: 0.3,
    ..Default::default()
};
```

---

### 1.4 侵蚀模拟（可选）

侵蚀可以使地形更加自然。

#### 1.3.1 热力侵蚀

模拟热胀冷缩导致的岩石碎裂。

```rust
fn thermal_erosion(heights: &mut [f32], iterations: u32, talus: f32) {
    for _ in 0..iterations {
        for i in 0..heights.len() {
            let neighbors = get_neighbors(i);
            for &n in &neighbors {
                let diff = heights[i] - heights[n];
                if diff > talus {
                    let transfer = (diff - talus) * 0.5;
                    heights[i] -= transfer;
                    heights[n] += transfer;
                }
            }
        }
    }
}
```

#### 1.3.2 水力侵蚀

模拟水流对地形的侵蚀作用。

```rust
fn hydraulic_erosion(heights: &mut [f32], config: &ErosionConfig) {
    for _ in 0..config.iterations {
        // 1. 随机放置水滴
        let mut drop = WaterDrop::new(random_position());
        
        for _ in 0..config.max_lifetime {
            // 2. 计算梯度
            let gradient = calculate_gradient(heights, drop.pos);
            
            // 3. 更新速度
            drop.velocity = drop.velocity * (1.0 - config.inertia) 
                          + gradient * config.inertia;
            
            // 4. 移动水滴
            let new_pos = drop.pos + drop.velocity.normalize();
            
            // 5. 计算高度差
            let height_diff = heights[new_pos] - heights[drop.pos];
            
            // 6. 侵蚀或沉积
            if height_diff > 0.0 {
                // 上坡：沉积
                let deposit = min(height_diff, drop.sediment);
                heights[drop.pos] += deposit;
                drop.sediment -= deposit;
            } else {
                // 下坡：侵蚀
                let erosion = min(-height_diff, drop.capacity - drop.sediment);
                heights[drop.pos] -= erosion * config.erosion_rate;
                drop.sediment += erosion;
            }
            
            // 7. 更新水滴
            drop.pos = new_pos;
            drop.water *= 1.0 - config.evaporation;
            
            if drop.water < config.min_water {
                break;
            }
        }
    }
}
```

---

## 二、海陆分析算法

### 2.1 海陆分离

```rust
fn classify_land_sea(heights: &[u8], sea_level: u8) -> Vec<bool> {
    heights.iter().map(|&h| h >= sea_level).collect()
}
```

### 2.2 连通分量检测

使用洪水填充算法识别独立的陆地和水体。

```rust
fn find_landmasses(is_land: &[bool], neighbors: &NeighborMap) -> Vec<Landmass> {
    let mut visited = vec![false; is_land.len()];
    let mut landmasses = Vec::new();
    
    for start in 0..is_land.len() {
        if visited[start] || !is_land[start] {
            continue;
        }
        
        // BFS 洪水填充
        let mut cells = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        visited[start] = true;
        
        while let Some(cell) = queue.pop_front() {
            cells.push(cell as u32);
            
            for &neighbor in &neighbors[cell] {
                if !visited[neighbor] && is_land[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        
        landmasses.push(Landmass {
            id: landmasses.len() as u16 + 1,
            cells,
            is_continent: cells.len() > CONTINENT_THRESHOLD,
        });
    }
    
    landmasses
}
```

### 2.3 海岸线提取

```rust
fn find_coastline_cells(is_land: &[bool], neighbors: &NeighborMap) -> Vec<u32> {
    (0..is_land.len())
        .filter(|&i| {
            is_land[i] && neighbors[i].iter().any(|&n| !is_land[n])
        })
        .map(|i| i as u32)
        .collect()
}

fn find_coastline_edges(is_land: &[bool], edges: &EdgeList) -> Vec<u32> {
    edges.iter()
        .enumerate()
        .filter(|(_, (a, b))| is_land[*a] != is_land[*b])
        .map(|(i, _)| i as u32)
        .collect()
}
```

---

## 三、水系生成算法

### 3.1 流向计算

每个单元格的水流向其最低的邻居。

```rust
fn compute_flow_direction(
    heights: &[u8], 
    is_land: &[bool],
    neighbors: &NeighborMap
) -> Vec<Option<u32>> {
    heights.iter().enumerate().map(|(i, &h)| {
        if !is_land[i] {
            return None;
        }
        
        neighbors[i].iter()
            .filter(|&&n| heights[n] < h || !is_land[n])
            .min_by_key(|&&n| heights[n])
            .copied()
            .map(|n| n as u32)
    }).collect()
}
```

### 3.2 流量累积

从高到低遍历，累积水流量。

```rust
fn compute_flux(
    heights: &[u8],
    is_land: &[bool],
    flow_direction: &[Option<u32>],
    precipitation: Option<&[u8]>
) -> Vec<u16> {
    // 按高度排序（从高到低）
    let mut sorted: Vec<usize> = (0..heights.len())
        .filter(|&i| is_land[i])
        .collect();
    sorted.sort_by(|&a, &b| heights[b].cmp(&heights[a]));
    
    // 初始流量（来自降水或固定值）
    let mut flux: Vec<u16> = match precipitation {
        Some(precip) => precip.iter().map(|&p| p as u16).collect(),
        None => vec![1; heights.len()],
    };
    
    // 累积流量
    for &cell in &sorted {
        if let Some(downstream) = flow_direction[cell] {
            flux[downstream as usize] = flux[downstream as usize]
                .saturating_add(flux[cell]);
        }
    }
    
    flux
}
```

### 3.3 河流路径提取

```rust
fn extract_rivers(
    flux: &[u16],
    flow_direction: &[Option<u32>],
    is_land: &[bool],
    threshold: u16
) -> Vec<River> {
    let mut visited = vec![false; flux.len()];
    let mut rivers = Vec::new();
    
    // 找到所有河口（流入海洋或湖泊的高流量点）
    let mouths: Vec<usize> = (0..flux.len())
        .filter(|&i| {
            flux[i] >= threshold 
            && is_land[i]
            && flow_direction[i].map_or(false, |d| !is_land[d as usize])
        })
        .collect();
    
    for &mouth in &mouths {
        let river = trace_river_upstream(mouth, flux, flow_direction, &mut visited, threshold);
        if !river.cells.is_empty() {
            rivers.push(river);
        }
    }
    
    rivers
}

fn trace_river_upstream(
    mouth: usize,
    flux: &[u16],
    flow_direction: &[Option<u32>],
    visited: &mut [bool],
    threshold: u16
) -> River {
    let mut cells = Vec::new();
    let mut current = mouth;
    
    // 从河口向上游追溯
    loop {
        if visited[current] {
            break;
        }
        visited[current] = true;
        cells.push(current as u32);
        
        // 找到流入当前单元格的最大流量上游
        let upstream: Option<usize> = find_upstream_with_max_flux(current, flux, flow_direction);
        
        match upstream {
            Some(up) if flux[up] >= threshold => current = up,
            _ => break,
        }
    }
    
    // 反转使其从源头到河口
    cells.reverse();
    
    River {
        cells,
        source_cell: *cells.first().unwrap_or(&0),
        mouth_cell: mouth as u32,
        ..Default::default()
    }
}
```

### 3.4 湖泊检测

湖泊形成于地形凹陷处。

```rust
fn detect_lakes(
    heights: &[u8],
    is_land: &[bool],
    neighbors: &NeighborMap
) -> Vec<Lake> {
    let mut lakes = Vec::new();
    let mut in_lake = vec![false; heights.len()];
    
    for start in 0..heights.len() {
        if !is_land[start] || in_lake[start] {
            continue;
        }
        
        // 检查是否为局部最低点
        let is_depression = neighbors[start].iter()
            .all(|&n| heights[n] >= heights[start]);
        
        if !is_depression {
            continue;
        }
        
        // 填充凹陷形成湖泊
        let lake = fill_depression(start, heights, neighbors);
        for &cell in &lake.cells {
            in_lake[cell as usize] = true;
        }
        lakes.push(lake);
    }
    
    lakes
}

fn fill_depression(
    start: usize,
    heights: &[u8],
    neighbors: &NeighborMap
) -> Lake {
    // 使用优先队列填充到溢出点
    let mut cells = Vec::new();
    let mut water_level = heights[start];
    let mut boundary = BinaryHeap::new();
    let mut in_lake = HashSet::new();
    
    boundary.push(Reverse((heights[start], start)));
    
    while let Some(Reverse((h, cell))) = boundary.pop() {
        if in_lake.contains(&cell) {
            continue;
        }
        
        if h > water_level && !in_lake.is_empty() {
            // 找到溢出点
            break;
        }
        
        in_lake.insert(cell);
        cells.push(cell as u32);
        water_level = water_level.max(h);
        
        for &neighbor in &neighbors[cell] {
            if !in_lake.contains(&neighbor) {
                boundary.push(Reverse((heights[neighbor], neighbor)));
            }
        }
    }
    
    Lake {
        cells,
        water_level,
        ..Default::default()
    }
}
```

### 3.5 河流宽度计算

```rust
fn calculate_river_widths(rivers: &mut [River], flux: &[u16]) {
    for river in rivers.iter_mut() {
        let mouth_flux = flux[river.mouth_cell as usize] as f32;
        
        // 河口宽度（基于流量的对数）
        river.width_km = (mouth_flux.ln() * 0.5).max(0.1);
        
        // 计算每个点的相对宽度
        river.widths = river.cells.iter().map(|&cell| {
            let cell_flux = flux[cell as usize] as f32;
            let relative = cell_flux / mouth_flux;
            (relative.sqrt() * river.width_km * 10.0) as u8
        }).collect();
    }
}
```

---

## 四、气候计算算法

### 4.1 温度计算

```rust
fn calculate_temperature(
    cells: &[Pos2],
    heights: &[u8],
    config: &ClimateConfig
) -> Vec<i8> {
    let map_height = config.map_height as f32;
    
    cells.iter().enumerate().map(|(i, pos)| {
        // 纬度因子 (0 = 赤道, 1 = 极地)
        let latitude = (pos.y / map_height - 0.5).abs() * 2.0;
        
        // 基础温度 (赤道 30°C, 极地 -30°C)
        let base_temp = 30.0 - latitude * 60.0;
        
        // 海拔修正 (每1000m降6.5°C)
        // height 20 = 海平面, 255 = 最高
        let altitude = (heights[i] as f32 - 20.0).max(0.0) / 235.0;
        let altitude_km = altitude * config.max_altitude_km;
        let altitude_effect = -altitude_km * 6.5;
        
        // 洋流效应（可选）
        let ocean_effect = config.ocean_current_map
            .map_or(0.0, |m| m[i] * 5.0);
        
        let temp = base_temp + altitude_effect + ocean_effect;
        temp.clamp(-128.0, 127.0) as i8
    }).collect()
}
```

### 4.2 降水计算

```rust
fn calculate_precipitation(
    cells: &[Pos2],
    heights: &[u8],
    is_land: &[bool],
    config: &ClimateConfig
) -> Vec<u8> {
    let wind_dir = Vec2::from_angle(config.wind_direction);
    
    // 1. 计算每个单元格到海洋的距离
    let distance_to_sea = compute_distance_to_sea(is_land, cells);
    
    // 2. 计算降水
    cells.iter().enumerate().map(|(i, pos)| {
        // 基础降水（沿海高，内陆低）
        let base = 200.0 - distance_to_sea[i] * 0.5;
        
        // 雨影效应
        // 检查上风方向是否有山脉
        let upwind_pos = *pos - wind_dir * 100.0;
        let upwind_height = sample_height_at(upwind_pos, heights, cells);
        let rain_shadow = if heights[i] < upwind_height {
            // 在山脉背风坡
            0.5  // 减少 50% 降水
        } else if heights[i] > upwind_height + 50 {
            // 在迎风坡
            1.5  // 增加 50% 降水
        } else {
            1.0
        };
        
        // 赤道附近降水更多
        let latitude_factor = 1.0 - (pos.y / config.map_height as f32 - 0.5).abs();
        
        let precip = base * rain_shadow * (0.5 + latitude_factor);
        (precip.clamp(0.0, 255.0)) as u8
    }).collect()
}
```

### 4.3 生物群落分配

使用 Whittaker 生物群落分类。

```rust
#[derive(Clone, Copy)]
pub enum Biome {
    IceCap = 1,
    Tundra = 2,
    Taiga = 3,
    TemperateGrassland = 4,
    TemperateDeciduousForest = 5,
    TemperateRainforest = 6,
    Desert = 7,
    TropicalSavanna = 8,
    TropicalSeasonalForest = 9,
    TropicalRainforest = 10,
    Wetland = 11,
    Mangrove = 12,
}

fn assign_biomes(
    temperature: &[i8],
    precipitation: &[u8],
    is_land: &[bool],
    is_coast: &[bool],
    flux: &[u16]
) -> Vec<u16> {
    temperature.iter().enumerate().map(|(i, &temp)| {
        if !is_land[i] {
            return 0; // 海洋
        }
        
        let precip = precipitation[i] as i32;
        let temp = temp as i32;
        
        // 特殊生物群落
        if flux[i] > 1000 {
            return Biome::Wetland as u16;
        }
        if is_coast[i] && temp > 20 && precip > 150 {
            return Biome::Mangrove as u16;
        }
        
        // 基于温度和降水的分类
        let biome = match temp {
            t if t < -10 => Biome::IceCap,
            t if t < 0 => {
                if precip < 25 { Biome::Tundra } else { Biome::Taiga }
            },
            t if t < 10 => {
                match precip {
                    p if p < 30 => Biome::TemperateGrassland,
                    p if p < 150 => Biome::TemperateDeciduousForest,
                    _ => Biome::Taiga,
                }
            },
            t if t < 20 => {
                match precip {
                    p if p < 30 => Biome::TemperateGrassland,
                    p if p < 100 => Biome::TemperateDeciduousForest,
                    _ => Biome::TemperateRainforest,
                }
            },
            _ => {
                match precip {
                    p if p < 25 => Biome::Desert,
                    p if p < 100 => Biome::TropicalSavanna,
                    p if p < 200 => Biome::TropicalSeasonalForest,
                    _ => Biome::TropicalRainforest,
                }
            }
        };
        
        biome as u16
    }).collect()
}
```

---

## 五、人口分布算法

### 5.1 适宜度评分

```rust
fn calculate_habitability(
    biome: &[u16],
    temperature: &[i8],
    precipitation: &[u8],
    rivers: &[River],
    coastline: &[u32]
) -> Vec<f32> {
    let mut habitability = vec![0.0; biome.len()];
    
    // 基于生物群落的基础适宜度
    let biome_base: HashMap<u16, f32> = hashmap! {
        0 => 0.0,   // 海洋
        1 => 0.0,   // 冰盖
        2 => 0.1,   // 苔原
        3 => 0.4,   // 针叶林
        4 => 0.7,   // 温带草原
        5 => 0.9,   // 温带森林
        6 => 0.8,   // 温带雨林
        7 => 0.1,   // 沙漠
        8 => 0.6,   // 热带草原
        9 => 0.8,   // 热带季雨林
        10 => 0.5,  // 热带雨林
        11 => 0.3,  // 湿地
        12 => 0.4,  // 红树林
    };
    
    for (i, &b) in biome.iter().enumerate() {
        habitability[i] = *biome_base.get(&b).unwrap_or(&0.0);
    }
    
    // 河流加成
    for river in rivers {
        for &cell in &river.cells {
            habitability[cell as usize] *= 1.5;
        }
    }
    
    // 海岸加成
    for &cell in coastline {
        habitability[cell as usize] *= 1.3;
    }
    
    // 极端温度惩罚
    for (i, &temp) in temperature.iter().enumerate() {
        if temp < -20 || temp > 40 {
            habitability[i] *= 0.1;
        }
    }
    
    habitability
}
```

### 5.2 人口分配

```rust
fn distribute_population(
    habitability: &[f32],
    is_land: &[bool],
    total_population: u64
) -> Vec<u32> {
    // 计算总适宜度
    let total_habitability: f64 = habitability.iter()
        .enumerate()
        .filter(|(i, _)| is_land[*i])
        .map(|(_, &h)| h as f64)
        .sum();
    
    // 按比例分配人口
    habitability.iter().enumerate().map(|(i, &h)| {
        if !is_land[i] || total_habitability == 0.0 {
            return 0;
        }
        
        let ratio = h as f64 / total_habitability;
        (ratio * total_population as f64) as u32
    }).collect()
}
```

---

## 六、文化区域生成

### 6.1 文化种子放置

```rust
fn place_culture_origins(
    population: &[u32],
    biome: &[u16],
    is_land: &[bool],
    culture_count: u32
) -> Vec<CultureOrigin> {
    // 选择人口密集的单元格作为候选
    let mut candidates: Vec<(usize, u32)> = population.iter()
        .enumerate()
        .filter(|(i, _)| is_land[*i])
        .map(|(i, &p)| (i, p))
        .collect();
    
    // 按人口排序
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    
    // 选择分散的起源点
    let mut origins = Vec::new();
    let min_distance = (population.len() as f32).sqrt() * 2.0;
    
    for (cell, _) in candidates {
        // 检查与现有起源点的距离
        let too_close = origins.iter().any(|origin: &CultureOrigin| {
            cell_distance(cell, origin.cell) < min_distance
        });
        
        if !too_close {
            let culture_type = determine_culture_type(cell, biome);
            origins.push(CultureOrigin {
                cell: cell as u32,
                culture_type,
            });
            
            if origins.len() >= culture_count as usize {
                break;
            }
        }
    }
    
    origins
}

fn determine_culture_type(cell: usize, biome: &[u16]) -> CultureType {
    match biome[cell] {
        4 | 8 => CultureType::Nomadic,       // 草原
        5 | 9 | 10 => CultureType::Agricultural,  // 森林
        7 => CultureType::Desert,             // 沙漠
        2 | 3 => CultureType::Hunting,        // 苔原/针叶林
        _ => CultureType::Agricultural,
    }
}
```

### 6.2 文化扩张

```rust
fn expand_cultures(
    origins: &[CultureOrigin],
    is_land: &[bool],
    heights: &[u8],
    rivers: &[River],
    neighbors: &NeighborMap
) -> Vec<u16> {
    let mut culture = vec![0u16; is_land.len()];
    let mut cost = vec![f32::INFINITY; is_land.len()];
    let mut heap = BinaryHeap::new();
    
    // 初始化
    for (i, origin) in origins.iter().enumerate() {
        let id = (i + 1) as u16;
        culture[origin.cell as usize] = id;
        cost[origin.cell as usize] = 0.0;
        heap.push(Reverse((OrderedFloat(0.0), origin.cell, id)));
    }
    
    // Dijkstra 风格扩张
    while let Some(Reverse((OrderedFloat(c), cell, culture_id))) = heap.pop() {
        let cell = cell as usize;
        
        if c > cost[cell] {
            continue;
        }
        
        for &neighbor in &neighbors[cell] {
            if !is_land[neighbor] {
                continue;
            }
            
            // 计算扩张成本
            let expansion_cost = calculate_expansion_cost(
                cell, neighbor, heights, rivers
            );
            
            let new_cost = c + expansion_cost;
            
            if new_cost < cost[neighbor] {
                cost[neighbor] = new_cost;
                culture[neighbor] = culture_id;
                heap.push(Reverse((OrderedFloat(new_cost), neighbor as u32, culture_id)));
            }
        }
    }
    
    culture
}

fn calculate_expansion_cost(
    from: usize,
    to: usize,
    heights: &[u8],
    rivers: &[River]
) -> f32 {
    let mut cost = 1.0;
    
    // 高度差惩罚（山脉阻隔）
    let height_diff = (heights[to] as i32 - heights[from] as i32).abs();
    if height_diff > 30 {
        cost += height_diff as f32 * 0.1;
    }
    
    // 大河惩罚
    if is_major_river_between(from, to, rivers) {
        cost += 5.0;
    }
    
    cost
}
```

---

## 七、国家生成算法

### 7.1 首都选址

```rust
fn select_capitals(
    population: &[u32],
    culture: &[u16],
    rivers: &[River],
    coastline: &[u32],
    harbor_scores: &[u8],
    state_count: u32
) -> Vec<u32> {
    // 计算每个单元格的首都适宜度
    let mut scores: Vec<(usize, f32)> = population.iter()
        .enumerate()
        .filter(|(_, &p)| p > 0)
        .map(|(i, &pop)| {
            let mut score = pop as f32;
            
            // 河流加成
            if is_on_river(i, rivers) {
                score *= 1.5;
            }
            
            // 港口加成
            score *= 1.0 + harbor_scores[i] as f32 * 0.2;
            
            // 河流交汇处加成
            if is_river_confluence(i, rivers) {
                score *= 2.0;
            }
            
            (i, score)
        })
        .collect();
    
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    // 选择分散的首都
    let mut capitals = Vec::new();
    let min_distance = (population.len() as f32).sqrt() * 3.0;
    
    for (cell, _) in scores {
        let too_close = capitals.iter().any(|&c: &u32| {
            cell_distance(cell, c as usize) < min_distance
        });
        
        if !too_close {
            capitals.push(cell as u32);
            if capitals.len() >= state_count as usize {
                break;
            }
        }
    }
    
    capitals
}
```

### 7.2 国家扩张

```rust
fn expand_states(
    capitals: &[u32],
    culture: &[u16],
    population: &[u32],
    heights: &[u8],
    is_land: &[bool],
    neighbors: &NeighborMap
) -> Vec<u16> {
    let mut state = vec![0u16; culture.len()];
    let mut priority = vec![f32::INFINITY; culture.len()];
    let mut heap = BinaryHeap::new();
    
    // 初始化首都
    for (i, &capital) in capitals.iter().enumerate() {
        let id = (i + 1) as u16;
        state[capital as usize] = id;
        priority[capital as usize] = 0.0;
        
        // 获取首都文化
        let capital_culture = culture[capital as usize];
        
        heap.push(ExpansionCandidate {
            cell: capital,
            state_id: id,
            priority: 0.0,
            origin_culture: capital_culture,
        });
    }
    
    // 扩张
    while let Some(candidate) = heap.pop() {
        let cell = candidate.cell as usize;
        
        if state[cell] != 0 && state[cell] != candidate.state_id {
            continue; // 已被其他国家占领
        }
        
        for &neighbor in &neighbors[cell] {
            if !is_land[neighbor] || state[neighbor] != 0 {
                continue;
            }
            
            // 计算扩张成本
            let cost = calculate_state_expansion_cost(
                cell, neighbor, 
                candidate.origin_culture, 
                culture, heights, population
            );
            
            let new_priority = candidate.priority + cost;
            
            if new_priority < priority[neighbor] {
                priority[neighbor] = new_priority;
                state[neighbor] = candidate.state_id;
                
                heap.push(ExpansionCandidate {
                    cell: neighbor as u32,
                    state_id: candidate.state_id,
                    priority: new_priority,
                    origin_culture: candidate.origin_culture,
                });
            }
        }
    }
    
    state
}

fn calculate_state_expansion_cost(
    from: usize,
    to: usize,
    origin_culture: u16,
    culture: &[u16],
    heights: &[u8],
    population: &[u32]
) -> f32 {
    let mut cost = 1.0;
    
    // 距离成本
    cost += 0.1;
    
    // 文化差异惩罚
    if culture[to] != origin_culture {
        cost += 3.0;
    }
    
    // 地形惩罚
    let height_diff = (heights[to] as i32 - heights[from] as i32).abs();
    cost += height_diff as f32 * 0.05;
    
    // 山脉惩罚
    if heights[to] > 200 {
        cost += 5.0;
    }
    
    // 人口稀少区域惩罚
    if population[to] < 100 {
        cost += 2.0;
    }
    
    cost
}
```

### 7.3 边界优化

```rust
fn optimize_state_borders(
    state: &mut [u16],
    neighbors: &NeighborMap,
    iterations: u32
) {
    for _ in 0..iterations {
        let mut changes = Vec::new();
        
        for cell in 0..state.len() {
            if state[cell] == 0 {
                continue;
            }
            
            // 统计邻居的国家分布
            let mut neighbor_states: HashMap<u16, u32> = HashMap::new();
            for &n in &neighbors[cell] {
                if state[n] != 0 {
                    *neighbor_states.entry(state[n]).or_insert(0) += 1;
                }
            }
            
            // 如果大多数邻居属于其他国家，可能需要变更
            let current_count = *neighbor_states.get(&state[cell]).unwrap_or(&0);
            for (&s, &count) in &neighbor_states {
                if s != state[cell] && count > current_count * 2 {
                    changes.push((cell, s));
                    break;
                }
            }
        }
        
        // 应用变更
        for (cell, new_state) in changes {
            state[cell] = new_state;
        }
    }
}

fn remove_enclaves(
    state: &mut [u16],
    neighbors: &NeighborMap,
    min_enclave_size: usize
) {
    // 找到所有飞地
    for state_id in 1..=max_state_id(state) {
        let cells: Vec<usize> = state.iter()
            .enumerate()
            .filter(|(_, &s)| s == state_id)
            .map(|(i, _)| i)
            .collect();
        
        // 找到连通分量
        let components = find_connected_components(&cells, neighbors);
        
        // 保留最大的分量，其他的合并到邻国
        if let Some((main_component, rest)) = split_largest(&components) {
            for component in rest {
                if component.len() < min_enclave_size {
                    // 将飞地合并到周围最常见的国家
                    let surrounding_state = find_surrounding_state(&component, state, neighbors);
                    for cell in component {
                        state[cell] = surrounding_state;
                    }
                }
            }
        }
    }
}
```

---

## 八、城镇放置算法

### 8.1 城镇选址评分

```rust
fn score_burg_location(
    cell: usize,
    population: &[u32],
    rivers: &[River],
    coastline: &[u32],
    heights: &[u8],
    state: &[u16],
    existing_burgs: &[Burg]
) -> f32 {
    let mut score = population[cell] as f32;
    
    // 河流加成
    if is_on_river(cell, rivers) {
        score *= 1.5;
    }
    
    // 河流交汇加成
    if is_river_confluence(cell, rivers) {
        score *= 2.0;
    }
    
    // 港口加成
    if coastline.contains(&(cell as u32)) {
        let harbor_quality = calculate_harbor_quality(cell, heights);
        score *= 1.0 + harbor_quality;
    }
    
    // 平原加成（更适合建城）
    if heights[cell] < 50 {
        score *= 1.2;
    }
    
    // 距离现有城市的惩罚（避免太密集）
    for burg in existing_burgs {
        let distance = cell_distance(cell, burg.cell as usize);
        if distance < 10.0 {
            score *= distance / 10.0;
        }
    }
    
    score
}
```

### 8.2 城市放置

```rust
fn place_burgs(
    state: &[u16],
    population: &[u32],
    rivers: &[River],
    coastline: &[u32],
    heights: &[u8],
    config: &BurgConfig
) -> Vec<Burg> {
    let mut burgs = Vec::new();
    let states: HashSet<u16> = state.iter().cloned().filter(|&s| s != 0).collect();
    
    // 为每个国家放置首都
    for &state_id in &states {
        let cells: Vec<usize> = state.iter()
            .enumerate()
            .filter(|(_, &s)| s == state_id)
            .map(|(i, _)| i)
            .collect();
        
        // 选择最佳首都位置
        let capital = cells.iter()
            .max_by(|&&a, &&b| {
                let score_a = score_burg_location(a, population, rivers, coastline, heights, state, &burgs);
                let score_b = score_burg_location(b, population, rivers, coastline, heights, state, &burgs);
                score_a.partial_cmp(&score_b).unwrap()
            })
            .copied()
            .unwrap();
        
        burgs.push(Burg {
            id: burgs.len() as u16 + 1,
            cell: capital as u32,
            state: state_id,
            is_capital: true,
            population: calculate_burg_population(capital, population, 1.0),
            ..Default::default()
        });
    }
    
    // 放置其他城市
    let mut candidates: Vec<(usize, f32)> = population.iter()
        .enumerate()
        .filter(|(i, &p)| p > 0 && state[*i] != 0)
        .map(|(i, _)| {
            let score = score_burg_location(i, population, rivers, coastline, heights, state, &burgs);
            (i, score)
        })
        .collect();
    
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    for (cell, _) in candidates {
        if burgs.len() >= config.max_burgs as usize {
            break;
        }
        
        // 检查距离约束
        let min_distance = 5.0; // 最小城市间距
        let too_close = burgs.iter().any(|b| {
            cell_distance(cell, b.cell as usize) < min_distance
        });
        
        if !too_close {
            let pop = calculate_burg_population(cell, population, 0.5);
            let is_port = coastline.contains(&(cell as u32));
            
            burgs.push(Burg {
                id: burgs.len() as u16 + 1,
                cell: cell as u32,
                state: state[cell],
                is_capital: false,
                is_port,
                population: pop,
                ..Default::default()
            });
        }
    }
    
    burgs
}
```

---

## 九、道路生成算法

### 9.1 道路网络生成

```rust
fn generate_routes(
    burgs: &[Burg],
    heights: &[u8],
    rivers: &[River],
    is_land: &[bool],
    neighbors: &NeighborMap
) -> Vec<Route> {
    let mut routes = Vec::new();
    
    // 按重要性排序城市
    let mut sorted_burgs = burgs.to_vec();
    sorted_burgs.sort_by(|a, b| b.population.cmp(&a.population));
    
    // 首都之间的主干道
    let capitals: Vec<&Burg> = sorted_burgs.iter()
        .filter(|b| b.is_capital)
        .collect();
    
    for i in 0..capitals.len() {
        for j in (i + 1)..capitals.len() {
            if let Some(path) = find_route_path(
                capitals[i].cell, 
                capitals[j].cell,
                heights, rivers, is_land, neighbors,
                RouteType::Highway
            ) {
                routes.push(Route {
                    id: routes.len() as u16 + 1,
                    route_type: RouteType::Highway,
                    from_burg: capitals[i].id,
                    to_burg: capitals[j].id,
                    path,
                });
            }
        }
    }
    
    // 城市到首都的道路
    for burg in &sorted_burgs {
        if burg.is_capital {
            continue;
        }
        
        // 找到同国首都
        if let Some(capital) = capitals.iter().find(|c| c.state == burg.state) {
            if let Some(path) = find_route_path(
                burg.cell, capital.cell,
                heights, rivers, is_land, neighbors,
                RouteType::Road
            ) {
                routes.push(Route {
                    id: routes.len() as u16 + 1,
                    route_type: RouteType::Road,
                    from_burg: burg.id,
                    to_burg: capital.id,
                    path,
                });
            }
        }
    }
    
    routes
}
```

### 9.2 A* 路径查找

```rust
fn find_route_path(
    from: u32,
    to: u32,
    heights: &[u8],
    rivers: &[River],
    is_land: &[bool],
    neighbors: &NeighborMap,
    route_type: RouteType
) -> Option<Vec<u32>> {
    let from = from as usize;
    let to = to as usize;
    
    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<usize, usize> = HashMap::new();
    let mut g_score = vec![f32::INFINITY; heights.len()];
    
    g_score[from] = 0.0;
    open_set.push(Reverse((OrderedFloat(heuristic(from, to)), from)));
    
    while let Some(Reverse((_, current))) = open_set.pop() {
        if current == to {
            return Some(reconstruct_path(&came_from, current));
        }
        
        for &neighbor in &neighbors[current] {
            if !is_land[neighbor] {
                continue;
            }
            
            let movement_cost = calculate_movement_cost(
                current, neighbor, heights, rivers, route_type
            );
            
            let tentative_g = g_score[current] + movement_cost;
            
            if tentative_g < g_score[neighbor] {
                came_from.insert(neighbor, current);
                g_score[neighbor] = tentative_g;
                let f_score = tentative_g + heuristic(neighbor, to);
                open_set.push(Reverse((OrderedFloat(f_score), neighbor)));
            }
        }
    }
    
    None
}

fn calculate_movement_cost(
    from: usize,
    to: usize,
    heights: &[u8],
    rivers: &[River],
    route_type: RouteType
) -> f32 {
    let mut cost = 1.0;
    
    // 高度差成本
    let height_diff = (heights[to] as i32 - heights[from] as i32).abs();
    cost += height_diff as f32 * 0.1;
    
    // 山地惩罚
    if heights[to] > 150 {
        cost += 5.0;
    }
    
    // 河流穿越成本（需要桥）
    if crosses_river(from, to, rivers) {
        cost += 3.0;
    }
    
    // 主干道优先使用现有道路
    if route_type == RouteType::Highway {
        // TODO: 检查是否有现有道路
    }
    
    cost
}
```

---

## 十、名称生成算法

### 10.1 马尔可夫链生成器

```rust
pub struct NameGenerator {
    chains: HashMap<CultureType, MarkovChain>,
}

struct MarkovChain {
    order: usize,
    transitions: HashMap<String, Vec<(char, f32)>>,
}

impl NameGenerator {
    pub fn generate(&self, culture_type: CultureType, min_len: usize, max_len: usize) -> String {
        let chain = &self.chains[&culture_type];
        let mut name = String::new();
        let mut state = String::new();
        
        // 选择起始状态
        state = chain.random_start();
        name.push_str(&state);
        
        while name.len() < max_len {
            if let Some(next) = chain.next_char(&state) {
                if next == '\0' && name.len() >= min_len {
                    break;
                }
                name.push(next);
                state = state[1..].to_string() + &next.to_string();
            } else {
                break;
            }
        }
        
        // 首字母大写
        let mut chars = name.chars();
        match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().chain(chars).collect(),
        }
    }
}

impl MarkovChain {
    pub fn train(names: &[&str], order: usize) -> Self {
        let mut transitions: HashMap<String, Vec<(char, u32)>> = HashMap::new();
        
        for name in names {
            let padded = format!("{}{}{}", 
                "^".repeat(order), 
                name.to_lowercase(), 
                "\0"
            );
            
            for i in 0..padded.len() - order {
                let state: String = padded[i..i + order].to_string();
                let next = padded.chars().nth(i + order).unwrap();
                
                let entry = transitions.entry(state).or_default();
                if let Some(pos) = entry.iter().position(|(c, _)| *c == next) {
                    entry[pos].1 += 1;
                } else {
                    entry.push((next, 1));
                }
            }
        }
        
        // 转换为概率
        let transitions = transitions.into_iter().map(|(k, v)| {
            let total: u32 = v.iter().map(|(_, c)| c).sum();
            let probs: Vec<(char, f32)> = v.into_iter()
                .map(|(c, count)| (c, count as f32 / total as f32))
                .collect();
            (k, probs)
        }).collect();
        
        Self { order, transitions }
    }
}
```

### 10.2 基于文化的命名

```rust
fn generate_names_for_features(
    map: &mut MapSystem,
    name_gen: &NameGenerator
) {
    // 国家名称
    for state in &mut map.states {
        let culture_type = map.cultures[state.culture as usize - 1].culture_type;
        state.name = name_gen.generate(culture_type, 4, 12);
    }
    
    // 城市名称
    for burg in &mut map.burgs {
        let culture_id = map.cells_data.culture[burg.cell as usize];
        let culture_type = map.cultures[culture_id as usize - 1].culture_type;
        
        // 首都加后缀
        let base = name_gen.generate(culture_type, 3, 8);
        burg.name = if burg.is_capital {
            format!("{} City", base)
        } else if burg.is_port {
            format!("{} Port", base)
        } else {
            base
        };
    }
    
    // 河流名称
    for river in &mut map.rivers {
        let mouth_culture = map.cells_data.culture[river.mouth_cell as usize];
        let culture_type = map.cultures[mouth_culture as usize - 1].culture_type;
        let base = name_gen.generate(culture_type, 3, 8);
        river.name = format!("{} River", base);
    }
}
```

---

## 十一、性能优化技巧

### 11.1 并行处理

```rust
// 使用 rayon 并行处理独立单元格
use rayon::prelude::*;

let heights: Vec<u8> = cells.par_iter()
    .map(|pos| calculate_height(pos, &noise_config))
    .collect();
```

### 11.2 空间局部性

```rust
// 按空间位置排序以提高缓存命中率
let mut sorted_indices: Vec<usize> = (0..cells.len()).collect();
sorted_indices.sort_by(|&a, &b| {
    let za = morton_code(cells[a]);
    let zb = morton_code(cells[b]);
    za.cmp(&zb)
});
```

### 11.3 增量更新

```rust
// 只更新被修改的区域
fn update_affected_region(
    center: usize,
    radius: f32,
    map: &mut MapSystem
) {
    let affected = map.point_index.query_radius(&map.grid.points, center, radius);
    
    for cell in affected {
        recalculate_cell_properties(cell, map);
    }
}
```

---

*文档版本: 1.0*
*最后更新: 2026-01-12*

