# Sekai 地质基底 V1 设计

**状态：** 自审通过；依据用户已授权的例行设计确认，进入实施计划  
**日期：** 2026-07-29  
**上位设计：** `docs/superpowers/specs/2026-07-28-sekai-current-slice-world-design.md`  
**上游切片：**

- `docs/superpowers/specs/2026-07-29-tectonic-natural-foundation-design.md`
- `docs/superpowers/specs/2026-07-29-rule-capabilities-author-constraints-design.md`

## 1. 摘要

本切片在正式板块、地壳和地貌产物之上建立后续水文与侵蚀所需的地质基底：

```text
规则包 + GeologicSpec
  → 已解析地质模型输入
  → 深部热点、热流与火山影响
  → ReliefStage 消费火山强迫并继续唯一写入高程
  → 地表基岩、裂隙、抗蚀性、渗透性与资源形成潜势
  → 只读字段显示
```

它仍然生成当前时间切片，不积分地质年代，不生成喷发年份、地层年代、矿山或历史事件。

本切片的主要目的不是给地图增加一层装饰色，而是补齐三个稳定的因果接口：

1. 热点在地貌生成前成为类型化自然强迫；
2. 水文和侵蚀以后只读取正式的基岩抗蚀性与渗透性；
3. 资源阶段以后读取地质形成潜势，而不是从噪声直接放置具体矿藏。

## 2. 问题与依据

当前主链已经拥有：

- 连通板块、独立地壳、相对运动和构造边界；
- 地壳基准、构造地貌、区域起伏、最终高程和海陆；
- 类型化规则能力、作者构造约束、阶段缓存和字段显示。

但下游自然系统仍缺少：

- 与板块独立的深部热点及热流；
- 热点对最终地貌的显式贡献；
- 地表暴露基岩的稳定分类；
- 可供侵蚀使用的相对抗蚀性；
- 可供径流和地下水使用的相对渗透性；
- 只表达地质许可条件、不假装已经发现资源的潜势字段。

