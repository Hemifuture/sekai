# Sekai 架构设计文档

## 一、系统架构总览

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              Application Layer                                   │
│  ┌───────────────────────────────────────────────────────────────────────────┐  │
│  │                         TemplateApp (app.rs)                               │  │
│  │  - 应用生命周期管理                                                         │  │
│  │  - 资源初始化                                                               │  │
│  │  - UI 布局                                                                  │  │
│  └───────────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────────┘
                                       │
         ┌─────────────────────────────┼─────────────────────────────┐
         ▼                             ▼                             ▼
┌─────────────────┐      ┌──────────────────────┐      ┌─────────────────┐
│    UI Layer     │      │    Generator Layer   │      │   GPU Layer     │
│                 │      │                      │      │                 │
│  Canvas         │      │  GeneratorPipeline   │      │  Renderers      │
│  InputManager   │◄────►│  HeightmapGenerator  │◄────►│  Shaders        │
│  Panels         │      │  RiverGenerator      │      │  Pipelines      │
│  Dialogs        │      │  StateGenerator      │      │                 │
│                 │      │  ...                 │      │                 │
└─────────────────┘      └──────────────────────┘      └─────────────────┘
         │                             │                             │
         └─────────────────────────────┼─────────────────────────────┘
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              Model Layer                                         │
│  ┌───────────────────────────────────────────────────────────────────────────┐  │
│  │                           MapSystem                                        │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │  │
│  │  │    Grid      │  │  CellsData   │  │  EdgesData   │  │   Features   │   │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘   │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                     │  │
│  │  │   Delaunay   │  │   Voronoi    │  │ SpatialIndex │                     │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘                     │  │
│  └───────────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                             Resource Layer                                       │
│  ┌───────────────────────────────────────────────────────────────────────────┐  │
│  │  Resource<T> - 线程安全的共享资源容器 (Arc<RwLock<T>>)                      │  │
│  │  - CanvasStateResource                                                      │  │
│  │  - MapSystemResource                                                        │  │
│  │  - *RendererResource                                                        │  │
│  └───────────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## 二、分层数据架构

### 2.1 地图数据层次

地图数据按照逻辑依赖关系分层，每一层依赖于下层的数据：

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│  Layer 7: 标注层 (Labels)                                                        │
│  - 地名、城市名、区域名称的位置和样式                                               │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Layer 6: 路线层 (Routes)                                                        │
│  - 道路网络、航线、贸易路线                                                        │
│  - 依赖: 城镇、地形、河流                                                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Layer 5: 城镇层 (Burgs)                                                         │
│  - 城市、城镇、村庄位置和属性                                                      │
│  - 依赖: 国家、地形、河流、人口                                                    │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Layer 4: 政治层 (Politics)                                                      │
│  ├── States (国家)                                                               │
│  ├── Provinces (省份)                                                            │
│  └── Religions (宗教)                                                            │
│  - 依赖: 文化、生物群落、人口                                                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Layer 3: 人文层 (Demographics)                                                  │
│  ├── Cultures (文化区域)                                                          │
│  └── Population (人口分布)                                                        │
│  - 依赖: 生物群落、水系、地形                                                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Layer 2: 气候层 (Climate)                                                       │
│  ├── Temperature (温度)                                                          │
│  ├── Precipitation (降水)                                                        │
│  └── Biomes (生物群落)                                                            │
│  - 依赖: 高度、纬度、洋流                                                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Layer 1: 水系层 (Hydrography)                                                   │
│  ├── Rivers (河流)                                                               │
│  ├── Lakes (湖泊)                                                                │
│  └── Coastline (海岸线)                                                           │
│  - 依赖: 高度图、降水                                                             │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Layer 0: 地形层 (Terrain)                                                       │
│  ├── Heightmap (高度图)                                                          │
│  └── Land/Sea (海陆分布)                                                          │
│  - 基础层，无依赖                                                                 │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Base: 几何层 (Geometry)                                                          │
│  ├── Grid Points                                                                 │
│  ├── Delaunay Triangulation                                                      │
│  └── Voronoi Diagram                                                             │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 数据依赖图

