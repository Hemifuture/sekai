# Sekai 架构设计文档

## 一、系统架构总览

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Application Layer                          │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │                        TemplateApp (app.rs)                      │ │
│  │  - 应用生命周期管理                                               │ │
│  │  - 资源初始化                                                     │ │
│  │  - UI 布局                                                        │ │
│  └─────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
                                    │
          ┌─────────────────────────┼─────────────────────────┐
          ▼                         ▼                         ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   UI Layer      │     │   Model Layer   │     │   GPU Layer     │
│                 │     │                 │     │                 │
│  Canvas         │     │  MapSystem      │     │  Renderers      │
│  InputManager   │◄───►│  Grid           │◄───►│  Shaders        │
│  Widgets        │     │  CellsData      │     │  Pipelines      │
│                 │     │  Features       │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
          │                         │                         │
          └─────────────────────────┼─────────────────────────┘
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          Resource Layer                              │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │  Resource<T> - 线程安全的共享资源容器 (Arc<RwLock<T>>)           │ │
│  │  - CanvasStateResource                                           │ │
│  │  - MapSystemResource                                             │ │
│  │  - *RendererResource                                             │ │
│  └─────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 二、核心模块详解

### 2.1 几何计算模块 (`src/delaunay/`)

负责地图的几何基础计算。

```
delaunay/
├── mod.rs           # 模块导出
├── delaunay.rs      # Delaunay 三角剖分核心算法
├── voronoi.rs       # Voronoi 图生成
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

### 2.2 GPU 渲染模块 (`src/gpu/`)

负责所有图形渲染，使用 wgpu。

```
gpu/
├── mod.rs
├── canvas_uniform.rs     # 画布变换 Uniform 结构
├── map_renderer.rs       # 渲染管线创建工厂
├── pipelines.rs          # 管线配置
├── helpers.rs            # 视口裁剪等辅助函数
│
├── points_renderer.rs    # 点渲染器
├── points_callback.rs    # 点渲染回调
│
├── delaunay/
│   ├── delaunay_renderer.rs  # Delaunay 边渲染器
│   ├── delaunay_callback.rs  # Delaunay 渲染回调
│   └── helpers.rs
│
└── voronoi/
    ├── voronoi_renderer.rs   # Voronoi 边渲染器
    └── voronoi_callback.rs   # Voronoi 渲染回调
```

#### 渲染管线架构

```
┌─────────────────────────────────────────────────────────────┐
│                    Canvas Widget                             │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │                   egui Painter                           │ │
│  │                        │                                 │ │
│  │      ┌─────────────────┼─────────────────┐               │ │
│  │      ▼                 ▼                 ▼               │ │
│  │  ┌────────┐      ┌──────────┐      ┌─────────┐           │ │
│  │  │Voronoi │      │ Delaunay │      │ Points  │           │ │
│  │  │Callback│      │ Callback │      │Callback │           │ │
│  │  └────┬───┘      └────┬─────┘      └────┬────┘           │ │
│  │       │               │                 │                │ │
│  └───────┼───────────────┼─────────────────┼────────────────┘ │
│          ▼               ▼                 ▼                  │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │                   wgpu Render Pass                        │ │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐           │ │
│  │  │ Voronoi    │  │ Delaunay   │  │ Points     │           │ │
│  │  │ Pipeline   │  │ Pipeline   │  │ Pipeline   │           │ │
│  │  │ (Lines)    │  │ (Lines)    │  │ (Triangles)│           │ │
│  │  └────────────┘  └────────────┘  └────────────┘           │ │
│  └──────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

#### CanvasUniforms 结构

```rust
#[repr(C)]
struct CanvasUniforms {
    canvas_x: f32,        // 画布左上角 X
    canvas_y: f32,        // 画布左上角 Y
    canvas_width: f32,    // 画布宽度
    canvas_height: f32,   // 画布高度
    translation_x: f32,   // 平移 X
    translation_y: f32,   // 平移 Y
    scale: f32,           // 缩放
    padding: [f32; 3],    // 对齐填充
}
```

### 2.3 数据模型模块 (`src/models/`)

定义地图的核心数据结构。

```
models/
├── mod.rs
├── map_layer.rs       # 图层 trait (简化版)
│
└── map/
    ├── mod.rs
    ├── grid.rs        # 网格生成
    ├── system.rs      # MapSystem - 核心数据容器
    ├── cells_data.rs  # 单元格属性数据
    └── feature.rs     # 特征系统定义
```

#### MapSystem 结构

```rust
struct MapSystem {
    config: MapConfig,              // 配置
    grid: Grid,                     // 网格点
    delaunay: Vec<usize>,           // 三角形索引
    voronoi: IndexedVoronoiDiagram, // Voronoi 图
    cells_data: CellsData,          // 单元格数据
}
```

### 2.4 UI 模块 (`src/ui/`)

用户界面组件。

```
ui/
├── mod.rs
├── map/
│   └── map_impl.rs    # 地图相关 UI
│
└── canvas/
    ├── mod.rs
    ├── canvas.rs       # Canvas 组件定义
    ├── widget_impl.rs  # egui::Widget 实现
    ├── state.rs        # CanvasState - 画布状态
    ├── helpers.rs      # 辅助绘制函数
    │
    └── input/
        ├── mod.rs
        ├── state_manager.rs  # 输入状态机
        ├── input_state.rs    # 输入状态定义
        └── button_state.rs   # 按钮状态
```

#### 输入状态机

