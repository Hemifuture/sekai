// 第二章：架构设计文档

#import "@preview/cetz:0.4.2"

= 架构设计文档

== 系统架构总览

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    // 应用层
    rect((0, 0), (14, -2.5), stroke: (thickness: 1pt), fill: rgb("#e8f4f8"))
    content((7, -0.4), text(weight: "bold", size: 10pt, "Application Layer"))

    rect((0.5, -0.8), (13.5, -2.3), stroke: (thickness: 0.8pt), fill: white)
    content((7, -1.2), text(weight: "bold", size: 9pt, "TemplateApp (app.rs)"))
    content((7, -1.6), text(size: 8pt, "应用生命周期管理 • 资源初始化 • UI 布局"))

    // 向下箭头
    line((7, -2.5), (7, -3.2), mark: (end: "stealth"))

    // 中间三层
    let y = -3.2
    let box-width = 4
    let box-height = 3
    let gap = 0.5

    // UI Layer
    rect((0.5, y), (0.5 + box-width, y - box-height),
         stroke: (thickness: 0.8pt), fill: rgb("#fff3cd"))
    content((0.5 + box-width / 2, y - 0.5),
            text(weight: "bold", size: 9pt, "UI Layer"))
    content((0.5 + box-width / 2, y - 1.5),
            text(size: 7pt, align(center, [Canvas \ InputManager \ Panels \ Dialogs])))

    // Generator Layer
    rect((0.5 + box-width + gap, y), (0.5 + box-width * 2 + gap, y - box-height),
         stroke: (thickness: 0.8pt), fill: rgb("#d4edda"))
    content((0.5 + box-width * 1.5 + gap, y - 0.5),
            text(weight: "bold", size: 9pt, "Generator Layer"))
    content((0.5 + box-width * 1.5 + gap, y - 1.8),
            text(size: 7pt, align(center, [GeneratorPipeline \ HeightmapGenerator \ RiverGenerator \ StateGenerator])))

    // GPU Layer
    rect((0.5 + box-width * 2 + gap * 2, y), (0.5 + box-width * 3 + gap * 2, y - box-height),
         stroke: (thickness: 0.8pt), fill: rgb("#f8d7da"))
    content((0.5 + box-width * 2.5 + gap * 2, y - 0.5),
            text(weight: "bold", size: 9pt, "GPU Layer"))
    content((0.5 + box-width * 2.5 + gap * 2, y - 1.5),
            text(size: 7pt, align(center, [Renderers \ Shaders \ Pipelines])))

    // 双向箭头
    line((0.5 + box-width, y - box-height / 2), (0.5 + box-width + gap, y - box-height / 2),
         mark: (start: "stealth", end: "stealth"))
    line((0.5 + box-width * 2 + gap, y - box-height / 2),
         (0.5 + box-width * 2 + gap * 2, y - box-height / 2),
         mark: (start: "stealth", end: "stealth"))

    // 向下箭头到 Model Layer
    let y2 = y - box-height
    line((7, y2), (7, y2 - 0.7), mark: (end: "stealth"))

    // Model Layer
    let y3 = y2 - 0.7
    rect((0, y3), (14, y3 - 3), stroke: (thickness: 1pt), fill: rgb("#e7e7ff"))
    content((7, y3 - 0.4), text(weight: "bold", size: 10pt, "Model Layer"))

    rect((0.5, y3 - 0.8), (13.5, y3 - 2.8), stroke: (thickness: 0.8pt), fill: white)
    content((7, y3 - 1.2), text(weight: "bold", size: 9pt, "MapSystem"))
    content((7, y3 - 2), text(size: 7pt, "Grid • CellsData • EdgesData • Features • Delaunay • Voronoi • SpatialIndex"))

    // 向下箭头到 Resource Layer
    let y4 = y3 - 3
    line((7, y4), (7, y4 - 0.7), mark: (end: "stealth"))

    // Resource Layer
    let y5 = y4 - 0.7
    rect((0, y5), (14, y5 - 2), stroke: (thickness: 1pt), fill: rgb("#f0f0f0"))
    content((7, y5 - 0.4), text(weight: "bold", size: 10pt, "Resource Layer"))

    rect((0.5, y5 - 0.8), (13.5, y5 - 1.8), stroke: (thickness: 0.8pt), fill: white)
    content((7, y5 - 1.3),
            text(size: 8pt, [Resource\<T\> - 线程安全的共享资源容器 (Arc\<RwLock\<T\>\>)]))
  }),
  caption: [系统架构总览]
)

== 分层数据架构

=== 地图数据层次