现实地质图将地表暴露地质、基岩和表层覆盖物作为不同信息层处理；本切片只建立基岩层，沉积覆盖、土壤和风化层由后续阶段拥有。[USGS Cooperative National Geologic Map](https://www.usgs.gov/data/cooperative-national-geologic-map-earths-surface-geology)

岩性会显著影响大尺度侵蚀和产沙能力，因此侵蚀不能只读取高程与降水。[A global erodibility index to represent sediment production potential of different rock types](https://doi.org/10.1016/j.apgeog.2018.10.010)

USGS 的矿产评估先按地质环境划定“许可区”，再评估具体矿床。本切片采用同样的职责边界：只生成形成潜势，不生成矿床数量、储量、价值或开采状态。[USGS National Mineral-Resource Assessment](https://pubs.usgs.gov/info/assessment/)

热点是深部热异常及相关火山活动的当前空间来源；热点轨迹则隐含板块随时间运动。本切片只生成当前热点中心和紧支撑影响，不生成岛链年龄序列或历史轨迹。[USGS: What is a hotspot and how do you know it is there?](https://www.usgs.gov/faqs/what-a-hotspot-and-how-do-you-know-its-there)

> 2026-08-03 补充：经[因果岛屿与多尺度地貌噪声设计](./2026-08-03-causal-island-relief-design.md)批准，Relief 可以读取当前板块速度，把当前热点支撑塑造成短程各向异性岛组。这只是当前状态的空间响应；不得生成年龄、事件序列、旧火山实体或任何历史状态。

热流采用 `mW/m²`，与公开地学数据的单位一致。默认背景值和异常值是生成模型的安全范围，不声称重建某个现实地区。[USGS Heat Flow Data](https://data.usgs.gov/datacatalog/data/USGS%3A60edd2c3d34e48f87173b1bb)

## 3. 方案比较与决策

### 3.1 方案 A：高程之后贴岩性色块

做法：

- 从现有地壳类型、边界种类和噪声直接分类岩性；
- 不建立热点产物；
- 不修改地貌依赖。

优点：

- 改动最少；
- 可以很快显示岩性字段。

缺点：

- 热点不能影响火山地貌；
- 资源潜势与热流没有权威来源；
- 地质只成为显示后处理，不是自然因果链的一部分。

**结论：不采用。**

### 3.2 方案 B：完整地层、岩浆和地质历史模拟

做法：

- 积分岩浆活动、沉积、变质、抬升、剥露和地层年代；
- 保存地层柱、侵入体和喷发序列。

优点：

- 可表达详细地质历史；
- 可以生成更具体的矿床模型。

缺点：

- 引入当前明确排除的时间模型；
- 计算、校准、存储和作者编辑成本远超当前阶段；
- 会把水文与社会切片推迟到不可接受的范围。

**结论：不采用。**

### 3.3 方案 C：深部强迫与地表地质分层

做法：

- `MantleStage` 从空间和已解析地质输入生成热点、热流和火山影响；
- `ReliefStage` 只消费这一强迫并继续唯一写入高程；
- `GeologicStage` 从空间、构造、深部强迫和地貌派生基岩与物性；
- 资源只表达连续形成潜势。

优点：

- 热点在正确的因果位置进入地貌；
- 深部强迫与地表物性可独立替换和测试；
- 没有历史时间线；
- 水文、侵蚀和资源阶段获得最小稳定输入；
- 规则模型、缓存和显示边界保持一致。

成本：

- `ReliefSnapshot` 需要一个明确的 schema 升级；
- 阶段图、规则能力和应用文档都需要扩展；
- 固定高程 golden 必须重新人工审阅。

**结论：采用。**

## 4. 范围

### 4.1 包含

- 强类型 `HotspotId`；
- 语义化 `GeologicSpec`；
- 世界法则选择的 `GeologicModel::CurrentSliceV1`；
- 地质模型规则能力、完整解析审计和最小生成投影；
- 与板块 ID 和地壳类别独立的热点中心；
- 每单元地幔热流和火山影响；
- 火山地貌对 `ReliefSnapshot` 的可解释分量；
- 五类宽泛基岩省；
- 裂隙强度、基岩抗蚀性和相对渗透性；
- 金属成矿、地热和沉积盆地三类形成潜势；
- 阶段、缓存、验证、诊断和只读字段显示；
- 固定夹具、性质测试、golden、性能和真实应用视觉验收。

### 4.2 不包含

- 地质年代、地层年代、岩层柱和历史事件；
- 热点轨迹、岛链年龄、板块时间积分或喷发序列；
- 火山实体目录、喷发危险度或当前天气；
- 松散沉积物、风化层、土壤或植被覆盖；
- 水文、侵蚀、沉积输运、气候、冰川或海流；
- 具体金属、宝石、盐、煤、油气或矿床实体；
- 储量、品位、可开采性、经济价值或采矿设施；
- 魔法热点、魔力地脉或超自然资源；
- 地质作者约束 UI、地图笔刷或直接数组编辑；
- 点实体和网络的新增 GPU 叠加能力；
- 旧 `terrain` 岩性或资源原型迁移。

## 5. 架构边界

### 5.1 模块所有权

```text
world::natural::geology
  拥有 GeologicSpec、MantleSnapshot、GeologicSnapshot、单位和验证
            ↑
rules
  只选择已编译地质模型；不读取自然数组
            ↑
generators::natural::geology
  拥有热点、热流、基岩和潜势算法
            ↑
generators::natural::stage
  只把类型化产物编排成无环阶段图
            ↑
app
  只组合外部规格、构建结果与显示文档
            ↓
view → gpu / ui
  只读字段与几何
```

约束：

- `world::natural` 不导入 `engine`、`rules`、`generators`、`app`、`view`、`ui` 或 GPU；
- `rules` 只依赖公开世界规格，不读取构造、地貌或地质快照；
- `generators::natural` 不导入旧 `terrain`、`app`、`view`、`ui`、egui 或 wgpu；
- `MantleGenerator` 不读取板块 ID、边界或高程；
- `ReliefGenerator` 是最终高程的唯一权威写入者；
- `GeologicGenerator` 只读上游产物，不修改高程、板块、地壳或热点；
- 显示注册表不反向定义领域类型；
- 旧 `terrain` 中的岩性、资源或火山逻辑不得进入正式主链。

### 5.2 阶段图

```text
PlanarSpaceArtifact ─→ SpatialStage ───────────────────────┐
                                                          │
TectonicSpecArtifact + RulePackSet + AuthorConstraints     │
  → RuleTectonicResolutionStage                            │
  → ResolvedTectonicInputStage                             │
  → TectonicStage ───────────────────────────────┐         │
                                                 │         │
GeologicSpecArtifact + RulePackSet               │         │
  → RuleGeologicResolutionStage                  │         │
  → ResolvedGeologicInputStage                   │         │
  ├→ MantleStage ────────────────────────────────┼→ ReliefStage
  └──────────────────────────────────────────────┘      │
                                                       │
Spatial + Tectonic + Mantle + Relief + resolved model  │
  └────────────────────────────────────────────→ GeologicStage
```

精确阶段职责：

#### `RuleGeologicResolutionStage`

- 输入：`GeologicSpecArtifact`、`RulePackSetArtifact`；
- 输出：`GeologicRuleResolutionArtifact`；
- 只负责规则依赖/能力验证、地质模型选择和完整审计；
- 不使用 RNG，不读取作者构造约束。

#### `ResolvedGeologicInputStage`

- 输入：`GeologicRuleResolutionArtifact`；
- 输出：`ResolvedGeologicInputArtifact`；
- 只投影 `model + spec`；
- 审计元数据改变但模型和规格不变时，下游缓存保持命中。

#### `MantleStage`

- 输入：`SpatialArtifact`、`ResolvedGeologicInputArtifact`；
- 输出：`MantleArtifact`；
- 只生成热点、热流和火山影响；
- 不读取构造或地貌。

#### `ReliefStage`

- 输入：`SpatialArtifact`、`TectonicArtifact`、`MantleArtifact`；
- 输出：`ReliefArtifact`；
- 继续唯一负责全部高程分量、最终高程和海陆分类；
- schema 和阶段算法版本显式升级。

#### `GeologicStage`

- 输入：`SpatialArtifact`、`TectonicArtifact`、`MantleArtifact`、
  `ReliefArtifact`、`ResolvedGeologicInputArtifact`；
- 输出：`GeologicArtifact`；
- 只生成地表基岩、物性和形成潜势。

### 5.3 缓存与失效

- 改变显示状态：所有世界阶段命中；
- 改变审计元数据但保持地质投影相同：`MantleStage` 及下游命中；
- 改变 `GeologicSpec`：地质规则解析、投影、深部强迫、地貌和地质失效；
- 改变 `TectonicSpec`：构造、地貌和地质失效，深部强迫命中；
- 改变根种子：所有使用 RNG 的阶段失效；
- 地质阶段失败：不发布半成品，不替换应用上一份完整文档。

## 6. 规则能力

新增唯一世界法则能力：

```text
sekai.core.natural.geologic-model@1
```

契约：

- `CapabilityCardinality::UniqueRequired`；
- 最低权限 `RulePackKind::WorldLaw`；
- 作者不能直接提供；
- 载荷为闭合枚举 `GeologicModel::CurrentSliceV1`。

内置 `sekai.builtin.earthlike` 同时提供：

- `TectonicModel::CurrentSliceV1`；
- `GeologicModel::CurrentSliceV1`。

本切片不新增普通地质控制能力和作者地质约束。`GeologicSpec` 是项目基础偏好；规则包只选择受信任模型。以后若确有多个普通规则需要控制热点或资源潜势，再为具体目标增加类型化约束，不能预建任意键值参数。

构造规则解析与地质规则解析保持两个独立审计产物。二者共享规则包集合和能力注册表，但不把地质字段塞入 `TectonicRuleResolution`。

## 7. 领域契约

### 7.1 `GeologicSpec`

```rust
pub struct GeologicSpec {
    pub schema_version: u16,
    pub hotspot_count: u16,
    pub mantle_activity: MantleActivity,
}

pub enum MantleActivity {
    Quiet,
    Moderate,
    Active,
}
```

V1 范围：

- schema 必须为 `1`；
- `hotspot_count: 0..=16`，且不得超过空间单元数；
- 活动级别只控制背景热流、热点强度和火山地貌幅度；
- 默认类地球配置为 `4` 个热点、`Moderate`。

`hotspot_count` 是当前地图中的深部异常中心数，不是全球现实热点数量，也不暗示历史持续时间。

### 7.2 `MantleSnapshot`

```rust
pub struct Hotspot {
    pub id: HotspotId,
    pub source_cell: CellId,
    pub strength_permille: u16,
    pub support_radius_m: Meters,
}

pub struct MantleSnapshot {
    schema_version: u16,
    cell_count: u32,
    hotspots: Vec<Hotspot>,
    heat_flow_mw_m2: Vec<f32>,
    volcanic_influence: Vec<f32>,
}
```

不变量：

- 热点 ID 从 `0` 连续并按 ID 排序；
- 热点源单元唯一且有效；
- 强度位于 `1..=1000`；
- 支撑半径有限、为正且不超过世界对角线；
- 热流有限并位于 `20..=400 mW/m²`；
- 火山影响有限并位于 `0..=1`；
- 没有热点时火山影响全为 `0`，热流仍有活动级别对应的背景值；
- 快照不保存年龄、轨迹、板块归属或喷发状态。

### 7.3 `ReliefSnapshot` schema V2

新增：

```rust
volcanic_offset_m: ElevationField
```

分量恒等式变为：

```text
elevation_m =
    crust_base_elevation_m
    + tectonic_offset_m
    + volcanic_offset_m
    + regional_offset_m
```

约束：

- `volcanic_offset_m: 0..=4_000 m`；
- 热点火山影响为零时该分量全为零；
- 最终高程仍位于 `-11_000..=9_000 m`；
- 统一海平面仍为 `0 m`；
- 安全钳制必须调整一个可解释分量并保持恒等式；
- `elevation_m@1` 的值类型、单位和语义仍是最终地表高程，因此字段 ID 不升级；
- `ReliefSnapshot` 的序列化布局改变，因此快照 schema 从 `1` 升到 `2`。

### 7.4 基岩分类

```rust
pub enum BedrockKind {
    OceanicMafic,
    ContinentalCrystalline,
    Sedimentary,
    Metamorphic,
    Volcanic,
}
```

这些是地图尺度的宽泛地质省，不是具体岩组或地层。

- `OceanicMafic`：稳定洋壳基底；
- `ContinentalCrystalline`：稳定大陆结晶基底；
- `Sedimentary`：盆地和稳定低地中的固结沉积基岩；
- `Metamorphic`：强汇聚、碰撞和造山影响区；
- `Volcanic`：热点、火山弧、裂谷和洋中脊影响区。

松散冲积物、冰碛物、土壤和风化壳以后作为覆盖层叠加，不能改写基岩真值。

### 7.5 `GeologicSnapshot`

```rust
pub struct GeologicSnapshot {
    schema_version: u16,
    cell_count: u32,
    bedrock_kind: BedrockKindField,
    fracture_intensity: Vec<f32>,
    erosion_resistance: Vec<f32>,
    relative_permeability: Vec<f32>,
    metallic_mineral_potential: Vec<f32>,
    geothermal_potential: Vec<f32>,
    sedimentary_basin_potential: Vec<f32>,
}
```

全部连续字段：

- 有限；
- 位于 `0..=1`；
- 表示当前生成模型中的相对指标；
- 不冒充实验室强度、绝对渗透率、矿石品位或发现概率。

语义：

- `fracture_intensity`：构造破碎和热异常共同产生的相对裂隙程度；
- `erosion_resistance`：以后侵蚀求解器使用的基岩相对抗蚀性，高值更难侵蚀；
- `relative_permeability`：以后径流/地下水求解器使用的相对渗透性；
- `metallic_mineral_potential`：弧、裂谷、热点、洋中脊和造山带提供的成矿许可程度；
- `geothermal_potential`：热流与裂隙共同提供的地热系统许可程度；
- `sedimentary_basin_potential`：沉积基岩与盆地形态提供的沉积型资源许可程度。

潜势可以重叠。不得把三类潜势压缩成一个互斥“资源类型”分类。

## 8. 确定性算法

### 8.1 随机子流

`MantleStage` 从自己的 `StageRng` 一次捕获固定根材料，再使用：

```text
hotspot-seeds-v1
hotspot-strength-v1
```

`GeologicStage` 使用：

```text
bedrock-province-v1
```

热点数量改变不得通过 RNG 消费顺序改变无关的基岩省随机场。所有并列使用稳定 `CellId` 或 `HotspotId`。

### 8.2 热点中心

1. 从独立子流选择第一个有效单元；
2. 后续热点使用量化世界距离的 farthest-point 采样；
3. 源单元必须唯一；
4. 按选择顺序分配连续 `HotspotId`；
5. 强度从独立标签按热点 ID 派生；
6. 支撑半径由地图短边、活动级别和强度确定，并钳制到语义安全范围。

热点选择不读取板块、地壳、边界或高程，因此改变板块数量不会移动固定种子下的热点。

### 8.3 热流和火山影响

- 背景热流由 `MantleActivity` 给出固定基准；
- 每个热点在图距离上产生紧支撑平滑核；
- 重叠热点取有界组合，不允许无界求和；
- `volcanic_influence` 是归一化后的最大/组合影响；
- `heat_flow_mw_m2` 是背景值加有界异常；
- 算法最多处理 16 个热点，时间和内存有明确上界。

### 8.4 火山地貌

`ReliefGenerator` 从 `volcanic_influence` 生成非负火山地貌分量：

- 洋壳热点可形成海山和岛屿；
- 大陆热点形成较宽、较低的火山高地；
- 强度和支撑只来自 `MantleSnapshot`；
- 不生成热点轨迹或沿板块速度方向排列的旧火山链；
- 最终安全处理保持各分量恒等式和统一海平面分类。

构造弧和裂谷已有的高程贡献仍属于 `tectonic_offset_m`。热点贡献只属于 `volcanic_offset_m`，避免重复所有权。

### 8.5 基岩省

基岩分类先计算稳定的边界影响、热点影响、盆地倾向和小幅省域随机场，再按明确优先级分类：

1. 强热点、火山弧、裂谷或洋中脊影响 → `Volcanic`；
2. 强大陆碰撞/造山影响 → `Metamorphic`；
3. 盆地倾向超过阈值 → `Sedimentary`；
4. 洋壳 → `OceanicMafic`；
5. 其余大陆壳 → `ContinentalCrystalline`。

盆地倾向可读取当前地貌的局部相对低势和宽尺度构造下沉，但不运行沉积输运。

### 8.6 物性

每类基岩有编译期固定的抗蚀性和基准渗透性。裂隙强度：

- 降低抗蚀性；
- 提高结晶岩的相对渗透性；
- 提高热流转化为地热潜势的效率。

所有计算使用有限 `f32`、稳定遍历和显式钳制。分类阈值前使用固定量化，避免原生/WASM 微小差异改变类别。

### 8.7 形成潜势

- 金属成矿潜势：岩浆/火山影响、边界类型、边界强度和裂隙的有界组合；
- 地热潜势：归一化热流与裂隙的有界组合；
- 沉积盆地潜势：沉积基岩、宽尺度低势和低构造扰动的有界组合。

这些字段没有随机“矿点”后处理。以后具体资源阶段必须显式消费潜势、气候、生态、魔法和规则输入。

## 9. 字段与显示

新增正式字段：

| 字段 ID | 类型 | 单位 | 依赖 |
|---|---|---|---|
| `sekai.core.natural.mantle_heat_flow_mw_m2@1` | ScalarF32 | mW/m² | — |
| `sekai.core.natural.volcanic_influence@1` | ScalarF32 | unitless | heat flow |
| `sekai.core.natural.volcanic_offset_m@1` | ScalarF32 | m | volcanic influence |
| `sekai.core.natural.bedrock_kind@1` | CategoryU32 | — | crust, boundary, volcanic influence, relief |
| `sekai.core.natural.fracture_intensity@1` | ScalarF32 | unitless | boundary strength, volcanic influence |
| `sekai.core.natural.erosion_resistance@1` | ScalarF32 | unitless | bedrock, fracture |
| `sekai.core.natural.relative_permeability@1` | ScalarF32 | unitless | bedrock, fracture |
| `sekai.core.natural.metallic_mineral_potential@1` | ScalarF32 | unitless | bedrock, boundary, fracture |
| `sekai.core.natural.geothermal_potential@1` | ScalarF32 | unitless | heat flow, fracture |
| `sekai.core.natural.sedimentary_basin_potential@1` | ScalarF32 | unitless | bedrock, relief |

更新 `elevation_m@1` 的依赖列表以加入 `volcanic_offset_m@1`。

显示约束：

- 所有新字段通过现有借用式 `FieldPayloadRef` 接入；
- 不复制权威密集数组到扩展字段集；
- 默认仍显示高程；
- 切换岩性或潜势字段不触发世界重建；
- 本切片不新增热点点符号；`volcanic_influence` 和热流字段用于验证空间位置；
- 类别图使用分类调色板，连续潜势使用顺序调色板，火山地貌使用发散/顺序语义一致的调色板。

`NaturalFieldDocument` 新增 `Arc<MantleArtifact>` 和 `Arc<GeologicArtifact>`。候选构建只有在全部五个正式自然产物和字段目录验证成功后才原子发布。

## 10. 验证与错误策略

### 10.1 致命错误

- 不支持的规格或快照 schema；
- 热点数量超过单元数；
- 热点 ID 不连续、源单元重复或越界；
- 热流、影响、高程分量或地质字段非有限/越界；
- 产物与空间单元基数不一致；
- 地质产物与构造、深部强迫或地貌不兼容；
- 字段 schema、依赖或负载类型错误；
- 规则包缺失/重复地质模型；
- 原生分类路径出现无法规范化的数值。

致命错误阻止候选文档发布，并保留上一份完整地图、GPU 包、字段选择和规则摘要。

### 10.2 非致命诊断

- 最终高程因新增火山分量触及安全范围并被调整；
- 极端小地图无法形成宽泛质量门要求的全部基岩类别；
- 某一固定种子没有高潜势区域；
- 活动配置和地图尺度组合使热点支撑半径被安全钳制。

诊断使用稳定机器代码、可选 `CellId` 和相关 `FieldId`。

## 11. 测试策略

### 11.1 契约测试

- `HotspotId`、`GeologicSpec`、`MantleSnapshot`、`BedrockKindField` 和
  `GeologicSnapshot` serde 往返；
- 私有不变量在反序列化时重新验证；
- 字段 ID、单位、范围、类别、依赖和负载精确匹配；
- `ReliefSnapshot` V2 分量恒等式；
- 旧 V1 relief JSON 明确拒绝为不支持 schema，不静默迁移。

### 11.2 规则测试

- 地质能力 ID、权限和唯一基数精确；
- 内置 earthlike 同时提供构造与地质模型；
- 普通规则包不能替换地质模型；
- 缺失或重复地质模型产生稳定能力错误；
- 审计变化但投影相同产生相同投影哈希。

### 11.3 生成性质

多组固定种子验证：

- 热点数精确，源单元唯一，ID 连续；
- 热流和火山影响有界；
- 改变板块数量不改变热点和热流；
- 热点附近火山地貌高于无热点对照；
- 基岩字段覆盖全部单元；
- 稳定默认种子集合中出现洋壳基岩、大陆结晶基岩及至少一种活动基岩；
- 物性和潜势全部有限且在 `0..=1`；
- 高地质热流与高裂隙组合不会产生低地热潜势；
- 强碰撞附近的变质/金属成矿倾向高于稳定大陆内部；
- 沉积盆地潜势不等同于海陆分类。

### 11.4 阶段与缓存

- 阶段和外部产物集合精确；
- 构造变化使 `MantleArtifact` 命中、地貌和地质失效；
- 地质规格变化不失效空间与构造；
- 审计元数据变化但投影相同不失效深部强迫；
- 根种子变化使随机自然阶段失效；
- 失败地质构建不发布半成品且先前缓存可恢复；
- 同输入缓存命中与未命中得到相同内容哈希。

### 11.5 视觉与 golden

- 固定种子保存高程、热流、火山影响、基岩和资源潜势 CPU 参考图；
- 更新后的高程图必须人工审阅，不只机械重录；
- 实际 release 应用中检查：
  - 热点形成局部海山、岛屿或火山高地，可受当前速度塑造成短程方向性组团，但不形成带年龄的历史轨迹；
  - 基岩省边界与构造、地壳和地貌有关，不是独立噪声拼图；
  - 金属、地热和沉积潜势可重叠且各自具有不同空间结构；
  - 没有椭圆大陆、空白洞、NaN 颜色或几何错位；
  - 新种子重建全部自然字段，字段切换不重建世界。

### 11.6 平台与性能

- native debug/release 全目标测试；
- rustfmt、Clippy `-D warnings` 和全特性 check；
- `wasm32-unknown-unknown` 全特性库检查；
- Trunk build；
- 默认 20,000 单元 release 性能基线；
- 热点算法上界为 `O(H × (cells + edges))`，其中 `H <= 16`；
- 地质算法内存为 `O(cells + edges + hotspots)`；
- UI 静态帧不重建网格或自然字段。

## 12. 迁移与兼容边界

- 保留公开函数名 `natural_foundation_graph`，但文档说明它现在返回扩展后的正式自然基础图；
- `ReliefSnapshot` schema 升为 V2，不提供错误的隐式 V1 反序列化；
- 现有 `elevation_m@1` 字段 ID 保持，因为值类型、单位、范围和“最终高程”语义不变；
- 固定截图基线只在人工审阅新火山分量后更新；
- 旧 `terrain` 模块继续编译，但不得成为地质数据源；
- 本切片不发布完整 `NaturalSnapshot`，直到气候、水文、土壤、生态和资源核心段齐备；
- 下一个自然切片可以只消费：
  - `SpatialArtifact`
  - `ReliefArtifact`
  - `GeologicArtifact`
  - 经规则投影的气候输入

## 13. 自审记录

### 13.1 占位符

- 无占位文字、空实现或未决定的必需契约；
- 延后内容均有明确所有权边界。

### 13.2 内部一致性

- 热点在地貌之前生成，避免地质后处理反向修改高程；
- 地貌仍只有一个权威写入阶段；
- 地质阶段只读全部上游产物；
- 资源潜势不是资源实体；
- 基岩不是土壤或松散沉积覆盖；
- 没有历史时间、年龄序列、旧火山实体或持久化热点轨迹；当前速度只调制当前地貌形态。

### 13.3 范围

- 一个规则模型接缝、一个深部产物、一个地貌 schema 升级和一个地表地质产物构成单一可验收切片；
- 水文、侵蚀、气候、魔法和具体资源均排除；
- 不新增 UI 编辑器或新 GPU 图元类型。

### 13.4 歧义

- `potential` 明确定义为相对地质许可程度，不是概率、储量或价值；
- `relative_permeability` 明确定义为归一化求解输入，不是绝对物理渗透率；
- `BedrockKind` 明确定义为宽泛地质省，不是地层单位；
- `hotspot_count` 明确定义为当前地图中心数，不是现实全球统计；
- 火山高程只归 `volcanic_offset_m`，构造弧/裂谷原有地貌仍归
  `tectonic_offset_m`。

## 14. 完成定义

只有同时满足以下条件，本切片才完成：

- 规则包通过唯一世界法则能力选择地质模型；
- 阶段图生成通过验证的 `MantleArtifact`、Relief V2 和 `GeologicArtifact`；
- 热点独立于板块生成并显式影响高程；
- 基岩、裂隙、抗蚀性、渗透性和三类形成潜势可独立查看；
- 缓存边界证明地质输入不会污染空间或构造阶段；
- 固定种子满足契约、性质和视觉质量门；
- native、release、WASM、Trunk、格式和 Clippy 门禁通过；
- 实际应用经过字段切换和新种子重建检查；
- 变更提交、合并并推送到主分支；
- 合并后的主干 release 再次启动供用户查看。
