# Sekai 球面自然产品接入设计（S0B.6）

日期：2026-08-04

状态：已由用户审阅批准；进入实施计划与实现

依据：`2026-08-04-spherical-natural-process-migration-design.md`、S0B.1–S0B.5 已合并实现，以及用户对“球面是新世界唯一事实源，二维地图只是球面派生数据”的确认

范围：把已经完成的球面地表、板块、地幔、地形、地质、初步气候、水文和侵蚀接入正式生成引擎，建立缓存／来源身份和无投影字段文档，隔离旧平面 V1，并完成 S0B 整体验收。本阶段不实现二维投影或三维球体显示。

## 1. 决策摘要

S0B.6 采用“唯一球面生产图、细粒度类型阶段、完整结果原子发布、旧平面隔离兼容”的设计：

- `SphericalSurfaceSnapshot` 是新建世界唯一的几何和拓扑事实源。
- 所有球面自然 Artifact 都携带或封装与该地表完全一致的 `SurfaceRef`；相同单元数不代表同一地表。
- 正式自然生成由六个独立球面阶段组成，复用现有规则解析和形成预设阶段；不建立包含平面／球面分支的万能阶段。
- 引擎保留阶段级缓存，应用只在全部阶段成功并通过交叉验证后发布一个完整只读字段文档。
- 二维地图、三维球体、投影切缝和 GPU 网格都不进入科学图、Artifact、缓存键或世界哈希。
- 旧平面 V1 只保留为冻结的兼容读取与回归路径，不进入新世界创建服务，不提供“平面／球面”产品切换。
- S0C 直接消费本阶段发布的同一球面地表和自然字段，不重新生成或复制科学事实。

## 2. 当前状态与缺口

S0B.1–S0B.5 已经完成：

- 权威闭合球面 Voronoi 地表、稳定 `SurfaceRef`、平面／球面只读表面适配器和统一拓扑索引；
- 球面 Euler 极板块运动、地幔热点、地形和地质；
- 球面纬度、三维切向初步风、非负水汽输送；
- 闭合球面海洋／内流盆地水文和原子“水文 → 侵蚀 → 最终水文”；
- 严格 V2+ 快照、确定性矩阵、极端半径与规模性能门。

当前缺口不是科学算法，而是产品编排：

1. 球面生成器还没有正式 `Artifact` 和 `Stage`；
2. 正式应用图仍只构建平面自然阶段；
3. 现有字段文档把自然字段与 `PreparedCellMesh` 直接绑定，因此不能表示“科学数据已完成、显示投影尚未建立”的球面文档；
4. 缓存虽已按依赖内容哈希，但尚无完整球面图证明不同地表不会误复用；
5. 旧平面兼容路径与未来球面生产入口尚未在产品边界明确分离。

## 3. 唯一权威生产图

正式球面图为：

```text
SphericalSpaceArtifact
  → SphericalSurfaceArtifact
      ├→ SphericalTectonicArtifact ─┐
      └→ SphericalMantleArtifact ───┴→ SphericalReliefArtifact
                                         ├→ SphericalGeologicArtifact ───────────┐
                                         └→ SphericalPreliminaryClimateArtifact ┴→ SphericalHydroErosionArtifact
                                                                                     → SphericalNaturalFieldDocument
```

规则与作者输入仍通过现有外部 Artifact 和解析阶段进入：

- `TectonicSpecArtifact`
- `GeologicSpecArtifact`
- `ClimateSpecArtifact`
- `HydroErosionSpecArtifact`
- `WorldFormationSpecArtifact`
- `RulePackSetArtifact`
- `AuthorConstraintsArtifact`
- 四组规则解析／resolved-input 阶段
- `WorldFormationStage`

这些阶段的语义与几何无关，因此继续作为唯一实现。球面科学阶段只替换几何相关的执行适配器。

### 3.1 稳定 Artifact 与 Stage 身份

新增类型使用互不冲突的稳定键：