地图数据按照逻辑依赖关系分层，每一层依赖于下层的数据：

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let layer-height = 2.0
    let start-y = 0
    let width = 14

    // Layer 7: Labels
    let y = start-y
    rect((0, y), (width, y - layer-height), stroke: (thickness: 0.8pt), fill: rgb("#ffe6e6"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 7: 标注层 (Labels)"))
    content((0.5, y - 1.0), anchor: "west", text(size: 7pt, "地名、城市名、区域名称的位置和样式"))

    // Layer 6: Routes
    set-style(stroke: (thickness: 0.8pt))
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#fff0e6"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 6: 路线层 (Routes)"))
    content((0.5, y - 1.0), anchor: "west", text(size: 7pt, "道路网络、航线、贸易路线"))
    content((0.5, y - 1.5), anchor: "west", text(size: 6.5pt, fill: gray, "依赖: 城镇、地形、河流"))

    // Layer 5: Burgs
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#fff9e6"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 5: 城镇层 (Burgs)"))
    content((0.5, y - 1.0), anchor: "west", text(size: 7pt, "城市、城镇、村庄位置和属性"))
    content((0.5, y - 1.5), anchor: "west", text(size: 6.5pt, fill: gray, "依赖: 国家、地形、河流、人口"))

    // Layer 4: Politics
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#ffffe6"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 4: 政治层 (Politics)"))
    content((0.8, y - 0.9), anchor: "west", text(size: 7pt, "├ States (国家)"))
    content((0.8, y - 1.3), anchor: "west", text(size: 7pt, "├ Provinces (省份)"))
    content((0.8, y - 1.7), anchor: "west", text(size: 7pt, "└ Religions (宗教)"))

    // Layer 3: Demographics
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#f0ffe6"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 3: 人文层 (Demographics)"))
    content((0.8, y - 0.9), anchor: "west", text(size: 7pt, "├ Cultures (文化区域)"))
    content((0.8, y - 1.3), anchor: "west", text(size: 7pt, "└ Population (人口分布)"))
    content((0.5, y - 1.7), anchor: "west", text(size: 6.5pt, fill: gray, "依赖: 生物群落、水系、地形"))

    // Layer 2: Climate
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#e6fff9"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 2: 气候层 (Climate)"))
    content((0.8, y - 0.9), anchor: "west", text(size: 7pt, "├ Temperature (温度)"))
    content((0.8, y - 1.3), anchor: "west", text(size: 7pt, "├ Precipitation (降水)"))
    content((0.8, y - 1.7), anchor: "west", text(size: 7pt, "└ Biomes (生物群落)"))

    // Layer 1: Hydrography
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#e6f3ff"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 1: 水系层 (Hydrography)"))
    content((0.8, y - 0.9), anchor: "west", text(size: 7pt, "├ Rivers (河流)"))
    content((0.8, y - 1.3), anchor: "west", text(size: 7pt, "├ Lakes (湖泊)"))
    content((0.8, y - 1.7), anchor: "west", text(size: 7pt, "└ Coastline (海岸线)"))

    // Layer 0: Terrain
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#f0e6ff"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Layer 0: 地形层 (Terrain)"))
    content((0.8, y - 0.9), anchor: "west", text(size: 7pt, "├ Heightmap (高度图)"))
    content((0.8, y - 1.3), anchor: "west", text(size: 7pt, "└ Land/Sea (海陆分布)"))
    content((0.5, y - 1.7), anchor: "west", text(size: 6.5pt, fill: gray, "基础层，无依赖"))

    // Base: Geometry
    let y = y - layer-height
    rect((0, y), (width, y - layer-height), fill: rgb("#f5f5f5"))
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "Base: 几何层 (Geometry)"))
    content((0.8, y - 0.9), anchor: "west", text(size: 7pt, "├ Grid Points"))
    content((0.8, y - 1.3), anchor: "west", text(size: 7pt, "├ Delaunay Triangulation"))
    content((0.8, y - 1.7), anchor: "west", text(size: 7pt, "└ Voronoi Diagram"))
  }),
  caption: [地图数据层次]
)