```
┌─────────────────────────────────────────────────────────┐
│                  Input State Machine                     │
│                                                          │
│   ┌───────┐  Space 按下   ┌──────────────┐               │
│   │ Idle  │──────────────►│  Panning     │               │
│   │       │◄──────────────│  (准备平移)  │               │
│   └───────┘  Space 释放   └──────┬───────┘               │
│                                  │                       │
│                           鼠标按下 │                       │
│                                  ▼                       │
│                          ┌──────────────┐                │
│                          │  Panning     │                │
│                          │  (拖拽中)    │                │
│                          └──────────────┘                │
│                                                          │
│   滚轮: 直接平移画布                                      │
│   Ctrl+滚轮 / 捏合: 缩放                                  │
└─────────────────────────────────────────────────────────┘
```

### 2.5 资源管理模块 (`src/resource/`)

线程安全的资源共享机制。

```rust
// resource_impl.rs
pub struct Resource<T> {
    inner: Arc<RwLock<T>>,
}

impl<T> Resource<T> {
    /// 只读访问
    pub fn read_resource<R>(&self, f: impl FnOnce(&T) -> R) -> R;
    
    /// 可变访问
    pub fn with_resource<R>(&self, f: impl FnOnce(&mut T) -> R) -> R;
}

// 类型别名
pub type CanvasStateResource = Resource<CanvasState>;
pub type MapSystemResource = Resource<MapSystem>;
pub type PointsRendererResource = Resource<PointsRenderer>;
// ...
```

---

## 三、着色器架构

### 3.1 着色器文件

```
assets/shaders/
├── points.wgsl     # 点渲染 (实例化三角形)
├── delaunay.wgsl   # Delaunay 边渲染 (线段)
└── voronoi.wgsl    # Voronoi 边渲染 (线段)
```

### 3.2 坐标变换流程

```
Canvas Space (逻辑坐标)
        │
        │  × scale
        │  + translation
        ▼
Screen Space (屏幕像素)
        │
        │  normalize to [-1, 1]
        ▼
NDC (Normalized Device Coordinates)
```

```wgsl
fn get_screen_pos(point: Pos2, uniforms: CanvasUniforms) -> vec2<f32> {
    let x = (point.x * uniforms.scale + uniforms.translation_x 
             - uniforms.canvas_x) / uniforms.canvas_width * 2.0 - 1.0;
    let y = -((point.y * uniforms.scale + uniforms.translation_y 
              - uniforms.canvas_y) / uniforms.canvas_height * 2.0 - 1.0);
    return vec2<f32>(x, y);
}
```

---

## 四、数据流图

### 4.1 初始化流程

```
┌─────────────────┐
│  TemplateApp    │
│     ::new()     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│  MapSystem      │────►│  Grid           │
│  ::default()    │     │  ::generate()   │
└────────┬────────┘     └─────────────────┘
         │
         ▼
┌─────────────────┐
│  delaunay::     │
│  triangulate()  │
└────────┬────────┘
         │
         ▼
┌─────────────────────┐
│  voronoi::          │
│  compute_indexed()  │
└────────┬────────────┘
         │
         ▼
┌─────────────────────────┐
│  创建 GPU 渲染器         │
│  - PointsRenderer       │
│  - DelaunayRenderer     │
│  - VoronoiRenderer      │
└─────────────────────────┘
```

### 4.2 渲染帧流程

```
┌───────────────────────────────────────────────────────────────┐
│                        每帧更新循环                            │
├───────────────────────────────────────────────────────────────┤
│                                                               │
│  1. 输入处理                                                  │
│     InputStateManager.update()                                │
│         │                                                     │
│         ▼                                                     │
│     更新 CanvasState (平移/缩放)                              │
│                                                               │
│  2. UI 构建                                                   │
│     Canvas Widget                                             │
│         │                                                     │
│         ├─► draw_grid() - CPU 绘制网格线                      │
│         │                                                     │
│         └─► 添加 GPU 回调                                     │
│              - VoronoiCallback                                │
│              - DelaunayCallback                               │
│              - PointsCallback                                 │
│                                                               │
│  3. GPU 渲染 (egui_wgpu 调用回调)                             │
│     Callback.paint()                                          │
│         │                                                     │
│         ├─► 更新 Uniforms                                     │
│         ├─► upload_to_gpu()                                   │
│         └─► render()                                          │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

---

## 五、扩展点

### 5.1 添加新的渲染图层

1. 创建 `XxxRenderer` 结构 (参考 `VoronoiRenderer`)
2. 创建 `XxxCallback` 实现 `egui_wgpu::CallbackTrait`
3. 编写对应的 `.wgsl` 着色器
4. 在 `MapRenderer` 中添加管线创建函数
5. 在 `Canvas::ui()` 中添加回调

### 5.2 添加新的单元格属性

1. 在 `CellsData` 中添加新字段
2. 根据需要实现相应的生成算法
3. 添加可视化渲染

### 5.3 添加新的特征类型

1. 在 `feature.rs` 中扩展 `CellFeatureType` 或 `PointFeatureType`
2. 实现 `CellFeature` 或 `PointFeature` trait
3. 在 UI 中添加相应的编辑界面

---

## 六、性能优化策略

### 6.1 已实现

- **视口裁剪**: `helpers::get_visible_indices()` 只渲染可见区域
- **GPU 渲染**: 使用 wgpu 进行硬件加速
- **并行计算**: 使用 rayon 进行 CPU 并行

### 6.2 待实现

- **LOD**: 根据缩放级别动态调整渲染细节
- **空间索引**: 四叉树/KD树加速空间查询
- **增量更新**: 只更新变化的部分而非全量更新
- **Buffer 复用**: 避免每帧重新分配 GPU Buffer

---

*文档版本: 1.0*
*最后更新: 2026-01-11*