| Artifact | Artifact key | Stage ID |
|---|---|---|
| `SphericalTectonicArtifact` | `world.spherical-tectonics` | `natural.spherical-tectonics` |
| `SphericalMantleArtifact` | `world.spherical-mantle` | `natural.spherical-mantle` |
| `SphericalReliefArtifact` | `world.spherical-relief` | `natural.spherical-relief` |
| `SphericalGeologicArtifact` | `world.spherical-geology` | `natural.spherical-geology` |
| `SphericalPreliminaryClimateArtifact` | `world.spherical-preliminary-climate` | `natural.spherical-preliminary-climate` |
| `SphericalHydroErosionArtifact` | `world.spherical-hydro-erosion` | `natural.spherical-hydro-erosion` |

图入口命名为 `spherical_natural_foundation_graph()`。它不通过枚举包装现有平面阶段，也不复用平面输出 key。

六个新球面 Stage 的初始版本均为 `1`、namespace 均为 `sekai.core`。已有球面确定性夹具使用相同的核心阶段身份；实现不得为了接线而改变已经冻结的科学随机流。

### 3.2 精确依赖

- 板块：球面地表、resolved tectonic input、resolved formation；
- 地幔：球面地表、resolved geologic input、resolved formation；
- 地形：球面地表、球面板块、球面地幔；
- 地质：球面地表、球面板块、球面地幔、球面地形、resolved geologic input；
- 初步气候：球面地表、球面地形、resolved climate input；
- 水文／侵蚀：球面地表、球面地形、球面地质、球面初步气候、resolved hydro-erosion input。

每个阶段只能读取声明的依赖。投影、调色板、画布状态、GPU 缓冲和气候工作网格在类型上无法进入这张图。

## 4. 细粒度缓存与完整结果原子发布

不增加一个复制全部快照的“巨大最终 Artifact”。`BuildEngine` 已保证失败时不返回部分 `BuildOutcome`，因此：

- 阶段保持细粒度，局部输入变化只失效真正受影响的阶段；
- 每个存储 Artifact 只拥有自己的快照；
- 构建成功后，组合边界从 `BuildArtifacts` 取得 `Arc<Artifact>`，不复制密集字段；
- 只有组合边界完成全部跨快照验证后，才建立 `SphericalNaturalFieldDocument`；
- 任一阶段或文档验证失败都保留上一个完整文档，不发布半迁移字段，也不退回另一套物理。

字段文档记录只读来源身份：

```text
SphericalNaturalBuildIdentity
├── root_seed
├── surface_ref
├── build_result_hash
└── graph_contract_version
```

该身份用于审计、过期结果判断和未来存档来源说明，不复制科学数组，也不替代各 Artifact 的内容哈希。

## 5. 缓存身份

现有 `StageCacheKey` 已包含阶段 namespace、ID、版本、阶段种子、输出 key 和每个依赖的确定性内容哈希。S0B.6 不再建立第二套缓存协议，而是补齐球面依赖并自动化证明：

- 相同输入和根种子的第二次完整构建命中全部可复用阶段；
- 只更换显示投影、调色板或 GPU 设置时，科学图没有任何输入变化；
- 相同 cell/edge 数、不同 `SurfaceRef` 的地表使所有直接或间接球面自然阶段失效；
- 规则解析与形成选择若输入未变，可跨显示重建继续复用；
- Artifact 在进入缓存前已经完成本地验证；恢复时再次检查精确输出类型，字段文档随后执行完整跨快照验证。

球面自然阶段都直接声明 `SphericalSurfaceArtifact`，即使某个上游自然快照意外具有相同统计量，也不能绕过地表身份进入错误世界。

## 6. 字段文档与显示解耦

### 6.1 两层只读契约

把当前应用字段契约拆成两层：

1. `FieldDocument`：字段 registry、借用 payload、诊断、首选字段和首选范围；
2. `PresentedFieldDocument`：在 `FieldDocument` 基础上额外提供可渲染网格与选取结构。

球面自然文档在 S0B.6 实现第一层，不包含投影或 `PreparedCellMesh`。现有旧平面显示文档实现两层。S0C 再为同一个球面文档增加三维球体或二维投影 presenter；不会改变科学文档本身。