=== 数据依赖图

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let box-width = 2.5
    let box-height = 0.8
    let center-x = 7

    // Helper function to draw a node
    let draw-node = (x, y, name, color) => {
      rect((x - box-width/2, y), (x + box-width/2, y - box-height),
           stroke: (thickness: 0.8pt), fill: color)
      content((x, y - box-height/2), text(weight: "bold", size: 8pt, name))
    }

    // Grid
    let y = 0
    draw-node(center-x, y, "Grid", rgb("#e8f4f8"))

    // Delaunay
    let y = y - 1.5
    line((center-x, y + 0.7), (center-x, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x, y, "Delaunay", rgb("#e8f4f8"))

    // Voronoi
    let y = y - 1.5
    line((center-x, y + 0.7), (center-x, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x, y, "Voronoi", rgb("#e8f4f8"))

    // Heightmap
    let y = y - 1.5
    line((center-x, y + 0.7), (center-x, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x, y, "Heightmap", rgb("#d4edda"))

    // Split to three branches
    let y = y - 1.5
    line((center-x, y + 0.7), (center-x, y + 0.4))
    line((center-x, y + 0.4), (center-x - 3, y + 0.4))
    line((center-x, y + 0.4), (center-x + 3, y + 0.4))
    line((center-x - 3, y + 0.4), (center-x - 3, y + 0.1), mark: (end: "stealth"))
    line((center-x, y + 0.4), (center-x, y + 0.1), mark: (end: "stealth"))
    line((center-x + 3, y + 0.4), (center-x + 3, y + 0.1), mark: (end: "stealth"))

    draw-node(center-x - 3, y, "Coastline", rgb("#d4edda"))
    draw-node(center-x, y, "Temp", rgb("#d4edda"))
    draw-node(center-x + 3, y, "Precipit.", rgb("#d4edda"))

    // Climate
    let y = y - 1.5
    line((center-x, y + 0.7), (center-x, y + 0.1), mark: (end: "stealth"))
    line((center-x + 3, y + 0.7), (center-x, y + 0.3))
    draw-node(center-x, y, "Climate", rgb("#d4edda"))

    // Rivers and Biomes
    let y = y - 1.5
    line((center-x - 3, y + 0.7), (center-x - 3, y + 0.1), mark: (end: "stealth"))
    line((center-x, y + 0.7), (center-x, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x - 3, y, "Rivers", rgb("#a8d5e2"))
    draw-node(center-x, y, "Biomes", rgb("#d4edda"))

    // Merge to Population
    let y = y - 1.5
    line((center-x - 3, y + 0.7), (center-x - 1.5, y + 0.3))
    line((center-x, y + 0.7), (center-x - 1.5, y + 0.3))
    line((center-x - 1.5, y + 0.3), (center-x - 1.5, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x - 1.5, y, "Population", rgb("#fff3cd"))

    // Cultures
    let y = y - 1.5
    line((center-x - 1.5, y + 0.7), (center-x - 1.5, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x - 1.5, y, "Cultures", rgb("#fff3cd"))

    // States
    let y = y - 1.5
    line((center-x - 1.5, y + 0.7), (center-x - 1.5, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x - 1.5, y, "States", rgb("#f8d7da"))

    // Split to three branches
    let y = y - 1.5
    line((center-x - 1.5, y + 0.7), (center-x - 1.5, y + 0.4))
    line((center-x - 1.5, y + 0.4), (center-x - 3.5, y + 0.4))
    line((center-x - 1.5, y + 0.4), (center-x + 1, y + 0.4))
    line((center-x - 3.5, y + 0.4), (center-x - 3.5, y + 0.1), mark: (end: "stealth"))
    line((center-x - 1.5, y + 0.4), (center-x - 1.5, y + 0.1), mark: (end: "stealth"))
    line((center-x + 1, y + 0.4), (center-x + 1, y + 0.1), mark: (end: "stealth"))

    draw-node(center-x - 3.5, y, "Provinces", rgb("#f8d7da"))
    draw-node(center-x - 1.5, y, "Burgs", rgb("#f8d7da"))
    draw-node(center-x + 1, y, "Religions", rgb("#f8d7da"))

    // Routes
    let y = y - 1.5
    line((center-x - 1.5, y + 0.7), (center-x - 1.5, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x - 1.5, y, "Routes", rgb("#e7e7ff"))

    // Labels
    let y = y - 1.5
    line((center-x - 1.5, y + 0.7), (center-x - 1.5, y + 0.1), mark: (end: "stealth"))
    draw-node(center-x - 1.5, y, "Labels", rgb("#e7e7ff"))
  }),
  caption: [数据依赖关系图]
)

== 生成器管线 (Generator Pipeline)

=== 管线架构

生成器采用管线模式，每个生成阶段都是独立的处理单元：

```rust
/// 生成器管线
pub struct GeneratorPipeline {
    stages: Vec<Box<dyn GeneratorStage>>,
    config: GeneratorConfig,
}

/// 生成阶段 trait
pub trait GeneratorStage: Send + Sync {
    /// 阶段名称
    fn name(&self) -> &'static str;
    
    /// 检查前置条件是否满足
    fn can_run(&self, map: &MapSystem) -> bool;
    
    /// 执行生成
    fn execute(&self, map: &mut MapSystem, config: &GeneratorConfig) 
        -> Result<(), GeneratorError>;
    
    /// 获取进度（0.0 - 1.0）
    fn progress(&self) -> f32;
}
```

=== 生成阶段定义

#figure(
  table(
    columns: (auto, auto, 1fr, 1fr),
    stroke: 0.5pt,
    inset: 5pt,
    [*阶段*], [*名称*], [*输入*], [*输出*],
    [Stage 0], [GridGenerator], [MapConfig], [Grid, Delaunay, Voronoi],
    [Stage 1], [TectonicGenerator], [Voronoi cells], [TectonicPlates, height\[\]],
    [Stage 2], [CoastlineGenerator], [height\[\]], [Landmasses, Islands],
    [Stage 3], [ClimateGenerator], [height\[\], latitude], [temperature\[\], precipitation\[\]],
    [Stage 4], [RiverGenerator], [height\[\], precipitation\[\]], [Rivers, Lakes],
    [Stage 5], [BiomeGenerator], [temp\[\], precip\[\], height\[\]], [biome\[\]],
    [Stage 6], [PopulationGenerator], [biome\[\], rivers\[\], coastline\[\]], [population\[\]],
    [Stage 7], [CultureGenerator], [population\[\], biome\[\], rivers\[\]], [Cultures, culture\[\]],
    [Stage 8], [StateGenerator], [culture\[\], population\[\], biome\[\]], [States, state\[\]],
    [Stage 9], [ProvinceGenerator], [state\[\], population\[\]], [Provinces, province\[\]],
    [Stage 10], [BurgGenerator], [state\[\], population\[\], rivers\[\]], [Burgs\[\]],
    [Stage 11], [ReligionGenerator], [culture\[\], state\[\], burgs\[\]], [Religions, religion\[\]],
    [Stage 12], [RouteGenerator], [burgs\[\], state\[\], height\[\]], [Routes\[\]],
    [Stage 13], [NameGenerator], [All features], [Names for all features],
  ),
  caption: [生成阶段定义]
)

=== 生成配置

```rust
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    // 基础设置
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub cell_spacing: u32,
    
    // 板块构造设置
    pub tectonic: TectonicConfig,
    
    // 气候设置
    pub temperature_scale: f32,   // 全局温度调整
    pub precipitation_scale: f32,
    pub wind_direction: f32,      // 主导风向（弧度）
    
    // 政治设置
    pub state_count: u32,
    pub culture_count: u32,
    pub religion_count: u32,
    
    // 城镇设置
    pub total_population: u64,
    pub city_count: u32,
    pub town_count: u32,
    
    // 生成选项
    pub generate_rivers: bool,
    pub generate_states: bool,
    pub generate_religions: bool,
    pub generate_routes: bool,
}

/// 板块构造配置
#[derive(Debug, Clone)]
pub struct TectonicConfig {
    pub plate_count: u32,
    pub continental_ratio: f32,
    pub iterations: u32,
    pub collision_uplift_rate: f32,
    pub subduction_depth_rate: f32,
    pub rift_depth_rate: f32,
    pub isostatic_rate: f32,
    pub boundary_width: f32,
    pub noise_strength: f32,
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
        }
    }
}
```

== 核心模块详解

=== 几何计算模块 (`src/delaunay/`)

负责地图的几何基础计算。

```
delaunay/
├── mod.rs           # 模块导出
├── delaunay.rs      # Delaunay 三角剖分核心算法
├── voronoi.rs       # Voronoi 图生成
├── half_edge.rs     # 半边数据结构（邻接关系）
├── triangle.rs      # 三角形数据结构
├── utils.rs         # 工具函数（凸包计算等）
└── tests.rs         # 单元测试
```

*数据流*:

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let center-x = 7
    let box-width = 5
    let box-height = 0.8
    let process-width = 4.5

    // Helper to draw data box
    let draw-data = (x, y, name, color) => {
      rect((x - box-width/2, y), (x + box-width/2, y - box-height),
           stroke: (thickness: 0.8pt), fill: color, radius: 0.1)
      content((x, y - box-height/2), text(weight: "bold", size: 8pt, name))
    }

    // Helper to draw process box
    let draw-process = (x, y, name, note: none) => {
      rect((x - process-width/2, y), (x + process-width/2, y - box-height),
           stroke: (thickness: 0.8pt), fill: rgb("#e8f4f8"))
      content((x, y - box-height/2), text(weight: "bold", size: 8pt, name))
      if note != none {
        content((x + process-width/2 + 0.5, y - box-height/2), anchor: "west",
                text(size: 7pt, fill: gray, note))
      }
    }

    // Points
    let y = 0
    draw-data(center-x, y, "Points (Vec<Pos2>)", rgb("#d4edda"))

    // Arrow
    let y = y - 1.3
    line((center-x, y + 0.5), (center-x, y + 0.1), mark: (end: "stealth"))

    // triangulate()
    draw-process(center-x, y, "triangulate()", note: "使用 delaunator 库")

    // Arrow
    let y = y - 1.3
    line((center-x, y + 0.5), (center-x, y + 0.1), mark: (end: "stealth"))

    // Triangle Indices
    draw-data(center-x, y, "Triangle Indices (Vec<usize>)", rgb("#d4edda"))

    // Arrow
    let y = y - 1.3
    line((center-x, y + 0.5), (center-x, y + 0.1), mark: (end: "stealth"))

    // compute_indexed_voronoi
    draw-process(center-x, y, "compute_indexed_voronoi()")

    // Arrow
    let y = y - 1.3
    line((center-x, y + 0.5), (center-x, y + 0.1), mark: (end: "stealth"))

    // IndexedVoronoiDiagram
    let y = y
    rect((center-x - box-width/2, y), (center-x + box-width/2, y - 2.5),
         stroke: (thickness: 0.8pt), fill: rgb("#fff3cd"))
    content((center-x, y - 0.4), text(weight: "bold", size: 8pt, "IndexedVoronoiDiagram"))
    content((center-x - box-width/2 + 0.3, y - 1.0), anchor: "west",
            text(size: 7pt, "├ vertices: Vec<Pos2>"))
    content((center-x + box-width/2 - 0.3, y - 1.0), anchor: "east",
            text(size: 6.5pt, fill: gray, "Voronoi 顶点"))
    content((center-x - box-width/2 + 0.3, y - 1.5), anchor: "west",
            text(size: 7pt, "├ indices: Vec<usize>"))
    content((center-x + box-width/2 - 0.3, y - 1.5), anchor: "east",
            text(size: 6.5pt, fill: gray, "边索引"))
    content((center-x - box-width/2 + 0.3, y - 2.0), anchor: "west",
            text(size: 7pt, "└ cells: Vec<VoronoiCell>"))
    content((center-x + box-width/2 - 0.3, y - 2.0), anchor: "east",
            text(size: 6.5pt, fill: gray, "单元格"))
  }),
  caption: [几何数据流]
)

=== 生成器模块 (`src/generators/`)

包含所有地图生成算法。

```
generators/
├── mod.rs              # 模块导出和 GeneratorPipeline
├── config.rs           # GeneratorConfig 定义
├── error.rs            # GeneratorError 错误类型
│
├── terrain/
│   ├── mod.rs
│   ├── heightmap.rs    # 高度图生成（噪声算法）
│   ├── coastline.rs    # 海岸线检测
│   └── templates.rs    # 高度图模板
│
├── hydrology/
│   ├── mod.rs
│   ├── rivers.rs       # 河流生成
│   ├── lakes.rs        # 湖泊生成
│   └── drainage.rs     # 流域分析
│
├── climate/
│   ├── mod.rs
│   ├── temperature.rs  # 温度计算
│   ├── precipitation.rs # 降水计算
│   └── biomes.rs       # 生物群落分配
│
├── demographics/
│   ├── mod.rs
│   ├── population.rs   # 人口分布
│   └── cultures.rs     # 文化区域
│
├── politics/
│   ├── mod.rs
│   ├── states.rs       # 国家生成
│   ├── provinces.rs    # 省份划分
│   └── religions.rs    # 宗教分布
│
├── settlements/
│   ├── mod.rs
│   ├── burgs.rs        # 城镇放置
│   └── routes.rs       # 道路生成
│
└── naming/
    ├── mod.rs
    ├── generator.rs    # 名称生成器
    └── patterns.rs     # 命名模式
```

=== 数据模型模块 (`src/models/`)

定义地图的核心数据结构。

```
models/
├── mod.rs
├── map_layer.rs          # 图层 trait
│
├── map/
│   ├── mod.rs
│   ├── grid.rs           # 网格生成
│   ├── system.rs         # MapSystem - 核心数据容器
│   ├── cells_data.rs     # 单元格属性数据
│   ├── edges_data.rs     # 边属性数据
│   └── neighbors.rs      # 邻接关系
│
└── features/
    ├── mod.rs
    ├── landmass.rs       # 大陆/岛屿
    ├── river.rs          # 河流
    ├── lake.rs           # 湖泊
    ├── culture.rs        # 文化
    ├── state.rs          # 国家
    ├── province.rs       # 省份
    ├── religion.rs       # 宗教
    ├── burg.rs           # 城镇
    └── route.rs          # 道路/航线
```

==== MapSystem 结构

```rust
/// 地图系统 - 所有地图数据的容器
pub struct MapSystem {
    // === 配置 ===
    pub config: MapConfig,
    
    // === 基础几何数据 ===
    pub grid: Grid,
    pub delaunay: Vec<u32>,
    pub voronoi: IndexedVoronoiDiagram,
    
    // === 单元格属性 ===
    pub cells_data: CellsData,
    pub edges_data: EdgesData,
    
    // === 空间索引 ===
    pub point_index: GridIndex,
    pub voronoi_edge_index: EdgeIndex,
    pub delaunay_edge_index: EdgeIndex,
    
    // === 特征数据 ===
    pub landmasses: Vec<Landmass>,
    pub rivers: Vec<River>,
    pub lakes: Vec<Lake>,
    pub cultures: Vec<Culture>,
    pub states: Vec<State>,
    pub provinces: Vec<Province>,
    pub religions: Vec<Religion>,
    pub burgs: Vec<Burg>,
    pub routes: Vec<Route>,
    
    // === 元数据 ===
    pub generation_stage: GenerationStage,
    pub random_seed: u64,
}

/// 单元格数据（扩展版）
#[derive(Debug, Clone)]
pub struct CellsData {
    // 地形
    pub height: Vec<u8>,           // 高度 (0-255)
    pub is_water: Vec<bool>,       // 是否水体
    
    // 气候
    pub temperature: Vec<i8>,      // 温度
    pub precipitation: Vec<u8>,    // 降水量
    pub biome: Vec<u16>,           // 生物群落 ID
    
    // 水系
    pub flux: Vec<u16>,            // 水流量
    pub lake_id: Vec<u16>,         // 湖泊 ID (0=无)
    
    // 人文
    pub population: Vec<u32>,      // 人口
    pub culture: Vec<u16>,         // 文化 ID
    
    // 政治
    pub state: Vec<u16>,           // 国家 ID
    pub province: Vec<u16>,        // 省份 ID
    pub religion: Vec<u16>,        // 宗教 ID
    
    // 城镇
    pub burg: Vec<u16>,            // 城镇 ID (0=无)
    pub harbor: Vec<u8>,           // 港口等级
}
```

=== GPU 渲染模块 (`src/gpu/`)

负责所有图形渲染，使用 wgpu。

```
gpu/
├── mod.rs
├── canvas_uniform.rs     # 画布变换 Uniform 结构
├── map_renderer.rs       # 渲染管线创建工厂
├── pipelines.rs          # 管线配置
├── helpers.rs            # 视口裁剪等辅助函数
│
├── points/               # 点渲染
├── delaunay/             # Delaunay 边渲染
├── voronoi/              # Voronoi 边渲染
├── terrain/              # 地形渲染（新增）
├── water/                # 水系渲染（新增）
├── borders/              # 边界渲染（新增）
├── icons/                # 图标渲染（新增）
└── labels/               # 标注渲染（新增）
```

==== 渲染管线架构

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let width = 14
    let y = 0

    // Main container
    rect((0, y), (width, y - 10.2), stroke: (thickness: 1pt), fill: rgb("#f5f5f5"))
    content((width/2, y - 0.4), text(weight: "bold", size: 10pt, "Render Pipeline"))

    // Pass 1: Terrain Fill
    let y = y - 1.2
    content((0.5, y - 0.3), anchor: "west", text(weight: "bold", size: 8pt, "Pass 1: Terrain Fill (Fragment Shader)"))
    let y = y - 0.6
    rect((0.5, y), (width - 0.5, y - 1.2), stroke: (thickness: 0.6pt), fill: rgb("#d4edda"))
    content((1, y - 0.3), anchor: "west", text(size: 7pt, "Input: cell vertices, height/biome data"))
    content((1, y - 0.6), anchor: "west", text(size: 7pt, "Output: Colored cell polygons"))
    content((1, y - 0.9), anchor: "west", text(size: 7pt, "Shader: terrain.wgsl"))

    // Pass 2: Water Bodies
    let y = y - 1.6
    content((0.5, y - 0.3), anchor: "west", text(weight: "bold", size: 8pt, "Pass 2: Water Bodies"))
    let y = y - 0.6
    rect((0.5, y), (width - 0.5, y - 0.9), stroke: (thickness: 0.6pt), fill: rgb("#a8d5e2"))
    content((1, y - 0.3), anchor: "west", text(size: 7pt, "2a: Ocean - Blue gradient with depth"))
    content((1, y - 0.6), anchor: "west", text(size: 7pt, "2b: Lakes - Solid blue fill"))

    // Pass 3-8: Simple passes
    let passes = (
      "Pass 3: Rivers (Line Rendering)",
      "Pass 4: Borders (Line Rendering)",
      "Pass 5: Routes (Line Rendering)",
      "Pass 6: Icons (Instanced Quads)",
      "Pass 7: Labels (Text Rendering via egui)",
      "Pass 8: Debug Overlays (Optional)"
    )

    let colors = (
      rgb("#e6f3ff"),
      rgb("#fff3cd"),
      rgb("#f8d7da"),
      rgb("#e7e7ff"),
      rgb("#ffe6e6"),
      rgb("#f0f0f0")
    )

    let y = y - 1.0
    for (i, pass) in passes.enumerate() {
      rect((0.5, y), (width - 0.5, y - 0.7), stroke: (thickness: 0.6pt), fill: colors.at(i))
      content((1, y - 0.35), anchor: "west", text(weight: "bold", size: 7pt, pass))
      y = y - 0.8
    }
  }),
  caption: [渲染管线架构]
)