```
                    ┌───────────┐
                    │   Grid    │
                    └─────┬─────┘
                          │
                    ┌─────▼─────┐
                    │ Delaunay  │
                    └─────┬─────┘
                          │
                    ┌─────▼─────┐
                    │  Voronoi  │
                    └─────┬─────┘
                          │
                    ┌─────▼─────┐
                    │ Heightmap │
                    └─────┬─────┘
                          │
          ┌───────────────┼───────────────┐
          │               │               │
    ┌─────▼─────┐   ┌─────▼─────┐   ┌─────▼─────┐
    │ Coastline │   │   Temp    │   │ Precipit. │
    └─────┬─────┘   └─────┬─────┘   └─────┬─────┘
          │               │               │
          │         ┌─────▼─────┐         │
          │         │  Climate  │◄────────┘
          │         └─────┬─────┘
          │               │
    ┌─────▼─────┐   ┌─────▼─────┐
    │  Rivers   │   │  Biomes   │
    └─────┬─────┘   └─────┬─────┘
          │               │
          └───────┬───────┘
                  │
            ┌─────▼─────┐
            │Population │
            └─────┬─────┘
                  │
            ┌─────▼─────┐
            │ Cultures  │
            └─────┬─────┘
                  │
            ┌─────▼─────┐
            │  States   │
            └─────┬─────┘
                  │
          ┌───────┼───────┐
          │       │       │
    ┌─────▼───┐ ┌─▼─────┐ ┌▼────────┐
    │Provinces│ │Burgs  │ │Religions│
    └─────────┘ └───┬───┘ └─────────┘
                    │
              ┌─────▼─────┐
              │  Routes   │
              └─────┬─────┘
                    │
              ┌─────▼─────┐
              │  Labels   │
              └───────────┘
```

---

## 三、生成器管线 (Generator Pipeline)