### 6.2 字段映射唯一来源

平面兼容文档和球面文档不各自复制一份“字段 ID → payload”清单。建立一个内部 `NaturalFieldPayloadBundle`，集中完成稳定字段 ID、单位、域和只读数组的映射；两种文档只负责从各自已验证快照填充该 bundle。

`natural_field_registry` 的 schema 构建仍是唯一来源。动态上限通过明确的 registry limits 传入：

- 旧平面 wrapper 保留当前 V1 上限和序列化结果；
- 球面 wrapper 使用权威球面总面积计算汇水面积和最大可能流量上限；
- 字段 ID、单位和语义不因投影改变。

### 6.3 球面向量的只读表示

球面板块速度和初步风的权威值仍是三维切向向量。现有字段查看器只接受二维向量，因此文档建立一个可删除的本地切向显示缓存：

- 以固定行星自转轴建立确定性的局部东／北基；
- 将权威三维切向量点乘到该基，得到 `[east, north]`；
- 极点使用明确、确定性的规范基，不读取地图投影；
- 可从两个分量和局部基重建原切向量，误差受浮点容差约束；
- 缓存不序列化、不进入 Artifact、不参与世界或构建哈希。

这只是坐标表示，不是第二份板块运动或风场事实。

## 7. 旧平面 V1 隔离兼容

保留旧代码是为了不破坏已有数据，不表示产品拥有两个事实源。

- `PlanarSpaceArtifact`、`SpatialArtifact`、平面自然 Artifact、阶段 ID、版本和 wire schema 保持冻结；
- 旧 V1 快照继续严格反序列化并按原规则验证；
- 平面图只由兼容加载器、回归测试和旧文档 presenter 调用；
- 新世界创建服务不注册平面图，也不提供几何模式切换；
- 旧平面文档与新球面 Artifact 不得混装到同一字段文档；
- 旧数据不能自动包裹、周期拼接或按相同单元数解释为球面；
- 若未来提供迁移，只能由用户显式请求重新生成，并产生新的球面身份与来源记录。

当前应用持久化的主要是作者参数而不是完整生成快照。本阶段不把 eframe 设置存储伪装成世界存档。兼容验收覆盖现有 V1 wire、旧应用状态的缺省读取和明确的 legacy 入口；正式项目存档容器以后必须使用显式的 `LegacyPlanarV1`／`SphericalV1` 标签，禁止静默重算。

## 8. 产品切换边界

S0B.6 建立球面权威创建服务和无投影字段文档，但不提供一个无法正确显示的“球面模式”按钮，也不制作临时经纬矩形世界。

- S0B.6 期间现有二维画布明确属于旧平面兼容 presenter；
- 新球面创建服务是后续功能唯一允许调用的自然世界入口；
- S0C 完成真实三维／二维 presenter 后，应用的新建世界入口一次性切到球面；
- 切换时不存在平面／球面选择器，旧文档仅通过兼容加载入口打开；
- S0C 只构建派生显示缓存，不重跑 S0B.6 科学图。

这样既保持每个中间提交可运行，也不会向用户展示一套与实际物理不一致的临时预览。

## 9. 错误与恢复

每个球面阶段使用稳定、阶段特定的错误码，区分：

- 外部或 resolved spec 无效；
- 球面地表无效；
- 上游 `SurfaceRef` 不一致；
- 上游自然快照违反关系；
- 科学生成失败；
- 输出 Artifact 或最终字段文档验证失败。

错误不会触发以下行为：

- 改用平面生成；
- 降低科学验证阈值；
- 发布已完成的部分阶段；
- 用相同单元数掩盖地表身份不一致。

应用组合边界继续采用候选构建和原子替换：失败保留上一次完整、可审计的文档、字段状态和显示缓存。

## 10. 性能与内存

S0B.6 不修改 S0B.2–S0B.5 的科学公式。产品性能要求为：