=== 图层管理系统

```rust
/// 图层管理器
pub struct LayerManager {
    layers: Vec<LayerConfig>,
    render_order: Vec<LayerId>,
}

/// 图层配置
pub struct LayerConfig {
    pub id: LayerId,
    pub name: &'static str,
    pub visible: bool,
    pub opacity: f32,
    pub z_order: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayerId {
    // 基础层
    Grid, Delaunay, Voronoi,
    // 地形层
    Heightmap, Contours, Hillshade,
    // 水系层
    Ocean, Lakes, Rivers,
    // 气候层
    Temperature, Precipitation, Biomes,
    // 政治层
    States, Provinces, Cultures, Religions,
    // 定居点层
    Burgs, Routes,
    // 标注层
    Labels,
    // 调试层
    CellIds, FlowDirection,
}
```

=== UI 模块 (`src/ui/`)

用户界面组件。

```
ui/
├── mod.rs
│
├── canvas/                  # 画布组件
│   ├── mod.rs
│   ├── canvas.rs           # Canvas 组件定义
│   ├── widget_impl.rs      # egui::Widget 实现
│   ├── state.rs            # CanvasState - 画布状态
│   └── input/              # 输入处理
│
├── map/                     # 地图 UI
├── panels/                  # 面板（工具栏、图层、信息、生成器）
├── tools/                   # 编辑工具
└── dialogs/                 # 对话框
```

