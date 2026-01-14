// 第三章：算法文档

#import "@preview/cetz:0.4.2"

= 算法文档

本章详细描述地图生成器中使用的各种算法。

== 地形生成算法

=== 核心设计理念

本系统采用*"板块构造主导 + 噪声细节叠加"*的分层地形生成策略，这是基于真实地质学原理的科学方法：

*地质学基础*：
- 地球表面的大尺度地形（山脉、海沟、高原）由*板块运动*形成
- 中小尺度地形（褶皱、河谷、沟壑）由*侵蚀、沉积、局部构造*形成
- 这是一个*多尺度、层次化*的过程

*算法策略*：

#figure(
  table(
    columns: (auto, auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*层级*], [*空间尺度*], [*地质过程*], [*实现方法*],
    [宏观], [1000+ km], [板块碰撞、俯冲、张裂], [*板块构造模拟*（主算法）],
    [中观], [100-1000 km], [区域褶皱、火山群、盆地], [中频噪声叠加],
    [微观], [1-100 km], [侵蚀沟壑、冲积扇、沙丘], [高频噪声 + 可选物理侵蚀],
  ),
  caption: [多尺度地形生成策略]
)

*设计优势*：
- ✓ *科学真实*：符合板块构造理论，生成的地形符合地质学规律
- ✓ *性能平衡*：避免完整物理模拟的高昂成本，实时生成大规模地图
- ✓ *高度可控*：板块参数控制大格局，噪声参数控制细节，互不干扰
- ✓ *层次分明*：先宏观后微观，符合地质形成的自然时序

*完整生成流程*：

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let width = 14
    let y = 0

    // 主容器
    rect((0, y), (width, y - 20), stroke: (thickness: 1pt), fill: rgb("#f9f9f9"))
    content((width/2, y - 0.5), text(weight: "bold", size: 11pt, "分层地形生成完整流程"))

    // 阶段 1: 板块构造（主导）
    let y = y - 1.2
    rect((0.5, y), (width - 0.5, y - 3.5), stroke: (thickness: 1pt, paint: rgb("#d32f2f")), fill: rgb("#ffebee"))
    content((1, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, fill: rgb("#d32f2f"), "阶段 1: 板块构造模拟（主导机制）"))
    content((1, y - 0.9), anchor: "west", text(size: 7.5pt, "• 生成 12-15 个板块"))
    content((1, y - 1.3), anchor: "west", text(size: 7.5pt, "• 分配运动向量"))
    content((1, y - 1.7), anchor: "west", text(size: 7.5pt, "• 迭代 100-300 次模拟板块相互作用"))
    content((1, y - 2.1), anchor: "west", text(size: 7.5pt, "• 汇聚边界 → 山脉/海沟，分离边界 → 裂谷"))
    content((1, y - 2.5), anchor: "west", text(size: 7.5pt, "• 地壳均衡调整"))
    content((11.5, y - 1.75), text(size: 7pt, style: "italic", fill: gray, "决定大陆、山脉、#linebreak()海沟等宏观格局"))

    // 箭头
    let y = y - 3.7
    line((width/2, y), (width/2, y - 0.5), mark: (end: "stealth"), stroke: (thickness: 1.2pt))

    // 阶段 2: 中尺度噪声
    let y = y - 0.7
    rect((0.5, y), (width - 0.5, y - 2.5), stroke: (thickness: 1pt, paint: rgb("#1976d2")), fill: rgb("#e3f2fd"))
    content((1, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, fill: rgb("#1976d2"), "阶段 2: 中尺度噪声（区域构造）"))
    content((1, y - 0.9), anchor: "west", text(size: 7.5pt, "• 低频噪声（3 octaves，频率 0.01）"))
    content((1, y - 1.3), anchor: "west", text(size: 7.5pt, "• 大陆板块内部噪声强 (0.3)，海洋板块弱 (0.1)"))
    content((1, y - 1.7), anchor: "west", text(size: 7.5pt, "• 板块边界附近抑制噪声"))
    content((11.5, y - 1.25), text(size: 7pt, style: "italic", fill: gray, "添加褶皱、#linebreak()盆地等中尺度地貌"))

    // 箭头
    let y = y - 2.7
    line((width/2, y), (width/2, y - 0.5), mark: (end: "stealth"), stroke: (thickness: 1.2pt))

    // 阶段 3: 侵蚀（可选）
    let y = y - 0.7
    rect((0.5, y), (width - 0.5, y - 2), stroke: (thickness: 1pt, paint: rgb("#388e3c"), dash: "dashed"), fill: rgb("#e8f5e9"))
    content((1, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, fill: rgb("#388e3c"), "阶段 3: 侵蚀模拟（可选，高质量模式）"))
    content((1, y - 0.9), anchor: "west", text(size: 7.5pt, "• 热力侵蚀（模拟岩石碎裂）"))
    content((1, y - 1.3), anchor: "west", text(size: 7.5pt, "• 水力侵蚀（模拟水流侵蚀）"))
    content((11.5, y - 1), text(size: 7pt, style: "italic", fill: gray, "增强真实感，#linebreak()但计算量大"))

    // 箭头
    let y = y - 2.2
    line((width/2, y), (width/2, y - 0.5), mark: (end: "stealth"), stroke: (thickness: 1.2pt))

    // 阶段 4: 小尺度噪声
    let y = y - 0.7
    rect((0.5, y), (width - 0.5, y - 2.2), stroke: (thickness: 1pt, paint: rgb("#7b1fa2")), fill: rgb("#f3e5f5"))
    content((1, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, fill: rgb("#7b1fa2"), "阶段 4: 小尺度噪声（表面细节）"))
    content((1, y - 0.9), anchor: "west", text(size: 7.5pt, "• 高频噪声（5 octaves，频率 0.05）"))
    content((1, y - 1.3), anchor: "west", text(size: 7.5pt, "• 高海拔区域侵蚀增强"))
    content((11.5, y - 1.1), text(size: 7pt, style: "italic", fill: gray, "添加沟壑、#linebreak()小山丘等细节"))

    // 箭头
    let y = y - 2.4
    line((width/2, y), (width/2, y - 0.5), mark: (end: "stealth"), stroke: (thickness: 1.2pt))

    // 阶段 5: 后处理
    let y = y - 0.7
    rect((0.5, y), (width - 0.5, y - 1.8), stroke: (thickness: 1pt, paint: rgb("#f57c00")), fill: rgb("#fff3e0"))
    content((1, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, fill: rgb("#f57c00"), "阶段 5: 归一化与后处理"))
    content((1, y - 0.9), anchor: "west", text(size: 7.5pt, "• 归一化到 [0, 255]"))
    content((1, y - 1.3), anchor: "west", text(size: 7.5pt, "• 可选平滑处理"))

    // 输出
    let y = y - 2
    rect((width/2 - 2.5, y), (width/2 + 2.5, y - 0.8), stroke: (thickness: 1.5pt), fill: rgb("#4caf50"))
    content((width/2, y - 0.4), text(weight: "bold", size: 9pt, fill: white, "输出: 真实感高度图"))
  }),
  caption: [分层地形生成完整流程]
)

