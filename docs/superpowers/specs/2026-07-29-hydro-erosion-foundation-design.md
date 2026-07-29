# Sekai 水文—侵蚀当前切片设计

日期：2026-07-29

状态：主代理自审通过，进入实施

范围：前工业、中世纪幻想世界的当前自然切片

## 1. 目标

本切片把已经发布的空间、构造地貌、地质基底和初步月度气候，转化为后续最终气候、土壤、生态、聚落和交通可以共同依赖的当前地表与水文事实：

- 侵蚀和沉积后的当前地表高程；
- 每个单元的月度有效地表径流与月度流量；
- 无环、可验证的单流向排水 DAG；
- 汇水面积、流域、湖泊和有向河段；
- 受汇水量、坡度与基岩抗蚀性共同控制的河流侵蚀；
- 显式保存的侵蚀深度、沉积厚度和流域出口沉积物输出。

侵蚀是形成当前状态的有界算法，不生成虚构年份、洪水事件、河道年代或地貌时间线。

本切片不是完整水循环。蒸散、土壤蓄水、地下水、积雪、冰川、湿地、潮汐和最终气候反馈分别留给后续有明确输入条件的所有者。

## 2. 已确认的因果位置

```text
空间
  → 板块/地壳
  → 地貌与地质
  → 初步月度气候
  → 水文、侵蚀与沉积（本切片）
  → 最终气候、积雪与冰川
```

本切片的权威输入只有：

- `SpatialSnapshot`；
- `ReliefSnapshot`；
- `GeologicSnapshot` 中的相对渗透性与基岩抗蚀性；
- `PreliminaryClimateSnapshot` 中的月度降水；
- 经规则解析后的 `HydroErosionSpec`。

它不得读取：

- 气候生成器的内部规则网格；
- 旧 `terrain::hydrology`、旧高度模板或旧侵蚀原型；
- UI、GPU、应用状态；
- 土壤、植被、社会、魔法或历史事件。

## 3. 研究依据与模型诚实性

### 3.1 排水