== 着色器架构

=== 着色器文件

```
assets/shaders/
├── common.wgsl          # 公共函数和结构
├── points.wgsl          # 点渲染 (实例化三角形)
├── delaunay.wgsl        # Delaunay 边渲染 (线段)
├── voronoi.wgsl         # Voronoi 边渲染 (线段)
├── terrain.wgsl         # 地形填充渲染
├── water.wgsl           # 水体渲染
├── rivers.wgsl          # 河流渲染（变宽线段）
├── borders.wgsl         # 边界渲染（实线/虚线）
├── icons.wgsl           # 图标渲染（实例化四边形）
└── routes.wgsl          # 路线渲染
```

=== 通用 Uniform 结构

```wgsl
// common.wgsl
struct CanvasUniforms {
    canvas_pos: vec2<f32>,     // 画布左上角位置
    canvas_size: vec2<f32>,    // 画布尺寸
    translation: vec2<f32>,    // 平移
    scale: f32,                // 缩放
    time: f32,                 // 动画时间
}

struct RenderConfig {
    mode: u32,                 // 渲染模式
    highlight_id: u32,         // 高亮的特征 ID
    opacity: f32,              // 透明度
    _padding: f32,
}

fn world_to_ndc(pos: vec2<f32>, uniforms: CanvasUniforms) -> vec4<f32> {
    let screen = pos * uniforms.scale + uniforms.translation;
    let ndc = (screen - uniforms.canvas_pos) / uniforms.canvas_size * 2.0 - 1.0;
    return vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
}
```