### 3.1 管线架构

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
    fn execute(&self, map: &mut MapSystem, config: &GeneratorConfig) -> Result<(), GeneratorError>;
    
    /// 获取进度（0.0 - 1.0）
    fn progress(&self) -> f32;
}
```

### 3.2 生成阶段定义

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           Generator Pipeline                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Stage 0: GridGenerator                                                          │
│  ├── 输入: MapConfig (width, height, spacing)                                    │
│  ├── 输出: Grid (points), Delaunay (triangles), Voronoi (cells)                  │
│  └── 算法: Jittered Grid + Delaunator                                            │
│                                                                                  │
│  Stage 1: TectonicGenerator (板块构造高度图生成)                                  │
│  ├── 输入: Voronoi cells, neighbors                                              │
│  ├── 输出: TectonicPlates[], CellsData.height[], plate_id[]                      │
│  ├── 算法: Plate Tectonics Simulation                                            │
│  │   ├── 1. 生成板块 (Voronoi 划分)                                              │
│  │   ├── 2. 分配运动向量 (方向 + 速度)                                           │
│  │   ├── 3. 边界分析 (汇聚/分离/转换)                                            │
│  │   ├── 4. 迭代模拟 (碰撞隆起、俯冲下沉、裂谷形成)                               │
│  │   ├── 5. 地壳均衡调整 (Isostasy)                                              │
│  │   └── 6. 后处理 (噪声细节 + 平滑)                                             │
│  └── 参数: plate_count, continental_ratio, iterations, collision_rate            │
│                                                                                  │
│  Stage 2: CoastlineGenerator                                                     │
│  ├── 输入: height[]                                                              │
│  ├── 输出: Landmasses[], Islands[], coastline_cells[]                            │
│  ├── 算法: Flood Fill + Connected Component Analysis                             │
│  └── 参数: sea_level (default: 20)                                               │
│                                                                                  │
│  Stage 3: ClimateGenerator                                                       │
│  ├── 输入: height[], latitude (y-coordinate)                                     │
│  ├── 输出: CellsData.temperature[], CellsData.precipitation[]                    │
│  ├── 算法: Latitude-based + Altitude modifier + Wind simulation                  │
│  └── 参数: temperature_scale, precipitation_scale, wind_direction                │
│                                                                                  │
│  Stage 4: RiverGenerator                                                         │
│  ├── 输入: height[], precipitation[]                                             │
│  ├── 输出: Rivers[], Lakes[], EdgesData.river_id[]                               │
│  ├── 算法: Flow Accumulation + Threshold Detection                               │
│  └── 参数: river_threshold, lake_threshold                                       │
│                                                                                  │
│  Stage 5: BiomeGenerator                                                         │
│  ├── 输入: temperature[], precipitation[], height[]                              │
│  ├── 输出: CellsData.biome[]                                                     │
│  ├── 算法: Whittaker Biome Classification                                        │
│  └── 参数: biome_definitions[]                                                   │
│                                                                                  │
│  Stage 6: PopulationGenerator                                                    │
│  ├── 输入: biome[], rivers[], coastline[]                                        │
│  ├── 输出: CellsData.population[]                                                │
│  ├── 算法: Suitability Scoring + Distribution                                    │
│  └── 参数: total_population, density_factor                                      │
│                                                                                  │
│  Stage 7: CultureGenerator                                                       │
│  ├── 输入: population[], biome[], rivers[], mountains[]                          │
│  ├── 输出: Cultures[], CellsData.culture[]                                       │
│  ├── 算法: Seed Placement + Expansion with Barriers                              │
│  └── 参数: culture_count, expansion_rate                                         │
│                                                                                  │
│  Stage 8: StateGenerator                                                         │
│  ├── 输入: culture[], population[], biome[]                                      │
│  ├── 输出: States[], CellsData.state[]                                           │
│  ├── 算法: Capital Placement + Expansion + Border Stabilization                  │
│  └── 参数: state_count, capital_criteria                                         │
│                                                                                  │
│  Stage 9: ProvinceGenerator                                                      │
│  ├── 输入: state[], population[]                                                 │
│  ├── 输出: Provinces[], CellsData.province[]                                     │
│  ├── 算法: Subdivision based on population and geography                         │
│  └── 参数: province_size_target                                                  │
│                                                                                  │
│  Stage 10: BurgGenerator                                                         │
│  ├── 输入: state[], population[], rivers[], coastline[]                          │
│  ├── 输出: Burgs[]                                                               │
│  ├── 算法: Suitability Scoring + Hierarchical Placement                          │
│  └── 参数: city_count, town_count                                                │
│                                                                                  │
│  Stage 11: ReligionGenerator                                                     │
│  ├── 输入: culture[], state[], burgs[]                                           │
│  ├── 输出: Religions[], CellsData.religion[]                                     │
│  ├── 算法: Origin + Spread Model                                                 │
│  └── 参数: religion_count                                                        │
│                                                                                  │
│  Stage 12: RouteGenerator                                                        │
│  ├── 输入: burgs[], state[], height[], rivers[]                                  │
│  ├── 输出: Routes[]                                                              │
│  ├── 算法: A* Pathfinding with terrain costs                                     │
│  └── 参数: road_density                                                          │
│                                                                                  │
│  Stage 13: NameGenerator                                                         │
│  ├── 输入: All features (rivers, burgs, states, etc.)                            │
│  ├── 输出: Names for all features                                                │
│  ├── 算法: Markov Chain / Template-based                                         │
│  └── 参数: culture_name_bases[]                                                  │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 3.3 生成配置

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
    /// 俯冲海沟深度速率
    pub subduction_depth_rate: f32,
    /// 分离裂谷深度速率
    pub rift_depth_rate: f32,
    /// 均衡调整速率
    pub isostatic_rate: f32,
    /// 板块边缘影响范围（单元格数）
    pub boundary_width: f32,
    /// 噪声细节强度
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
    
    // 生成选项
    pub generate_rivers: bool,
    pub generate_states: bool,
    pub generate_religions: bool,
    pub generate_routes: bool,
}
```