Priority-Flood 能在不规则连接图上填洼，并可扩展为流向和流域标记；浮点高程的标准复杂度为 `O(n log n)`。[Barnes, Lehman & Mulla, 2014](https://doi.org/10.1016/j.cageo.2013.04.024)

平坦区不能靠不稳定的浮点微扰决定流向。正式实现使用 Priority-Flood 的稳定出队秩作为第二排序键，让所有接收者严格早于供水者，从而得到可证明无环的排水 DAG。其目的与平坦区方向分配研究一致，但适配当前不规则 Voronoi 拓扑。[Barnes, Lehman & Mulla, 2014](https://doi.org/10.1016/j.cageo.2013.01.009)

非洼地优先采用朝相邻单元的最大坡降，延续经典单流向排水模型的可解释性。[O'Callaghan & Mark, 1984](https://doi.org/10.1016/S0734-189X(84)80011-0)

### 3.2 河流侵蚀

河流下切使用汇水量或流量、局部坡度与抗蚀性的有界 stream-power 近似。该关系适合表达大尺度河网对地貌的首阶控制，但本产品不把程序参数冒充实测侵蚀率。[Whipple & Tucker, 1999](https://doi.org/10.1029/1999JB900120)

实现采用固定形成强度和硬上限，不暴露虚构的“模拟年数”。这保留了 stream-power 的因果方向，同时避免把未校准模型伪装成地质年代学。以后若需要更精确的隐式求解，可在不改变正式输入输出的情况下替换为 FastScape 一类算法。[Braun & Willett, 2013](https://doi.org/10.1016/j.geomorph.2012.10.008)

### 3.3 沉积

沉积物遵守逐单元体积守恒：上游来沙与本地侵蚀体积之和，只能成为本地沉积、向下游传输或在流域出口输出。低流能位置保留更多沉积，高流能位置传输更多。该结构与显式区分侵蚀、传输和沉积的景观演化模型一致。[Davy & Lague, 2009](https://doi.org/10.1029/2008JF001146)

V1 的沉积能力仍是有界程序近似，不宣称预测粒径、含沙量或真实地质时间内的沉积速率。

### 3.4 河网等级

河段使用 Strahler 序表达分支层级；它是当前河网的无量纲形态属性，不是显示专用归一化值。[Strahler, 1957](https://doi.org/10.1029/TR038i006p00913)

## 4. 方案选择

### 4.1 采用：单一原子阶段，内部固定两趟

```text
初始排水
  → 有界侵蚀与沉积
  → 最终地表上的排水、湖泊、流域和河网
  → 原子发布 HydroErosionSnapshot
```

模块内部职责：

- `hydrology`：Priority-Flood、流向、汇水、流域、湖泊和河段；
- `erosion`：stream-power 侵蚀、沉积输运和地表恒等式；
- `hydro_erosion`：固定两趟编排、构造正式快照；
- `hydro_erosion_stage`：只做引擎产物适配。

固定两趟是算法版本的一部分，不是时间步：

1. 第一趟只为侵蚀提供排水强迫；
2. 第二趟在侵蚀—沉积后的地表上发布最终水文。

该结构保持调度图无环，并保证调用方看不到不一致的“侵蚀前水文 + 侵蚀后地表”半成品。

### 4.2 拒绝：三个公开阶段形成隐式反馈

```text
pre-hydrology → erosion → final-hydrology
```

虽然缓存更细，但会公开两个水文真值，扩大命名、字段和错误恢复表面。当前两个内部水文求解成本相对空间阶段很小，不值得牺牲契约清晰度。

### 4.3 拒绝：装饰性河流路径

从噪声或随机源头画折线不能提供汇水守恒、流域、湖泊出口或侵蚀因果，也会重新出现“看起来像河、不能被系统消费”的旧原型问题。

### 4.4 拒绝：覆盖 `ReliefSnapshot`

`ReliefSnapshot` 是构造、火山与区域地貌的唯一写入结果。侵蚀阶段不得回写它。

本切片新增 `surface_elevation_m`：

- `elevation_m`：形成侵蚀输入的构造地貌高程；
- `surface_elevation_m`：侵蚀与沉积后的当前地表高程。

二者语义不同、写入者唯一、依赖显式。应用在本切片完成后默认显示当前地表。

## 5. 规格与世界法则

### 5.1 `HydroErosionSpec`

```rust
pub struct HydroErosionSpec {
    schema_version: u16,
    river_discharge_threshold_deci_m3_s: u32,
    erosion_strength_permille: u16,
    minimum_lake_depth_cm: u16,
}
```

默认值：

| 参数 | 默认 | 有效范围 | 含义 |
|---|---:|---:|---|
| 河流阈值 | 250.0 m³/s | 0.1–100,000 m³/s | 超过阈值才发布为河网 |
| 侵蚀强度 | 1000‰ | 0–2000‰ | 形成式地貌下切强度 |
| 最小湖深 | 1.00 m | 0.01–100 m | 排除数值与微地形浅洼 |

全部可持久化参数使用定点整数。模型内部参考坡度、参考流量、最大下切、最大沉积和渗透—径流映射属于编译模型版本，不进入世界事实。

### 5.2 规则能力

稳定能力：

```text
sekai.core.natural.hydro-erosion-model@1
```

V1 唯一受信实现：

```rust
HydroErosionModel::PriorityFloodStreamPowerV1
```

Earthlike 内置规则包提供该唯一能力。规则解析器遵循已有唯一能力语义：

- 缺失时报错；
- 多个提供者时报错；
- 核心 schema 不兼容时报错；
- 审计记录 provider、模型和来源。

### 5.3 解析后输入

```rust
pub struct ResolvedHydroErosionInput {
    spec: HydroErosionSpec,
    model: HydroErosionModel,
}
```

完整规则审计保留在独立 resolution artifact；投影输入只含会改变生成结果的模型与规格。生成器不直接读取审计、规则包或作者约束，因此语义等价但来源身份不同的规则包不会污染生成阶段缓存键。

## 6. 正式世界契约

### 6.1 新稳定 ID

```rust
DrainageBasinId(u32)
LakeId(u32)
RiverSegmentId(u32)
```

所有 ID 从零连续、按稳定空间顺序分配，不来自哈希表迭代顺序。

### 6.2 `SurfaceProcessSnapshot`

```rust
pub struct SurfaceProcessSnapshot {
    schema_version: u16,
    cell_count: u32,
    erosion_depth_m: Vec<f32>,
    deposition_thickness_m: Vec<f32>,
    surface_elevation_m: ElevationField,
    sediment_throughput_m3: Vec<f64>,
    sediment_export_m3: f64,
}
```

每个单元满足：

```text
surface_elevation
  = constructional_elevation
  - erosion_depth
  + deposition_thickness
```

误差上限为 5 cm。侵蚀深度与沉积厚度非负、有限并受硬上限约束。海洋单元 V1 不执行河流侵蚀或海底沉积。

`sediment_throughput_m3` 是形成算子中离开该单元的沉积物总体积，不是年度输沙率。

### 6.3 水体、出口与网络

```rust
pub enum SurfaceWaterKind {
    DryLand,
    Ocean,
    Lake,
}

pub enum BasinOutletKind {
    Ocean,
    Lake,
    ClosedSink,
}

pub enum RiverSegmentKind {
    Channel,
    LakeOutlet,
}
```

```rust
pub struct DrainageBasin {
    id: DrainageBasinId,
    outlet_cell: CellId,
    outlet_kind: BasinOutletKind,
    area_km2: f64,
    mean_discharge_m3_s: f32,
}

pub struct Lake {
    id: LakeId,
    cells: Vec<CellId>,
    surface_elevation_m: f32,
    area_km2: f64,
    volume_m3: f64,
    outlet_cell: Option<CellId>,
    downstream_cell: Option<CellId>,
}

pub struct RiverSegment {
    id: RiverSegmentId,
    from: CellId,
    to: CellId,
    kind: RiverSegmentKind,
    strahler_order: u8,
    mean_discharge_m3_s: f32,
}
```

湖泊内部不伪造河道。河流进入湖泊后由 `Lake` 连通语义承接；只有湖泊真实出口发布 `LakeOutlet` 河段。

### 6.4 `HydrologySnapshot`

```rust
pub struct HydrologySnapshot {
    schema_version: u16,
    cell_count: u32,
    river_discharge_threshold_m3_s: f32,
    minimum_lake_depth_m: f32,
    monthly_local_runoff_mm: Vec<[f32; 12]>,
    monthly_discharge_m3_s: Vec<[f32; 12]>,
    annual_local_runoff_mm: Vec<f32>,
    mean_annual_discharge_m3_s: Vec<f32>,
    drainage_area_km2: Vec<f32>,
    drainage_surface_elevation_m: ElevationField,
    lake_depth_m: Vec<f32>,
    surface_water_kind: SurfaceWaterField,
    flow_receiver: Vec<Option<CellId>>,
    basin_id: Vec<Option<DrainageBasinId>>,
    strahler_order: StrahlerOrderField,
    basins: Vec<DrainageBasin>,
    lakes: Vec<Lake>,
    river_segments: Vec<RiverSegment>,
}
```

`drainage_surface_elevation_m` 是 Priority-Flood 填洼后的求解表面，仅用于水文解释与验证，不覆盖真实地表。

`SurfaceWaterField` 和 `StrahlerOrderField` 内部保存 `u32`，以便正式字段显示零拷贝借用；类型化访问器负责把原始值转换为枚举或受限序数。

### 6.5 原子快照

```rust
pub struct HydroErosionSnapshot {
    schema_version: u16,
    surface: SurfaceProcessSnapshot,
    hydrology: HydrologySnapshot,
}
```

只有该复合快照作为引擎产物发布。构造器和反序列化都必须完整验证。

## 7. 核心算法

### 7.1 数值规范化

- 输入高程先量化到厘米整数；
- Priority-Flood 优先键为 `(filled_height_cm, CellId)`；
- 最终类别阈值使用量化后的厘米、升每秒或定点规格；
- 所有邻接访问按 `CellId` 升序；
- 累积按稳定拓扑顺序串行执行；
- 连续输出可用 `f32/f64`，但会影响分类的值先量化。

### 7.2 海洋出口

最终地表低于全局海平面的单元是海洋出口。若没有海洋单元，选择全局最低、再按 `CellId` 破同值的单元作为唯一 `ClosedSink`。

这保证全陆测试世界仍有有界 DAG，但不会把闭合边界伪装成海洋。

### 7.3 Priority-Flood

1. 把所有海洋出口放入最小堆；
2. 若无海洋，放入稳定全局最低点；
3. 每次取最小 `(水位, CellId)`；
4. 未访问邻居的填洼水位为 `max(自身高程, 当前水位)`；
5. 记录稳定出队秩；
6. 继续直到每个单元访问一次。

### 7.4 流向

每个非海洋单元选择一个相邻接收者：

1. 候选必须拥有更低填洼高程，或相同填洼高程但更早出队；
2. 有真实下坡时优先最大物理坡降；
3. 平坦或洼地内按填洼高程、出队秩和 `CellId` 稳定排序；
4. 海洋与唯一闭合汇无接收者。

接收者的 `(填洼高程, 出队秩)` 严格小于供水者，因此天然无环。

### 7.5 有效径流

V1 尚无土壤、植被、蒸散、积雪与地下水，不构造虚假的完整水量平衡。它发布“有效地表径流代理”：

```text
runoff_fraction = lerp(0.85, 0.20, relative_permeability)
monthly_runoff_mm = preliminary_monthly_precipitation_mm × runoff_fraction
```

海洋单元径流为零。以后土壤—生态—最终气候切片可替换该编译模型，但必须保留相同单位和非负守恒边界。

### 7.6 月度流量

本地月水量：

```text
volume_m3 = runoff_mm / 1000 × cell_area_m2
```

按排水 DAG 从上游到下游累积，再除以统一的平均气候月秒数得到 `m³/s`。年度平均流量等于全年体积除以平均气候年秒数，并与 12 个月等权平均保持恒等。

### 7.7 汇水面积与流域

汇水面积按相同 DAG 累积。每个陆地单元沿接收者最终到达：

- 一个海洋单元；
- 一个湖泊终端；
- 或唯一闭合汇。

不同稳定终端形成不同流域，按终端 `CellId` 排序后分配连续 `DrainageBasinId`。

### 7.8 侵蚀

第一趟水文提供平均流量与坡度。形成式下切强度为：

```text
energy = bounded(
    discharge_response(mean_discharge)
  × slope_response(drainage_slope)
)

erosion_depth
  = max_formation_incision
  × erosion_strength
  × energy
  × (1 - erosion_resistance)
```

- 流量或坡度为零时不产生河流下切；
- 高抗蚀性抑制下切；
- 每单元和全模型都有硬上限；
- 结果量化到厘米；
- 不使用随机噪声修饰河谷。

### 7.9 沉积输运

按上游到下游顺序：

```text
available = upstream_sediment + local_eroded_volume
retained_fraction = bounded_function(low_energy, lake_or_sink)
deposited = min(available × retained_fraction, local_deposition_capacity)
outgoing = available - deposited
```

终端 `outgoing` 汇总进入 `SurfaceProcessSnapshot::sediment_export_m3`。V1 不把第一趟侵蚀排水的沉积出口强行映射到第二趟最终流域，避免两个水文真值发生隐式耦合。每个单元与全世界都检查体积守恒：

```text
eroded_volume
  = deposited_volume
  + terminal_export_volume
```

允许小的确定性浮点容差，不允许静默丢失或创造沉积物。

### 7.10 最终水文

侵蚀和沉积形成 `surface_elevation_m` 后，重新执行完整水文求解。正式湖泊、流域、流量、河段和 Strahler 序只来自第二趟。

## 8. 阶段图

新增外部产物：

```text
natural.hydro-erosion-spec
```

新增阶段：

```text
rules.hydro-erosion-resolution
natural.resolve-hydro-erosion-input
natural.hydro-erosion
```

正式输出：

```text
world.hydro-erosion
```

`natural.hydro-erosion` 的依赖精确为：

```text
natural.resolved-hydro-erosion-input
world.spatial
world.relief
world.geology
world.preliminary-climate
```

它不依赖板块、地幔或规则包原始产物，因为这些影响已经通过声明的上游产物传入。

完整自然图由 12 阶段 / 6 外部输入扩展为 15 阶段 / 7 外部输入。

## 9. 字段与显示

新增正式字段：

| 字段 ID 后缀 | 类型 | 单位 | 依赖 |
|---|---|---|---|
| `surface_elevation_m@1` | ScalarF32 | m | elevation, erosion, deposition |
| `fluvial_erosion_depth_m@1` | ScalarF32 | m | preliminary precipitation, elevation, permeability, erosion resistance |
| `sediment_deposition_thickness_m@1` | ScalarF32 | m | erosion, elevation |
| `surface_water_kind@1` | CategoryU32 | — | surface elevation, lake depth |
| `lake_depth_m@1` | ScalarF32 | m | surface elevation, drainage surface |
| `annual_local_runoff_mm@1` | ScalarF32 | mm/year | preliminary precipitation, permeability |
| `mean_annual_discharge_m3_s@1` | ScalarF32 | m³/s | runoff, receiver |
| `drainage_area_km2@1` | ScalarF32 | km² | spatial area, receiver |
| `strahler_stream_order@1` | CategoryU32 | — | discharge, receiver, lakes |

注册表从 27 个字段扩展到 36 个字段。

V1 显示边界：

- 九个字段都可检查；
- 标量和分类字段可做单元格填色；
- 有向河网仍作为正式世界网络保存，但不在本切片扩张 GPU 网络覆盖层；
- 应用默认填色从构造 `elevation_m` 切换到当前 `surface_elevation_m`；
- 头部当前切片更新为“空间 → 板块/地壳 → 地形/地质 → 初步气候 → 水文/侵蚀”。

## 10. 验证不变量

### 10.1 自包含

- 所有密集字段长度等于 `cell_count`；
- 所有连续值有限且位于声明范围；
- 月度与年度摘要恒等；
- ID 连续、记录稳定排序；
- 接收者不是自身且位于范围内；
- 接收者图无环；
- 河段引用有效相邻单元；
- 河段 ID 连续，方向与接收者一致；
- 湖泊单元互斥、排序且与水体字段一致；
- 流域 ID 连续，单元流域与终端一致；
- Strahler 序只在达到河流阈值的网络中非零。

### 10.2 对输入

- 空间、地貌、地质、气候与输出单元数完全一致；
- 接收者必须是 `SpatialSnapshot` 中的真实邻居；
- 构造高程、侵蚀、沉积与当前地表满足恒等式；
- 海平面分类来自正式地貌海平面；
- 径流只读取正式月降水与相对渗透性；
- 侵蚀只读取正式抗蚀性；
- 下游汇水面积和流量不小于任何直接上游贡献；
- 沉积物全局体积守恒。

## 11. 测试与验收

### 11.1 契约

- 规格边界、定点换算、serde 往返；
- 快照构造、反序列化重验；
- 错误长度、NaN、越界 ID、自环、环路、错误邻接；
- 月度—年度恒等；
- 地表组成恒等；
- 湖泊、流域、河段交叉引用。

### 11.2 算法

- 同输入产生逐字节相同结果；
- 合成碗地产生湖泊和稳定溢出口；
- 合成山脊形成不同流域；
- 平坦区不会循环；
- 全海洋、全陆地和单出口世界有定义；
- 更高渗透性降低有效径流与流量；
- 更高降水提高流量；
- 更软基岩产生更深下切；
- 零侵蚀强度保持原高程；
- 沉积体积守恒；
- 河网下游流量与 Strahler 序单调合法。

### 11.3 阶段与缓存

- 产物键、阶段 ID、版本和依赖精确；
- 第二次完整构建 15 阶段全命中；
- 只改水文规格时仅重跑 3 个新增阶段；
- 只改气候规格时重跑气候链与水文链，不重跑地貌/地质；
- 只改地质规格时重跑地质与水文，不重跑气候；
- 错误跨产物缓存不污染合法结果。

### 11.4 质量与视觉

固定八种子检查：

- 所有陆地最终可达海洋、合法湖泊终端或全陆闭合世界的唯一汇；
- 非空陆地世界有非零汇水面积；
- 默认大陆尺度世界形成多级分支河网；
- 河流集中于汇水路径，不形成椒盐点或椭圆条带；
- 侵蚀沿河网和陡坡组织，软岩区相对更强；
- 沉积集中于低能、湖盆和出口附近；
- 当前地表保留构造山脉，同时出现连贯河谷；
- 湖泊对应真实填洼深度，不是随机圆斑。

人工审阅金图：

- 当前地表；
- 地表水体类别；
- Strahler 河网等级；
- 河流侵蚀深度；
- 沉积厚度。

### 11.5 工程门禁

- debug 全目标测试；
- release 全目标测试；
- `clippy -D warnings`；
- `fmt --check` 与 `git diff --check`；
- WASM 全特性检查；
- `trunk build`；
- 20,000 单元 release 性能和内存预算；
- 真实桌面窗口切换水文字段并重建新种子。

## 12. 性能预算

20,000 单元默认世界：

- 水文—侵蚀阶段目标不超过 350 ms release；
- 两次 Priority-Flood 均为 `O(n log n)`；
- 其余 DAG 累积、流域、湖泊和河网均为 `O(n + e)`；
- 密集新增数据目标不超过 8 MiB；
- 不为每个月复制拓扑、邻接表或 Priority-Flood 状态；
- 不在热循环中构造哈希映射；
- 不进入每帧 UI 或 GPU 上传路径。

## 13. 错误恢复与应用边界

- 阶段失败时不发布部分水文或部分地表；
- 应用保留上一份完整有效文档、显示包和修订时钟；
- 新文档只有在空间、地貌、地质、气候与水文全部交叉验证后原子替换；
- 旧 `terrain::hydrology` 继续编译但不进入生产阶段图；
- 当前主程序在开发期间继续运行，最终合并后切换到主分支新构建。

## 14. 延后项及其所有者

| 延后项 | 所有者 |
|---|---|
| 蒸散、土壤蓄水、植被截留 | 最终气候/土壤联合切片 |
| 地下水、泉、湿地 | 地下水与湿地切片 |
| 积雪、冻土、冰川与季节冻结 | 最终气候/冰雪切片 |
| 海流、潮汐、三角洲海底沉积 | 海洋切片 |
| 洪水频率与灾害 | 未来风险切片，不是历史事件 |
| 河流网络 GPU 线覆盖 | 显示网络覆盖层 |
| 河道宽度、航运等级、桥渡成本 | 交通与聚落切片 |

本切片不建立空 trait、占位 artifact 或未来字段。

## 15. 自审

### 15.1 正交性

- `world` 只定义类型和不变量；
- `rules` 只解析能力；
- `hydrology` 只求水文；
- `erosion` 只求地表物质迁移；
- 联合生成器只编排固定两趟；
- 阶段适配只连接引擎；
- 字段 schema 不持有颜色、GPU 或 UI 状态；
- 应用只适配完整快照。

### 15.2 单一写入

- 构造地貌仍由 `ReliefStage` 唯一写入；
- 当前地表、侵蚀和沉积只由 `HydroErosionStage` 写入；
- 最终水文只由同一原子阶段写入；
- 最终气候以后读取当前地表和水文，不回写它们。

### 15.3 当前切片

- 两趟是有界形成算法，不是两年或两个历史时期；
- 月份是同一气候常态的季节维度；
- 不生成日期、洪水事件或侵蚀年代；
- 结果解释为“哪些当前因素形成了当前状态”，不解释为编年史。

### 15.4 大方向决策检查

本设计没有扩大到海洋、地下水、冰雪、土壤或历史；没有改变已确认的自然因果顺序；没有引入新的外部服务或产品形态。因此不需要用户追加方向决策。