=== 地形着色器示例

```wgsl
// terrain.wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) height: f32,
    @location(2) biome: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

// 高度图颜色映射
fn height_to_color(height: f32) -> vec4<f32> {
    if height < 0.08 {  // 深海
        return vec4<f32>(0.0, 0.1, 0.4, 1.0);
    } else if height < 0.15 { // 浅海
        return vec4<f32>(0.1, 0.3, 0.6, 1.0);
    } else if height < 0.2 { // 海岸
        return vec4<f32>(0.2, 0.5, 0.8, 1.0);
    } else if height < 0.4 { // 平原
        return vec4<f32>(0.3, 0.6, 0.3, 1.0);
    } else if height < 0.6 { // 丘陵
        return vec4<f32>(0.5, 0.5, 0.3, 1.0);
    } else if height < 0.8 { // 山地
        return vec4<f32>(0.4, 0.35, 0.3, 1.0);
    } else { // 雪山
        return vec4<f32>(0.9, 0.9, 0.95, 1.0);
    }
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = world_to_ndc(input.position, uniforms);
    output.color = height_to_color(input.height / 255.0);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color * config.opacity;
}
```

== 数据流图

=== 生成流程

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let center-x = 7
    let box-width = 4
    let box-height = 0.8
    let data-width = 4

    // Helper functions
    let draw-process = (x, y, name) => {
      rect((x - box-width/2, y), (x + box-width/2, y - box-height),
           stroke: (thickness: 0.8pt), fill: rgb("#e8f4f8"))
      content((x, y - box-height/2), text(weight: "bold", size: 8pt, name))
    }

    let draw-data = (x, y, name) => {
      rect((x - data-width/2, y), (x + data-width/2, y - box-height),
           stroke: (thickness: 0.8pt), fill: rgb("#d4edda"), radius: 0.1)
      content((x, y - box-height/2), text(size: 7pt, name))
    }

    // User config
    let y = 0
    content((center-x, y - 0.4), text(weight: "bold", size: 8pt, "用户配置"))

    // GeneratorPipeline
    let y = y - 1.2
    line((center-x, y + 0.4), (center-x, y + 0.1), mark: (end: "stealth"))
    rect((center-x - box-width/2, y), (center-x + box-width/2, y - 1.0),
         stroke: (thickness: 0.8pt), fill: rgb("#fff3cd"))
    content((center-x, y - 0.35), text(weight: "bold", size: 8pt, "GeneratorPipeline"))
    content((center-x, y - 0.65), text(size: 7pt, ".new(config)"))

    // Stage 0: Grid
    let y = y - 1.8
    line((center-x, y + 0.8), (center-x, y + 0.1), mark: (end: "stealth"))
    draw-process(center-x, y, "Stage 0: Grid")
    line((center-x + box-width/2, y - box-height/2), (center-x + box-width/2 + 0.5, y - box-height/2),
         mark: (end: "stealth"))
    draw-data(center-x + box-width/2 + data-width/2 + 0.5, y - box-height/2 + box-height/2,
              "Grid + Voronoi")

    // Stage 1: Heightmap
    let y = y - 1.5
    line((center-x, y + 0.7), (center-x, y + 0.1), mark: (end: "stealth"))
    draw-process(center-x, y, "Stage 1: Heightmap")
    line((center-x + box-width/2, y - box-height/2), (center-x + box-width/2 + 0.5, y - box-height/2),
         mark: (end: "stealth"))
    draw-data(center-x + box-width/2 + data-width/2 + 0.5, y - box-height/2 + box-height/2,
              "cells.height[]")

    // Stage 2: Coastline
    let y = y - 1.5
    line((center-x, y + 0.7), (center-x, y + 0.1), mark: (end: "stealth"))
    draw-process(center-x, y, "Stage 2: Coastline")
    line((center-x + box-width/2, y - box-height/2), (center-x + box-width/2 + 0.5, y - box-height/2),
         mark: (end: "stealth"))
    draw-data(center-x + box-width/2 + data-width/2 + 0.5, y - box-height/2 + box-height/2,
              "landmasses, islands")

    // More stages (dots)
    let y = y - 1.2
    line((center-x, y + 0.4), (center-x, y))
    content((center-x, y - 0.3), text(size: 8pt, "... (更多阶段) ..."))
    line((center-x, y - 0.6), (center-x, y - 1.0))

    // Complete Map
    let y = y - 1.6
    line((center-x, y + 0.6), (center-x, y + 0.1), mark: (end: "stealth"))
    rect((center-x - box-width/2, y), (center-x + box-width/2, y - box-height),
         stroke: (thickness: 0.8pt), fill: rgb("#d4edda"))
    content((center-x, y - box-height/2), text(weight: "bold", size: 8pt, "Complete Map"))

    // UI refresh arrow
    line((center-x + box-width/2 + 0.5, y - box-height/2),
         (center-x + box-width/2, y - box-height/2),
         mark: (end: "stealth"))
    content((center-x + box-width/2 + 1.3, y - box-height/2),
            text(size: 7pt, "触发 UI 刷新"))
  }),
  caption: [地图生成流程]
)