=== 噪声函数基础

==== Perlin/Simplex 噪声

噪声函数是程序化地形生成的基础。

*特点*:
- 连续性：相邻点的值变化平滑
- 可重复性：相同输入产生相同输出
- 伪随机性：看起来随机但可控

*多层叠加 (Fractal Brownian Motion)*:

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

*参数说明*:

#figure(
  table(
    columns: (auto, 1fr, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*参数*], [*说明*], [*典型值*],
    [octaves], [叠加层数], [4-8],
    [persistence], [振幅衰减], [0.5],
    [lacunarity], [频率倍增], [2.0],
    [base_frequency], [基础频率], [0.01-0.05],
  ),
  caption: [噪声参数说明]
)

=== 高度图生成策略

==== 纯噪声模式

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
        let adjusted = height + config.land_bias;
        
        // 归一化到 0-255
        ((adjusted + 1.0) / 2.0 * 255.0).clamp(0.0, 255.0) as u8
    }).collect()
}
```

==== 模板引导模式

使用预定义模板控制大陆形状。

#figure(
  table(
    columns: (auto, 1fr, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*模板*], [*说明*], [*公式*],
    [椭圆大陆], [中心高，边缘低], [`1 - distance_to_center`],
    [群岛], [多个高点], [`max(island1, island2, ...)`],
    [半球], [一侧大陆一侧海洋], [`x > 0.5 ? 1 : 0`],
    [边缘海洋], [边缘必须是海], [`smoothstep(edge_distance)`],
  ),
  caption: [高度图模板]
)

=== 板块构造模拟（推荐）

板块构造是地球地形形成的根本机制。通过模拟板块运动，可以生成最真实的大陆、山脉和海沟分布。

==== 设计原理

*多尺度地形生成策略*：

本算法采用"板块构造主导 + 噪声细节叠加"的分层生成策略，符合真实地质过程的多尺度特征：

#figure(
  table(
    columns: (auto, auto, 1fr, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*尺度*], [*机制*], [*现实对应*], [*实现方法*],
    [大尺度], [板块构造], [山脉、海沟、高原], [板块模拟（主算法）],
    [中尺度], [区域构造], [褶皱、断裂、火山], [中频噪声 + 侵蚀],
    [小尺度], [表面过程], [侵蚀沟壑、沉积], [高频噪声],
  ),
  caption: [地形生成的多尺度策略]
)

*为什么这样设计*：

- *符合地质学原理*：现实中，板块运动决定大格局（数千公里尺度），局部过程产生细节（数公里尺度）
- *性能与真实性平衡*：板块模拟处理宏观结构，噪声快速生成微观细节，避免完整物理模拟的高昂成本
- *可控性强*：板块参数控制大陆分布，噪声参数控制地表粗糙度，两者解耦便于调整

==== 算法概述

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let width = 14
    let y = 0

    // Main container
    rect((0, y), (width, y - 14), stroke: (thickness: 1pt), fill: rgb("#f5f5f5"))
    content((width/2, y - 0.4), text(weight: "bold", size: 10pt, "板块构造模拟流程"))

    // Step 1: Plate Generation
    let y = y - 1.0
    content((0.5, y - 0.3), anchor: "west", text(weight: "bold", size: 8pt, "Step 1: 板块生成"))
    let y = y - 0.6
    rect((0.5, y), (width - 0.5, y - 1.5), stroke: (thickness: 0.6pt), fill: rgb("#d4edda"))
    content((1, y - 0.3), anchor: "west", text(size: 7pt, "• 在地图上随机放置 N 个板块种子点"))
    content((1, y - 0.7), anchor: "west", text(size: 7pt, "• 使用 Voronoi 图划分板块区域"))
    content((1, y - 1.1), anchor: "west", text(size: 7pt, "• 为每个板块分配类型（大陆板块/海洋板块）"))

    // Arrow
    let y = y - 1.7
    line((width/2, y), (width/2, y - 0.4), mark: (end: "stealth"))

    // Step 2: Motion Vector Assignment
    let y = y - 0.6
    content((0.5, y - 0.3), anchor: "west", text(weight: "bold", size: 8pt, "Step 2: 运动向量分配"))
    let y = y - 0.6
    rect((0.5, y), (width - 0.5, y - 1.3), stroke: (thickness: 0.6pt), fill: rgb("#a8d5e2"))
    content((1, y - 0.3), anchor: "west", text(size: 7pt, "• 为每个板块分配运动方向（角度）"))
    content((1, y - 0.7), anchor: "west", text(size: 7pt, "• 为每个板块分配运动速度"))
    content((1, y - 1.0), anchor: "west", text(size: 7pt, "• 可选：考虑板块旋转"))

    // Arrow
    let y = y - 1.5
    line((width/2, y), (width/2, y - 0.4), mark: (end: "stealth"))

    // Step 3: Boundary Analysis
    let y = y - 0.6
    content((0.5, y - 0.3), anchor: "west", text(weight: "bold", size: 8pt, "Step 3: 边界分析（每次迭代）"))
    let y = y - 0.6
    rect((0.5, y), (width - 0.5, y - 1.8), stroke: (thickness: 0.6pt), fill: rgb("#fff3cd"))
    content((1, y - 0.3), anchor: "west", text(size: 7pt, "• 计算相邻板块的相对运动"))
    content((1, y - 0.6), anchor: "west", text(size: 7pt, "• 判断边界类型："))
    content((1.5, y - 0.9), anchor: "west", text(size: 6.5pt, "- 汇聚边界（碰撞）→ 造山/俯冲"))
    content((1.5, y - 1.2), anchor: "west", text(size: 6.5pt, "- 分离边界（张裂）→ 裂谷/洋脊"))
    content((1.5, y - 1.5), anchor: "west", text(size: 6.5pt, "- 转换边界（错动）→ 断层"))

    // Arrow
    let y = y - 2.0
    line((width/2, y), (width/2, y - 0.4), mark: (end: "stealth"))

    // Step 4: Height Update
    let y = y - 0.6
    content((0.5, y - 0.3), anchor: "west", text(weight: "bold", size: 8pt, "Step 4: 高度更新"))
    let y = y - 0.6
    rect((0.5, y), (width - 0.5, y - 1.5), stroke: (thickness: 0.6pt), fill: rgb("#f8d7da"))
    content((1, y - 0.3), anchor: "west", text(size: 7pt, "• 碰撞区域隆起（形成山脉）"))
    content((1, y - 0.7), anchor: "west", text(size: 7pt, "• 俯冲区域下沉（形成海沟）"))
    content((1, y - 1.0), anchor: "west", text(size: 7pt, "• 分离区域产生新地壳"))
    content((1, y - 1.3), anchor: "west", text(size: 7pt, "• 应用均衡调整（地壳均衡）"))

    // Arrow
    let y = y - 1.7
    line((width/2, y), (width/2, y - 0.4), mark: (end: "stealth"))

    // Step 5 & 6
    let y = y - 0.6
    rect((0.5, y), (width - 0.5, y - 0.9), stroke: (thickness: 0.6pt), fill: rgb("#e7e7ff"))
    content((1, y - 0.3), anchor: "west", text(weight: "bold", size: 7pt, "Step 5: 迭代（模拟地质时间）"))
    content((1, y - 0.6), anchor: "west", text(weight: "bold", size: 7pt, "Step 6: 后处理（噪声细节、平滑、侵蚀）"))
  }),
  caption: [板块构造模拟流程]
)

==== 数据结构

```rust
/// 板块
#[derive(Debug, Clone)]
pub struct TectonicPlate {
    pub id: u16,
    pub plate_type: PlateType,
    pub direction: f32,      // 运动方向（弧度）
    pub speed: f32,          // 运动速度
    pub cells: Vec<u32>,     // 板块包含的单元格
    pub boundary_cells: Vec<u32>,  // 边界单元格
    pub centroid: Pos2,      // 质心
    pub density: f32,        // 密度
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlateType {
    Continental,  // 大陆板块（密度 2.7 g/cm³）
    Oceanic,      // 海洋板块（密度 3.0 g/cm³）
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryType {
    Convergent { intensity: f32, subducting_plate: Option<u16> },
    Divergent { intensity: f32 },
    Transform { intensity: f32 },
}
```

==== 板块生成

```rust
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
    // ... 设置类型、运动向量、密度等
}
```

==== 边界分析

```rust
fn classify_boundary(
    plate_a: &TectonicPlate,
    plate_b: &TectonicPlate,
    boundary_cells: &[u32],
    cells: &[Pos2]
) -> BoundaryType {
    // 计算边界中点
    let boundary_center = calculate_centroid(boundary_cells, cells);
    
    // 运动向量
    let vel_a = Vec2::from_angle(plate_a.direction) * plate_a.speed;
    let vel_b = Vec2::from_angle(plate_b.direction) * plate_b.speed;
    
    // 边界法向量（从 A 指向 B）
    let normal = (plate_b.centroid - plate_a.centroid).normalized();
    
    // 相对运动在法向的投影
    let approach_a = vel_a.dot(normal);
    let approach_b = vel_b.dot(-normal);
    let relative_approach = approach_a + approach_b;
    
    // 判断边界类型
    if relative_approach > 0.3 {
        BoundaryType::Convergent { ... }  // 汇聚边界
    } else if relative_approach < -0.3 {
        BoundaryType::Divergent { ... }   // 分离边界
    } else {
        BoundaryType::Transform { ... }   // 转换边界
    }
}
```

==== 高度更新

```rust
fn apply_convergent_effects(
    heights: &mut [f32],
    boundary: &PlateBoundary,
    plates: &[TectonicPlate],
    config: &TectonicConfig
) {
    for &cell in &boundary.cells {
        for distance in 0..config.boundary_width as usize {
            let falloff = 1.0 - (distance as f32 / config.boundary_width);
            
            match subducting_plate {
                Some(subducting_id) => {
                    // 俯冲带：一侧下沉（海沟），另一侧隆起（火山弧）
                    if is_subducting_plate(affected, plates, subducting_id) {
                        heights[affected] -= config.subduction_depth_rate 
                            * intensity * falloff * 0.1;
                    } else {
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
```

==== 地壳均衡调整

```rust
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
```

==== 配置示例

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

==== 噪声细节叠加

在板块构造生成基础地形后，添加噪声细节以模拟中小尺度地质过程。

===== 约束性噪声策略

噪声强度应受板块特性和边界距离约束，以保持地质合理性：

```rust
/// 根据板块特性计算噪声强度
fn calculate_noise_strength(
    cell: usize,
    plate_type: PlateType,
    boundary_distance: f32,  // 归一化距离 [0, 1]，0=边界，1=板块中心
    height: f32,
) -> f32 {
    // 1. 基础噪声强度（受板块类型影响）
    let base_strength = match plate_type {
        PlateType::Continental => {
            // 大陆板块：更厚、更古老，有更多褶皱和起伏
            0.3
        }
        PlateType::Oceanic => {
            // 海洋板块：年轻、薄，相对平坦
            0.1
        }
    };

    // 2. 边界抑制因子
    // 板块边界已有明显地形（山脉/海沟），减少噪声干扰
    let boundary_suppression = 1.0 - (-boundary_distance * 5.0).exp();

    // 3. 高度调制（模拟侵蚀效应）
    let erosion_factor = if height > SEA_LEVEL {
        // 高山区侵蚀作用强，但保留尖峰特征
        1.0 + (height - SEA_LEVEL) / 255.0 * 0.5
    } else {
        // 海底相对平缓
        0.5
    };

    base_strength * boundary_suppression * erosion_factor
}

/// 应用分层噪声细节
fn apply_detail_noise(
    heights: &mut [f32],
    plates: &[TectonicPlate],
    plate_id: &[u16],
    cells: &[Pos2],
    config: &NoiseConfig,
) {
    for (i, &h) in heights.iter().enumerate() {
        let pid = plate_id[i];
        if pid == 0 { continue; }

        let plate = &plates[pid as usize - 1];

        // 计算到板块边界的距离
        let boundary_dist = calculate_boundary_distance(i, plate, cells);

        // 计算噪声强度
        let strength = calculate_noise_strength(
            i,
            plate.plate_type,
            boundary_dist,
            h,
        );

        // 多层噪声叠加
        let noise = fbm_noise(
            cells[i].x as f64,
            cells[i].y as f64,
            config
        );

        heights[i] += noise as f32 * strength * 255.0;
    }
}
```

===== 分阶段噪声应用

按地质时序应用不同尺度的噪声：

```rust
/// 完整地形生成流程
fn generate_terrain_with_details(
    cells: &[Pos2],
    config: &TerrainConfig,
) -> Vec<u8> {
    // ====== 阶段 1: 板块构造模拟 ======
    let (mut heights, plates, plate_id) = simulate_plate_tectonics(
        cells,
        &config.tectonic,
    );

    // ====== 阶段 2: 中尺度噪声（大地貌） ======
    // 模拟区域性构造活动、火山、褶皱
    let medium_noise_config = NoiseConfig {
        octaves: 3,
        base_frequency: 0.01,  // 低频
        persistence: 0.5,
        lacunarity: 2.0,
    };

    apply_detail_noise(
        &mut heights,
        &plates,
        &plate_id,
        cells,
        &medium_noise_config,
    );

    // ====== 阶段 3: 侵蚀模拟（可选） ======
    if config.enable_erosion {
        thermal_erosion(&mut heights, 30, 0.05);
        hydraulic_erosion(&mut heights, &config.erosion);
    }

    // ====== 阶段 4: 小尺度噪声（细节） ======
    // 模拟侵蚀沟壑、沉积等表面过程
    let detail_noise_config = NoiseConfig {
        octaves: 5,
        base_frequency: 0.05,  // 高频
        persistence: 0.4,
        lacunarity: 2.2,
    };

    apply_detail_noise(
        &mut heights,
        &plates,
        &plate_id,
        cells,
        &detail_noise_config,
    );

    // ====== 阶段 5: 归一化与后处理 ======
    normalize_heights(&mut heights, 0.0, 255.0);

    // 平滑处理（可选）
    if config.smoothing > 0 {
        smooth_heights(&mut heights, config.smoothing);
    }

    // 转换为 u8
    heights.iter().map(|&h| h.clamp(0.0, 255.0) as u8).collect()
}
```

===== 噪声参数配置

#figure(
  table(
    columns: (auto, auto, auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*阶段*], [*Octaves*], [*频率*], [*强度*], [*目的*],
    [中尺度], [3], [0.01], [0.2], [大地貌（山丘、盆地）],
    [小尺度], [5], [0.05], [0.1], [表面细节（沟壑、褶皱）],
  ),
  caption: [噪声参数建议]
)

*关键要点*：

- *约束性*：噪声不是均匀应用，而是根据板块类型、边界距离、高度动态调整
- *层次性*：先应用低频噪声（大地貌），再应用高频噪声（细节）
- *可选性*：侵蚀模拟计算量大，可作为高质量模式的可选项
- *保持主导*：噪声强度应适中（0.1-0.3），不能掩盖板块构造的主导作用

=== 侵蚀模拟（可选）

侵蚀可以使地形更加自然。

==== 热力侵蚀

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

==== 水力侵蚀

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
            
            // 5. 计算高度差 -> 侵蚀或沉积
            // 6. 蒸发
        }
    }
}
```

== 海陆分析算法

=== 海陆分离

```rust
fn classify_land_sea(heights: &[u8], sea_level: u8) -> Vec<bool> {
    heights.iter().map(|&h| h >= sea_level).collect()
}
```

=== 连通分量检测

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

=== 海岸线提取

```rust
fn find_coastline_cells(is_land: &[bool], neighbors: &NeighborMap) -> Vec<u32> {
    (0..is_land.len())
        .filter(|&i| {
            is_land[i] && neighbors[i].iter().any(|&n| !is_land[n])
        })
        .map(|i| i as u32)
        .collect()
}
```

== 水系生成算法

=== 流向计算

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

=== 流量累积

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
    
    // 初始流量
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

=== 河流路径提取

```rust
fn extract_rivers(
    flux: &[u16],
    flow_direction: &[Option<u32>],
    is_land: &[bool],
    threshold: u16
) -> Vec<River> {
    let mut visited = vec![false; flux.len()];
    let mut rivers = Vec::new();
    
    // 找到所有河口
    let mouths: Vec<usize> = (0..flux.len())
        .filter(|&i| {
            flux[i] >= threshold 
            && is_land[i]
            && flow_direction[i].map_or(false, |d| !is_land[d as usize])
        })
        .collect();
    
    for &mouth in &mouths {
        let river = trace_river_upstream(
            mouth, flux, flow_direction, &mut visited, threshold
        );
        if !river.cells.is_empty() {
            rivers.push(river);
        }
    }
    
    rivers
}
```

=== 湖泊检测

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
        
        if is_depression {
            let lake = fill_depression(start, heights, neighbors);
            for &cell in &lake.cells {
                in_lake[cell as usize] = true;
            }
            lakes.push(lake);
        }
    }
    
    lakes
}
```

=== 河流宽度计算

```rust
fn calculate_river_widths(rivers: &mut [River], flux: &[u16]) {
    for river in rivers.iter_mut() {
        let mouth_flux = flux[river.mouth_cell as usize] as f32;
        
        // 河口宽度（基于流量的对数）
        river.width_km = (mouth_flux.ln() * 0.5).max(0.1);
        
        // 每个点的相对宽度
        river.widths = river.cells.iter().map(|&cell| {
            let cell_flux = flux[cell as usize] as f32;
            let relative = cell_flux / mouth_flux;
            (relative.sqrt() * river.width_km * 10.0) as u8
        }).collect();
    }
}
```

== 气候计算算法

=== 温度计算

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
        let altitude = (heights[i] as f32 - 20.0).max(0.0) / 235.0;
        let altitude_km = altitude * config.max_altitude_km;
        let altitude_effect = -altitude_km * 6.5;
        
        let temp = base_temp + altitude_effect;
        temp.clamp(-128.0, 127.0) as i8
    }).collect()
}
```

=== 降水计算

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
        let rain_shadow = calculate_rain_shadow(i, pos, heights, cells, wind_dir);
        
        // 赤道附近降水更多
        let latitude_factor = 1.0 - (pos.y / config.map_height as f32 - 0.5).abs();
        
        let precip = base * rain_shadow * (0.5 + latitude_factor);
        (precip.clamp(0.0, 255.0)) as u8
    }).collect()
}
```

=== 生物群落分配

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
    flux: &[u16]
) -> Vec<u16> {
    temperature.iter().enumerate().map(|(i, &temp)| {
        if !is_land[i] { return 0; }
        
        let precip = precipitation[i] as i32;
        let temp = temp as i32;
        
        // 特殊生物群落
        if flux[i] > 1000 { return Biome::Wetland as u16; }
        
        // 基于温度和降水的分类
        match temp {
            t if t < -10 => Biome::IceCap,
            t if t < 0 => {
                if precip < 25 { Biome::Tundra } else { Biome::Taiga }
            },
            t if t < 10 => { /* 温带 */ },
            t if t < 20 => { /* 亚热带 */ },
            _ => { /* 热带 */ }
        } as u16
    }).collect()
}
```

== 人口分布算法

=== 适宜度评分

```rust
fn calculate_habitability(
    biome: &[u16],
    temperature: &[i8],
    rivers: &[River],
    coastline: &[u32]
) -> Vec<f32> {
    let mut habitability = vec![0.0; biome.len()];
    
    // 基于生物群落的基础适宜度
    let biome_base: HashMap<u16, f32> = hashmap! {
        0 => 0.0,   // 海洋
        5 => 0.9,   // 温带森林
        7 => 0.1,   // 沙漠
        10 => 0.5,  // 热带雨林
        // ...
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
    
    habitability
}
```

=== 人口分配

```rust
fn distribute_population(
    habitability: &[f32],
    is_land: &[bool],
    total_population: u64
) -> Vec<u32> {
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

== 文化区域生成

=== 文化种子放置

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
    
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    
    // 选择分散的起源点
    let mut origins = Vec::new();
    let min_distance = (population.len() as f32).sqrt() * 2.0;
    
    for (cell, _) in candidates {
        let too_close = origins.iter().any(|origin: &CultureOrigin| {
            cell_distance(cell, origin.cell) < min_distance
        });
        
        if !too_close {
            let culture_type = determine_culture_type(cell, biome);
            origins.push(CultureOrigin { cell: cell as u32, culture_type });
            
            if origins.len() >= culture_count as usize {
                break;
            }
        }
    }
    
    origins
}
```

=== 文化扩张

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
    
    // 初始化 & Dijkstra 风格扩张
    for (i, origin) in origins.iter().enumerate() {
        let id = (i + 1) as u16;
        culture[origin.cell as usize] = id;
        cost[origin.cell as usize] = 0.0;
        heap.push(Reverse((OrderedFloat(0.0), origin.cell, id)));
    }
    
    while let Some(Reverse((OrderedFloat(c), cell, culture_id))) = heap.pop() {
        let cell = cell as usize;
        if c > cost[cell] { continue; }
        
        for &neighbor in &neighbors[cell] {
            if !is_land[neighbor] { continue; }
            
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
```

== 国家生成算法

=== 首都选址

```rust
fn select_capitals(
    population: &[u32],
    culture: &[u16],
    rivers: &[River],
    harbor_scores: &[u8],
    state_count: u32
) -> Vec<u32> {
    let mut scores: Vec<(usize, f32)> = population.iter()
        .enumerate()
        .filter(|(_, &p)| p > 0)
        .map(|(i, &pop)| {
            let mut score = pop as f32;
            
            if is_on_river(i, rivers) { score *= 1.5; }
            score *= 1.0 + harbor_scores[i] as f32 * 0.2;
            if is_river_confluence(i, rivers) { score *= 2.0; }
            
            (i, score)
        })
        .collect();
    
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    // 选择分散的首都
    // ...
}
```

=== 国家扩张

使用优先队列实现带成本的领土扩张：

```rust
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
    if culture[to] != origin_culture { cost += 3.0; }
    
    // 地形惩罚
    let height_diff = (heights[to] as i32 - heights[from] as i32).abs();
    cost += height_diff as f32 * 0.05;
    
    // 山脉惩罚
    if heights[to] > 200 { cost += 5.0; }
    
    // 人口稀少区域惩罚
    if population[to] < 100 { cost += 2.0; }
    
    cost
}
```

=== 边界优化

```rust
fn optimize_state_borders(
    state: &mut [u16],
    neighbors: &NeighborMap,
    iterations: u32
) {
    for _ in 0..iterations {
        let mut changes = Vec::new();
        
        for cell in 0..state.len() {
            if state[cell] == 0 { continue; }
            
            // 统计邻居的国家分布
            let mut neighbor_states: HashMap<u16, u32> = HashMap::new();
            for &n in &neighbors[cell] {
                if state[n] != 0 {
                    *neighbor_states.entry(state[n]).or_insert(0) += 1;
                }
            }
            
            // 如果大多数邻居属于其他国家，考虑变更
            // ...
        }
        
        for (cell, new_state) in changes {
            state[cell] = new_state;
        }
    }
}
```

== 城镇放置算法

=== 城镇选址评分

```rust
fn score_burg_location(
    cell: usize,
    population: &[u32],
    rivers: &[River],
    coastline: &[u32],
    heights: &[u8],
    existing_burgs: &[Burg]
) -> f32 {
    let mut score = population[cell] as f32;
    
    if is_on_river(cell, rivers) { score *= 1.5; }
    if is_river_confluence(cell, rivers) { score *= 2.0; }
    if coastline.contains(&(cell as u32)) {
        let harbor_quality = calculate_harbor_quality(cell, heights);
        score *= 1.0 + harbor_quality;
    }
    if heights[cell] < 50 { score *= 1.2; }  // 平原加成
    
    // 距离现有城市的惩罚
    for burg in existing_burgs {
        let distance = cell_distance(cell, burg.cell as usize);
        if distance < 10.0 {
            score *= distance / 10.0;
        }
    }
    
    score
}
```

== 道路生成算法

=== A\* 路径查找

```rust
fn find_route_path(
    from: u32,
    to: u32,
    heights: &[u8],
    rivers: &[River],
    is_land: &[bool],
    neighbors: &NeighborMap
) -> Option<Vec<u32>> {
    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<usize, usize> = HashMap::new();
    let mut g_score = vec![f32::INFINITY; heights.len()];
    
    g_score[from as usize] = 0.0;
    open_set.push(Reverse((OrderedFloat(heuristic(from, to)), from)));
    
    while let Some(Reverse((_, current))) = open_set.pop() {
        if current == to as usize {
            return Some(reconstruct_path(&came_from, current));
        }
        
        for &neighbor in &neighbors[current] {
            if !is_land[neighbor] { continue; }
            
            let movement_cost = calculate_movement_cost(
                current, neighbor, heights, rivers
            );
            let tentative_g = g_score[current] + movement_cost;
            
            if tentative_g < g_score[neighbor] {
                came_from.insert(neighbor, current);
                g_score[neighbor] = tentative_g;
                let f_score = tentative_g + heuristic(neighbor, to as usize);
                open_set.push(Reverse((OrderedFloat(f_score), neighbor)));
            }
        }
    }
    
    None
}

fn calculate_movement_cost(from: usize, to: usize, heights: &[u8], rivers: &[River]) -> f32 {
    let mut cost = 1.0;
    
    // 高度差成本
    let height_diff = (heights[to] as i32 - heights[from] as i32).abs();
    cost += height_diff as f32 * 0.1;
    
    // 山地惩罚
    if heights[to] > 150 { cost += 5.0; }
    
    // 河流穿越成本
    if crosses_river(from, to, rivers) { cost += 3.0; }
    
    cost
}
```

== 名称生成算法

=== 马尔可夫链生成器

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
        let mut state = chain.random_start();
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
        capitalize_first(&name)
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
                // 统计转移概率...
            }
        }
        
        Self { order, transitions: /* 转换为概率 */ }
    }
}
```

== 实现进度（Phase 1）

=== 已完成功能 ✓

Phase 1 的核心功能已实现并集成到主应用中：

==== 1. 地形生成系统 ✓

*实现的模块*：
- `terrain/plate.rs`：板块构造模拟核心
- `terrain/noise.rs`：FBM 噪声生成系统
- `terrain/heightmap.rs`：完整地形生成管道
- `terrain/hydrology.rs`：水系生成（流向、河流、湖泊）

*实现的功能*：
- ✓ 板块生成与类型分配（大陆/海洋）
- ✓ 板块运动模拟（汇聚/分离/转换边界）
- ✓ 高度更新（造山、俯冲、裂谷）
- ✓ 地壳均衡调整
- ✓ 多尺度噪声叠加（中尺度 + 小尺度）
- ✓ 热力侵蚀（可选）
- ✓ 海陆分离（海平面阈值 = 20）
- ✓ 流向计算
- ✓ 流量累积
- ✓ 河流提取
- ✓ 湖泊检测
- ✓ 陆块分类

*配置预设*：
- ✓ `TerrainConfig::default()` - 默认配置
- 可扩展支持 earth_like、mountainous、archipelago 等预设

==== 2. 可视化层系统 ✓

*渲染模块*：
- `gpu/heightmap/heightmap_renderer.rs`：高度图渲染器
- `gpu/heightmap/heightmap_callback.rs`：渲染回调
- `assets/shaders/heightmap.wgsl`：高度图着色器

*实现的功能*：
- ✓ 填充的 Voronoi 单元格渲染（GPU 加速）
- ✓ 基于高度的颜色映射：
  - 海洋 (< 20)：深蓝 → 浅蓝
  - 低地 (20-97)：绿色
  - 中地 (97-176)：棕色
  - 高地 (176-255)：白色（雪）
- ✓ Fan 三角剖分（将多边形转为三角形）
- ✓ Storage Buffer 优化（最大 100 万顶点）

==== 3. 图层管理系统 ✓

*实现的结构*：
- `models/map/system.rs::LayerVisibility`：图层可见性状态

*支持的图层*：
- ✓ heightmap：高度图（填充的 Voronoi 单元格）
- ✓ voronoi_edges：Voronoi 边线
- ✓ delaunay：Delaunay 三角剖分
- ✓ points：原始点

*默认配置*：
- heightmap：开启
- 其他图层：关闭

==== 4. UI 控制系统 ✓

*实现的功能*：
- ✓ 左侧控制面板（SidePanel）
- ✓ 图层可见性复选框
- ✓ 地形生成按钮
- ✓ 启动时自动生成初始地形

*UI 位置*：
- `app.rs::update()` - 左侧面板 250px 宽
- 实时响应图层切换

=== 技术实现细节

==== 渲染架构

```rust
// 图层条件渲染（widget_impl.rs）
let layer_visibility = map_system.layer_visibility;

if layer_visibility.heightmap {
    // 渲染高度图（填充的 Voronoi 单元格）
    ui.painter().add(HeightmapCallback::new(...));
}

if layer_visibility.voronoi_edges {
    // 渲染 Voronoi 边线
    ui.painter().add(VoronoiCallback::new(...));
}

// 其他图层类似...
```

==== 地形生成流程

```rust
fn generate_terrain() {
    // 1. 创建生成器
    let config = TerrainConfig::default();
    let generator = TerrainGenerator::new(config);

    // 2. 提取单元格数据
    let cells = map_system.grid.get_all_points();
    let neighbors = extract_neighbors(&delaunay);

    // 3. 生成地形
    let (heights, plates, plate_id) = generator.generate(&cells, &neighbors);

    // 4. 更新地图系统
    map_system.cells_data.height = heights;
}
```

==== 邻居提取算法

```rust
fn extract_neighbors(triangles: &[u32], num_points: usize) -> Vec<Vec<u32>> {
    // 从 Delaunay 三角剖分提取每个点的邻居
    for triangle in triangles.chunks(3) {
        let (a, b, c) = (triangle[0], triangle[1], triangle[2]);
        // a 的邻居：b, c
        // b 的邻居：a, c
        // c 的邻居：a, b
    }
}
```

=== 待实现功能（Phase 2-4）

Phase 1 完成后，后续阶段包括：

==== Phase 2：气候与生物群落
- ⬜ 温度计算（纬度 + 海拔）
- ⬜ 降水计算（雨影效应）
- ⬜ 生物群落分配（Whittaker 分类）

==== Phase 3：人文系统
- ⬜ 人口分布
- ⬜ 文化区域生成
- ⬜ 国家生成
- ⬜ 城镇放置

==== Phase 4：交通网络
- ⬜ 道路生成（A\*）
- ⬜ 贸易路线
- ⬜ 航运路线

=== 性能指标

当前实现的性能表现：

*地形生成*：
- 点数：~20,000 个
- 板块数：15 个
- 迭代次数：200 次
- 生成时间：~ 1-2 秒（Debug 模式）

*渲染性能*：
- 顶点数：~ 100,000 个三角形顶点
- 帧率：60 FPS（1080p）
- GPU 内存：< 10 MB

== 性能优化技巧

=== 并行处理

```rust
// 使用 rayon 并行处理独立单元格
use rayon::prelude::*;

let heights: Vec<u8> = cells.par_iter()
    .map(|pos| calculate_height(pos, &noise_config))
    .collect();
```

=== 空间局部性

```rust
// 按空间位置排序以提高缓存命中率
let mut sorted_indices: Vec<usize> = (0..cells.len()).collect();
sorted_indices.sort_by(|&a, &b| {
    let za = morton_code(cells[a]);
    let zb = morton_code(cells[b]);
    za.cmp(&zb)
});
```

=== 增量更新

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