---

## 四、核心模块详解

### 4.1 几何计算模块 (`src/delaunay/`)

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

#### 数据流

```
Points (Vec<Pos2>)
        │
        ▼
┌───────────────────┐
│    triangulate()  │  ─── 使用 delaunator 库
└───────────────────┘
        │
        ▼
Triangle Indices (Vec<usize>)
        │
        ▼
┌────────────────────────┐
│ compute_indexed_voronoi│
└────────────────────────┘
        │
        ▼
IndexedVoronoiDiagram
├── vertices: Vec<Pos2>      # Voronoi 顶点
├── indices: Vec<usize>      # 边索引
└── cells: Vec<VoronoiCell>  # 单元格
```

### 4.2 生成器模块 (`src/generators/`)

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

### 4.3 数据模型模块 (`src/models/`)

定义地图的核心数据结构。

```
models/
├── mod.rs
├── map_layer.rs          # 图层 trait (简化版)
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

#### MapSystem 结构（扩展版）

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

/// 边数据
#[derive(Debug, Clone)]
pub struct EdgesData {
    pub river_id: Vec<u16>,        // 河流 ID (0=无)
    pub river_width: Vec<u8>,      // 河流宽度
    pub border_type: Vec<BorderType>, // 边界类型
}

#[derive(Debug, Clone, Copy)]
pub enum BorderType {
    None,
    State,           // 国境
    Province,        // 省界
    Culture,         // 文化边界
}

#[derive(Debug, Clone, Copy)]
pub enum GenerationStage {
    Empty,
    GridGenerated,
    HeightmapGenerated,
    CoastlineDetected,
    RiversGenerated,
    ClimateCalculated,
    BiomesAssigned,
    PopulationDistributed,
    CulturesGenerated,
    StatesGenerated,
    ProvincesGenerated,
    BurgsPlaced,
    ReligionsGenerated,
    RoutesGenerated,
    NamesGenerated,
    Complete,
}
```

### 4.4 GPU 渲染模块 (`src/gpu/`)

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
│   ├── mod.rs
│   ├── renderer.rs
│   └── callback.rs
│
├── delaunay/             # Delaunay 边渲染
│   ├── mod.rs
│   ├── renderer.rs
│   ├── callback.rs
│   └── helpers.rs
│
├── voronoi/              # Voronoi 边渲染
│   ├── mod.rs
│   ├── renderer.rs
│   └── callback.rs
│
├── terrain/              # 地形渲染（新增）
│   ├── mod.rs
│   ├── heightmap_renderer.rs   # 高度图着色
│   ├── contour_renderer.rs     # 等高线
│   └── hillshade_renderer.rs   # 山体阴影
│
├── water/                # 水系渲染（新增）
│   ├── mod.rs
│   ├── ocean_renderer.rs       # 海洋
│   ├── river_renderer.rs       # 河流
│   └── lake_renderer.rs        # 湖泊
│
├── borders/              # 边界渲染（新增）
│   ├── mod.rs
│   ├── state_border.rs         # 国境线
│   └── province_border.rs      # 省界
│
├── icons/                # 图标渲染（新增）
│   ├── mod.rs
│   └── burg_renderer.rs        # 城镇图标
│
└── labels/               # 标注渲染（新增）
    ├── mod.rs
    └── text_renderer.rs