=== 渲染帧流程

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let width = 12

    // Step 1: Input Processing
    let y = 0
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "1. 输入处理"))

    rect((1, y - 0.8), (width - 0.5, y - 2.8), stroke: (thickness: 0.8pt), fill: rgb("#e8f4f8"))
    content((1.5, y - 1.3), anchor: "west", text(size: 7pt, "InputStateManager.update()"))
    line((3.5, y - 1.5), (3.5, y - 1.7), mark: (end: "stealth"))
    content((1.5, y - 1.9), anchor: "west", text(size: 7pt, "更新 CanvasState (平移/缩放)"))
    line((3.5, y - 2.1), (3.5, y - 2.3), mark: (end: "stealth"))
    content((1.5, y - 2.5), anchor: "west", text(size: 7pt, "工具输入处理 (笔刷、选择等)"))

    // Step 2: UI Construction
    let y = y - 3.5
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "2. UI 构建"))

    rect((1, y - 0.8), (width - 0.5, y - 2.8), stroke: (thickness: 0.8pt), fill: rgb("#fff3cd"))
    content((1.5, y - 1.1), anchor: "west", text(weight: "bold", size: 7pt, "egui 面板"))
    content((2, y - 1.5), anchor: "west", text(size: 7pt, "├ 工具栏面板"))
    content((2, y - 1.8), anchor: "west", text(size: 7pt, "├ 图层面板"))
    content((2, y - 2.1), anchor: "west", text(size: 7pt, "├ 信息面板"))
    content((2, y - 2.4), anchor: "west", text(size: 7pt, "└ 生成器面板"))

    // Step 3: Canvas Widget
    let y = y - 3.5
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "3. Canvas Widget"))

    rect((1, y - 0.8), (width - 0.5, y - 2.6), stroke: (thickness: 0.8pt), fill: rgb("#d4edda"))
    content((1.5, y - 1.1), anchor: "west", text(size: 7pt, "获取可见视口"))
    line((3, y - 1.3), (3, y - 1.5), mark: (end: "stealth"))
    content((1.5, y - 1.7), anchor: "west", text(size: 7pt, "LayerManager.get_visible_layers()"))
    line((3, y - 1.9), (3, y - 2.1), mark: (end: "stealth"))
    content((1.5, y - 2.3), anchor: "west", text(size: 7pt, "为每个可见图层添加 GPU 回调"))

    // Step 4: GPU Rendering
    let y = y - 3.3
    content((0.5, y - 0.4), anchor: "west", text(weight: "bold", size: 9pt, "4. GPU 渲染 (egui_wgpu 调用回调)"))

    rect((1, y - 0.8), (width - 0.5, y - 2.1), stroke: (thickness: 0.8pt), fill: rgb("#f8d7da"))
    content((1.5, y - 1.1), anchor: "west", text(weight: "bold", size: 7pt, "对每个 Callback:"))
    content((2, y - 1.5), anchor: "west", text(size: 7pt, "├ prepare() - 更新 GPU 缓冲区"))
    content((2, y - 1.8), anchor: "west", text(size: 7pt, "└ paint() - 执行渲染"))
  }),
  caption: [每帧更新循环]
)