- 约 20,000 个球面单元的完整 S0B 自然图在 Release 桌面参考机上不超过冻结平面基线的 `2.5×`；
- 相对平面基线的峰值额外工作内存低于 `256 MiB`；
- 每个科学阶段最多建立一次 `NaturalTopologyIndex`；联合水文／侵蚀继续只建立一次并复用三次求解；
- 字段文档持有 Artifact 的 `Arc`，不得复制密集科学字段；
- 新增的局部东／北向量缓存至多为与单元数线性的一份板块速度和一份初步风数组；
- 构建循环不增加按 cell/edge 重复分配，静态字段读取不重建 registry、向量缓存或诊断数组；
- Artifact 哈希、字段文档和未来投影缓存分别计量，不能把序列化缓冲当作常驻世界数据。

## 11. 验收矩阵

### 11.1 图与 Artifact

- 精确的外部 Artifact 集、阶段 ID、版本、输出 key 和依赖表；
- 每个球面 Artifact 严格 serde、未知字段拒绝、本地验证和确定性内容哈希；
- 小型端到端图生成所有六个球面自然 Artifact；
- 文档对每个上游执行完整 `SurfaceRef` 和关系验证。

### 11.2 缓存与确定性

- 相同输入重复构建的阶段命中矩阵；
- 根种子、形成预设和各 spec 的精确失效矩阵；
- 相同单元数、不同半径或不同地表指纹的缓存隔离；
- 同一输入的完整 Artifact 哈希、字段哈希和 `BuildResultHash` 冻结；
- 投影／调色板／GPU 类型无法成为图依赖，并有源代码边界审计。

### 11.3 字段文档

- 所有已发布 cell/edge 字段恰有一个 payload，长度与域一致；
- 球面面积相关范围覆盖实际可能值；
- 局部东／北分量可重建权威三维切向量；
- 字段文档没有投影坐标、重复几何、GPU 数据或可写科学数组；
- S0C 删除所有 presenter 缓存后仍可从同一文档重建显示。

### 11.4 兼容与回归

- 全部现有平面 V1 哈希、serde、阶段图和显示金图无漂移；
- 旧应用状态缺少新字段时仍进入明确 legacy 语义；
- 新世界入口和球面文档中不存在平面 Artifact；
- 不存在把 `SpatialSnapshot` 自动解释为球面的代码路径。

### 11.5 科学与规模

- 端到端重新执行 S0B 已冻结的球面科学矩阵；
- 闭合表面、Euler 刚体运动、切向风、非负水汽、海洋／内流出口和水文邻接性质继续通过；
- Release 20k 全图时间、常驻字段、工作内存和序列化体积分别报告；
- 全仓库 fmt、check、严格 Clippy、全部测试、文档测试和 WASM 检查通过。

## 12. 非目标

- 不在 S0B.6 实现三维球体、二维地图投影、投影逆选取或 GPU 球面网格；
- 不添加平面／球面产品模式切换；
- 不把旧平面多边形拼接、包裹或重采样成球面；
- 不接入最终环流、洋流、ENSO、台风或分层气候；
- 不保存板块或侵蚀历史时间轴；
- 不引入任意几何／任意维度／任意字段存储的万能抽象；
- 不在缺少正式项目文件格式时把 UI 设置存储扩张成隐式世界档案。

## 13. 完成定义

S0B.6 只有在以下条件全部满足后才完成：

1. `SphericalSpaceArtifact` 能通过正式球面自然图生成全部六类自然 Artifact；
2. 所有 Artifact 和最终字段文档绑定同一个准确 `SurfaceRef`；
3. 新世界创建服务只接受球面空间，不存在活动平面分支；
4. 字段文档不依赖任何投影或渲染网格，并能向 S0C 提供完整只读字段；
5. 缓存失效、来源身份、错误恢复和原子发布均有自动化证明；
6. 旧平面 V1 仅存在于隔离兼容域，全部既有哈希、存档契约和显示回归保持通过；
7. 完整球面自然图通过科学、确定性、性能、内存、serde 和全仓库门槛；
8. S0C 可以在不重新生成、复制或修改自然事实的前提下建立二维地图和三维球体。