```

#### 渲染管线架构

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              Render Pipeline                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Pass 1: Terrain Fill (Fragment Shader)                                          │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  Input: cell vertices, height/biome data                                    │ │
│  │  Output: Colored cell polygons                                              │ │
│  │  Shader: terrain.wgsl                                                       │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Pass 2: Water Bodies                                                            │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  2a: Ocean - Blue gradient with depth                                       │ │
│  │  2b: Lakes - Solid blue fill                                                │ │
│  │  Shader: water.wgsl                                                         │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Pass 3: Rivers (Line Rendering)                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  Input: river paths, widths                                                 │ │
│  │  Output: Variable-width lines                                               │ │
│  │  Shader: rivers.wgsl                                                        │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Pass 4: Borders (Line Rendering)                                                │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  4a: State borders - Solid thick lines                                      │ │
│  │  4b: Province borders - Dashed lines                                        │ │
│  │  4c: Culture borders - Dotted lines                                         │ │
│  │  Shader: borders.wgsl                                                       │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Pass 5: Routes (Line Rendering)                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  Input: route paths, types                                                  │ │
│  │  Output: Styled road/sea routes                                             │ │
│  │  Shader: routes.wgsl                                                        │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Pass 6: Icons (Instanced Quads)                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  Input: burg positions, types, sizes                                        │ │
│  │  Output: City/town icons                                                    │ │
│  │  Shader: icons.wgsl                                                         │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Pass 7: Labels (Text Rendering)                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  Input: label positions, text, styles                                       │ │
│  │  Output: Rendered text (via egui)                                           │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Pass 8: Debug Overlays (Optional)                                               │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │  - Voronoi edges                                                            │ │
│  │  - Delaunay triangles                                                       │ │
│  │  - Grid points                                                              │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 4.5 图层管理系统

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
    Grid,
    Delaunay,
    Voronoi,
    
    // 地形层
    Heightmap,
    Contours,
    Hillshade,
    
    // 水系层
    Ocean,
    Lakes,
    Rivers,
    
    // 气候层
    Temperature,
    Precipitation,
    Biomes,
    
    // 政治层
    States,
    Provinces,
    Cultures,
    Religions,
    
    // 定居点层
    Burgs,
    Routes,
    
    // 标注层
    Labels,
    
    // 调试层
    CellIds,
    FlowDirection,
}

impl LayerManager {
    pub fn default_layers() -> Self {
        Self {
            layers: vec![
                LayerConfig { id: LayerId::Heightmap, name: "高度图", visible: true, opacity: 1.0, z_order: 0 },
                LayerConfig { id: LayerId::Ocean, name: "海洋", visible: true, opacity: 1.0, z_order: 10 },
                LayerConfig { id: LayerId::Lakes, name: "湖泊", visible: true, opacity: 1.0, z_order: 11 },
                LayerConfig { id: LayerId::Rivers, name: "河流", visible: true, opacity: 1.0, z_order: 20 },
                LayerConfig { id: LayerId::Biomes, name: "生物群落", visible: false, opacity: 0.8, z_order: 30 },
                LayerConfig { id: LayerId::States, name: "国家", visible: true, opacity: 0.6, z_order: 40 },
                LayerConfig { id: LayerId::Provinces, name: "省份", visible: false, opacity: 0.4, z_order: 41 },
                LayerConfig { id: LayerId::Routes, name: "道路", visible: true, opacity: 1.0, z_order: 50 },
                LayerConfig { id: LayerId::Burgs, name: "城镇", visible: true, opacity: 1.0, z_order: 60 },
                LayerConfig { id: LayerId::Labels, name: "标注", visible: true, opacity: 1.0, z_order: 70 },
                LayerConfig { id: LayerId::Voronoi, name: "Voronoi网格", visible: false, opacity: 0.3, z_order: 100 },
            ],
            render_order: vec![/* sorted by z_order */],
        }
    }
}
```