== 扩展点

=== 添加新的生成阶段

+ 在 `src/generators/` 中创建新模块
+ 实现 `GeneratorStage` trait
+ 在 `GeneratorPipeline` 中注册新阶段
+ 更新 `GenerationStage` 枚举

=== 添加新的渲染图层

+ 在 `src/gpu/` 中创建新的渲染器模块
+ 创建对应的着色器文件
+ 实现 `MapLayer` trait
+ 在 `LayerManager` 中注册新图层
+ 在 Canvas 中添加回调

=== 添加新的特征类型

+ 在 `src/models/features/` 中定义新的特征结构
+ 在 `MapSystem` 中添加存储
+ 实现相应的生成器
+ 添加渲染支持
+ 添加编辑 UI

=== 添加新的编辑工具

+ 在 `src/ui/tools/` 中创建工具模块
+ 实现 `Tool` trait
+ 在工具栏中注册
+ 处理输入事件
+ 更新地图数据

== 性能优化策略

=== 已实现

- *视口裁剪*: `helpers::get_visible_indices()` 只渲染可见区域
- *GPU 渲染*: 使用 wgpu 进行硬件加速
- *并行计算*: 使用 rayon 进行 CPU 并行
- *空间索引*: 网格索引和边索引加速查询

=== 待实现

- *LOD*: 根据缩放级别动态调整渲染细节
  - 远景：只渲染大区域边界
  - 近景：渲染所有细节
  
- *分块加载*: 超大地图按区块加载
  - 将地图划分为 Chunk
  - 只加载可见 Chunk
  
- *增量更新*: 只更新变化的部分
  - 脏标记系统
  - 局部重新生成
  
- *Buffer 复用*: 避免每帧重新分配 GPU Buffer
  - Buffer 池化
  - 预分配足够容量
  
- *计算着色器*: GPU 加速生成算法
  - 噪声生成
  - 流量计算
  - 扩张模拟

=== 内存优化

- 使用紧凑数据类型（u8, u16 代替 u32/usize）
- 避免不必要的数据复制
- 延迟加载非必要数据
- 及时释放临时数据

== 测试策略

=== 单元测试

- 几何算法测试（Delaunay、Voronoi）
- 生成器算法测试
- 数据结构测试

=== 集成测试

- 完整生成流程测试
- 渲染管线测试
- UI 交互测试

=== 性能测试

- Benchmark 测试（使用 criterion）
- 大规模数据测试
- 内存使用测试

=== 视觉测试

- 渲染结果截图对比
- 不同配置下的生成效果