### 4.6 UI 模块 (`src/ui/`)

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
│   ├── helpers.rs          # 辅助绘制函数
│   └── input/
│       ├── mod.rs
│       ├── state_manager.rs
│       ├── input_state.rs
│       └── button_state.rs
│
├── map/                     # 地图 UI
│   ├── mod.rs
│   └── map_impl.rs
│
├── panels/                  # 面板
│   ├── mod.rs
│   ├── toolbar.rs          # 工具栏
│   ├── layers.rs           # 图层面板
│   ├── info.rs             # 信息面板
│   ├── generator.rs        # 生成器面板
│   └── feature_editor.rs   # 特征编辑面板
│
├── tools/                   # 编辑工具
│   ├── mod.rs
│   ├── brush.rs            # 笔刷工具
│   ├── eraser.rs           # 橡皮工具
│   ├── select.rs           # 选择工具
│   ├── fill.rs             # 填充工具
│   └── river_draw.rs       # 河流绘制
│
└── dialogs/                 # 对话框
    ├── mod.rs
    ├── export.rs           # 导出对话框
    ├── import.rs           # 导入对话框
    └── settings.rs         # 设置对话框
```

---

## 五、关键算法

### 5.1 高度图生成

```rust
/// 多层噪声叠加生成高度图
fn generate_heightmap(cells: &[Pos2], config: &HeightmapConfig) -> Vec<u8> {
    let noise = Fbm::<Perlin>::new(config.seed);
    
    cells.par_iter().map(|pos| {
        let mut height = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = config.base_frequency;
        
        // 多层噪声叠加
        for _ in 0..config.octaves {
            let nx = pos.x * frequency / config.width as f32;
            let ny = pos.y * frequency / config.height as f32;
            height += noise.get([nx as f64, ny as f64]) * amplitude;
            amplitude *= config.persistence;
            frequency *= config.lacunarity;
        }
        
        // 归一化到 0-255
        let normalized = (height + 1.0) / 2.0;
        (normalized * 255.0).clamp(0.0, 255.0) as u8
    }).collect()
}
```

### 5.2 河流生成

```rust
/// 基于流量累积的河流生成
fn generate_rivers(map: &mut MapSystem) -> Vec<River> {
    // 1. 计算流向（指向最低邻居）
    let flow_direction = compute_flow_direction(&map.cells_data.height, &map.voronoi);
    
    // 2. 按高度排序单元格（从高到低）
    let sorted_cells = sort_by_height_descending(&map.cells_data.height);
    
    // 3. 累积流量
    let mut flux = vec![1u16; map.cells_data.height.len()];
    for &cell in &sorted_cells {
        if let Some(downstream) = flow_direction[cell] {
            flux[downstream] += flux[cell];
        }
    }
    
    // 4. 提取超过阈值的路径作为河流
    let river_threshold = config.river_threshold;
    let mut rivers = Vec::new();
    
    for (cell, &f) in flux.iter().enumerate() {
        if f >= river_threshold && !visited[cell] {
            let river = trace_river_path(cell, &flow_direction, &flux);
            rivers.push(river);
        }
    }
    
    rivers
}
```

### 5.3 国家生成

```rust
/// 国家生成算法
fn generate_states(map: &mut MapSystem, config: &StateConfig) -> Vec<State> {
    // 1. 选择首都位置
    let capitals = select_capital_locations(map, config.state_count);
    
    // 2. 初始化国家
    let mut states: Vec<State> = capitals.iter().enumerate().map(|(i, &cell)| {
        State {
            id: i as u16 + 1,
            capital_cell: cell,
            cells: vec![cell],
            ..Default::default()
        }
    }).collect();
    
    // 3. 扩张模拟
    let mut frontier: BinaryHeap<ExpansionCandidate> = /* init */;
    
    while !frontier.is_empty() {
        let candidate = frontier.pop().unwrap();
        
        if map.cells_data.state[candidate.cell] != 0 {
            continue; // 已被占领
        }
        
        // 分配给该国家
        map.cells_data.state[candidate.cell] = candidate.state_id;
        states[candidate.state_id as usize - 1].cells.push(candidate.cell);
        
        // 添加邻居到边界
        for neighbor in get_neighbors(candidate.cell, &map.voronoi) {
            if map.cells_data.state[neighbor] == 0 && is_land(neighbor, map) {
                let cost = calculate_expansion_cost(candidate.cell, neighbor, map);
                frontier.push(ExpansionCandidate {
                    cell: neighbor,
                    state_id: candidate.state_id,
                    priority: candidate.priority + cost,
                });
            }
        }
    }
    
    // 4. 边界优化（消除飞地等）
    optimize_borders(&mut states, map);
    
    states
}
```

---

## 六、着色器架构

### 6.1 着色器文件

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

### 6.2 通用 Uniform 结构

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

### 6.3 地形着色器示例

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

---

## 七、数据流图

### 7.1 生成流程

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           Map Generation Flow                                    │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  用户配置                                                                        │
│      │                                                                           │
│      ▼                                                                           │
│  ┌─────────────────────┐                                                         │
│  │ GeneratorPipeline   │                                                         │
│  │     .new(config)    │                                                         │
│  └──────────┬──────────┘                                                         │
│             │                                                                    │
│             ▼                                                                    │
│  ┌─────────────────────┐      ┌─────────────────────┐                            │
│  │  Stage 0: Grid      │─────►│   Grid + Voronoi    │                            │
│  └──────────┬──────────┘      └─────────────────────┘                            │
│             │                                                                    │
│             ▼                                                                    │
│  ┌─────────────────────┐      ┌─────────────────────┐                            │
│  │ Stage 1: Heightmap  │─────►│   cells.height[]    │                            │
│  └──────────┬──────────┘      └─────────────────────┘                            │
│             │                                                                    │
│             ▼                                                                    │
│  ┌─────────────────────┐      ┌─────────────────────┐                            │
│  │ Stage 2: Coastline  │─────►│ landmasses, islands │                            │
│  └──────────┬──────────┘      └─────────────────────┘                            │
│             │                                                                    │
│             ▼                                                                    │
│  ┌─────────────────────┐      ┌─────────────────────┐                            │
│  │  Stage 3: Climate   │─────►│  temp[], precip[]   │                            │
│  └──────────┬──────────┘      └─────────────────────┘                            │
│             │                                                                    │
│             ▼                                                                    │
│  ┌─────────────────────┐      ┌─────────────────────┐                            │
│  │  Stage 4: Rivers    │─────►│   rivers[], lakes[] │                            │
│  └──────────┬──────────┘      └─────────────────────┘                            │
│             │                                                                    │
│             ▼                                                                    │
│  ┌─────────────────────┐      ┌─────────────────────┐                            │
│  │  Stage 5: Biomes    │─────►│   cells.biome[]     │                            │
│  └──────────┬──────────┘      └─────────────────────┘                            │
│             │                                                                    │
│             ▼                                                                    │
│         ... (更多阶段) ...                                                        │
│             │                                                                    │
│             ▼                                                                    │
│  ┌─────────────────────┐                                                         │
│  │   Complete Map      │◄──── 触发 UI 刷新                                        │
│  └─────────────────────┘                                                         │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 7.2 渲染帧流程

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              每帧更新循环                                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  1. 输入处理                                                                     │
│     InputStateManager.update()                                                   │
│         │                                                                        │
│         ▼                                                                        │
│     更新 CanvasState (平移/缩放)                                                 │
│         │                                                                        │
│         ▼                                                                        │
│     工具输入处理 (笔刷、选择等)                                                   │
│                                                                                  │
│  2. UI 构建                                                                      │
│     ┌─────────────────────────────────────────────────────────────────────────┐ │
│     │  egui 面板                                                               │ │
│     │  ├── 工具栏面板                                                          │ │
│     │  ├── 图层面板                                                            │ │
│     │  ├── 信息面板                                                            │ │
│     │  └── 生成器面板                                                          │ │
│     └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  3. Canvas Widget                                                                │
│     ┌─────────────────────────────────────────────────────────────────────────┐ │
│     │  获取可见视口                                                            │ │
│     │      │                                                                   │ │
│     │      ▼                                                                   │ │
│     │  LayerManager.get_visible_layers()                                       │ │
│     │      │                                                                   │ │
│     │      ▼                                                                   │ │
│     │  为每个可见图层添加 GPU 回调                                              │ │
│     │  ├── TerrainCallback                                                     │ │
│     │  ├── WaterCallback                                                       │ │
│     │  ├── RiverCallback                                                       │ │
│     │  ├── BorderCallback                                                      │ │
│     │  ├── BurgCallback                                                        │ │
│     │  └── LabelCallback                                                       │ │
│     └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  4. GPU 渲染 (egui_wgpu 调用回调)                                                │
│     ┌─────────────────────────────────────────────────────────────────────────┐ │
│     │  对每个 Callback:                                                        │ │
│     │      │                                                                   │ │
│     │      ├─► prepare() - 更新 GPU 缓冲区                                     │ │
│     │      │   - 视口裁剪                                                      │ │
│     │      │   - 数据上传                                                      │ │
│     │      │                                                                   │ │
│     │      └─► paint() - 执行渲染                                              │ │
│     │          - 设置管线                                                      │ │
│     │          - 绑定缓冲区                                                    │ │
│     │          - 绘制调用                                                      │ │
│     └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## 八、扩展点

### 8.1 添加新的生成阶段

1. 在 `src/generators/` 中创建新模块
2. 实现 `GeneratorStage` trait
3. 在 `GeneratorPipeline` 中注册新阶段
4. 更新 `GenerationStage` 枚举

### 8.2 添加新的渲染图层

1. 在 `src/gpu/` 中创建新的渲染器模块
2. 创建对应的着色器文件
3. 实现 `MapLayer` trait
4. 在 `LayerManager` 中注册新图层
5. 在 Canvas 中添加回调

### 8.3 添加新的特征类型

1. 在 `src/models/features/` 中定义新的特征结构
2. 在 `MapSystem` 中添加存储
3. 实现相应的生成器
4. 添加渲染支持
5. 添加编辑 UI

### 8.4 添加新的编辑工具

1. 在 `src/ui/tools/` 中创建工具模块
2. 实现 `Tool` trait
3. 在工具栏中注册
4. 处理输入事件
5. 更新地图数据

---

## 九、性能优化策略

### 9.1 已实现

- **视口裁剪**: `helpers::get_visible_indices()` 只渲染可见区域
- **GPU 渲染**: 使用 wgpu 进行硬件加速
- **并行计算**: 使用 rayon 进行 CPU 并行
- **空间索引**: 网格索引和边索引加速查询

### 9.2 待实现

- **LOD**: 根据缩放级别动态调整渲染细节
  - 远景：只渲染大区域边界
  - 近景：渲染所有细节
  
- **分块加载**: 超大地图按区块加载
  - 将地图划分为 Chunk
  - 只加载可见 Chunk
  
- **增量更新**: 只更新变化的部分
  - 脏标记系统
  - 局部重新生成
  
- **Buffer 复用**: 避免每帧重新分配 GPU Buffer
  - Buffer 池化
  - 预分配足够容量
  
- **计算着色器**: GPU 加速生成算法
  - 噪声生成
  - 流量计算
  - 扩张模拟

### 9.3 内存优化

- 使用紧凑数据类型（u8, u16 代替 u32/usize）
- 避免不必要的数据复制
- 延迟加载非必要数据
- 及时释放临时数据

---

## 十、测试策略

### 10.1 单元测试

- 几何算法测试（Delaunay、Voronoi）
- 生成器算法测试
- 数据结构测试

### 10.2 集成测试

- 完整生成流程测试
- 渲染管线测试
- UI 交互测试

### 10.3 性能测试

- Benchmark 测试（使用 criterion）
- 大规模数据测试
- 内存使用测试

### 10.4 视觉测试

- 渲染结果截图对比
- 不同配置下的生成效果

---

*文档版本: 2.0*
*最后更新: 2026-01-12*
