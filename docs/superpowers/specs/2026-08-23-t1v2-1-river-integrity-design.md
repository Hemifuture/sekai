# T1 v2.1 设计规格：河网几何完整性与尺度化呈现（2026-08-23）

状态：**已冻结**（2026-08-23 用户批准整改顺序）。本规格是
`2026-08-20-t1v2-hierarchical-derivation.md` 的窄修订，仅替换其 A6/A8
中与河段几何边界、河宽传递和显示选择有关的契约；T1 其余图元派生、P5
水文拓扑和形成地貌不变。实现偏离只能以显式修订条目记录于 §10。

## 1. 已测基线与问题定位

权威 seed 42 P5 工件（BLAKE3 `83a67fc6688db690f0a0e691cce280593febbc5b737b26afcb261479717a7f90`）
包含 20,252 个格元、4,281 个陆格、3,155 条河段，最大 Strahler 级为 4。
P5 通过 Priority-Flood、单一 receiver 和循环检查产生有向无环排水图；按
P5 的格元水体语义检查，没有陆地河段穿越海洋。因此用户看到的“流入水域
又流回”“反复交叉”不能归因于 P5 逆流。

代码审计定位到 T1：`hierarchical_rivers.rs` 对每个质心—质心河段独立做
中点位移，约束只有自身 L0 大圆弦走廊；它不知道共享格边、水体多边形和
相邻河段。`app.rs` 又把 `RiverReach::half_width_m` 丢弃，按 Strahler 级
重造固定像素宽，并在所有缩放级显示全部河段。现状没有任何机制能保证
水边裁线、网络平面性或物理宽度。

本规格以“生产拓扑正确、派生几何缺少约束”为根因；禁止通过提高 P5
`DEFAULT_CHANNEL_DISCHARGE_THRESHOLD_M3_S` 掩盖问题。

## 2. 范围与非目标

本次交付：

1. 以 P5 相邻格共享边为唯一跨格门户；
2. 陆地河槽按水体边界裁线；
3. 保证路径无自交、无回折，跨河相交只发生在 P5 合法节点；
4. 河宽从同一生产事实进入雕刻、显示图元和 GPU；
5. 全球视图按 Strahler 层级抽稀，放大逐级恢复完整河网；
6. 地图与球面视图同步交付和验收。

明确不做：不改变 P5 receiver、流量、河级、湖泊或工件 wire；不添加新的
支流；不重标定降水、径流率、河道起始阈值或 hydraulic-geometry 系数；
不为湖内伪造人工中心线。后两项分别属于路线图 R1/R2。

## 3. 权威边界与数据流

```text
P5 RiverSegment + SurfaceWaterKind
        │（拓扑、流量、河级：权威事实）
        ▼
TerrainAmplifier::with_rivers
        │（共享边门户、陆地 legs、河床、物理宽度）
        ▼
HierarchicalRiverEvaluator
        │（受约束路径；雕刻与显示同源）
        ├──────────────► T1 图元高程雕刻
        └──────────────► RiverPolylineSegment { width_m }
                                  │
                                  ▼
                         地图/球面 GPU 投影宽度
```

- `world` 中的 `SurfaceWaterKind` 与 P5 `RiverSegment` 仍是语义事实源。
- `generators` 只做路径和宽度派生；`app` 只根据 LOD 选择既有河段；`gpu`
  只把米制宽度投影到屏幕。
- 不新增 schema 字段，不改变 P5 artifact 身份。

## 4. 共享边门户与水体裁线

### 4.1 门户

每条 P5 河段的 `from` 与 `to` 必须相邻。实现从权威
`SphericalSurface` 查找两格唯一共享的 `SphericalSurfaceEdge`，门户为该
边的大圆中点。不存在或不唯一时构造失败，禁止回退到不受约束的质心弦。

一个质心—质心河段分为最多两个有向 leg：

- 上游 leg：`from` 质心 → 门户；仅当 `from` 为 `DryLand` 时存在；
- 下游 leg：门户 → `to` 质心；仅当 `to` 为 `DryLand` 时存在。

因此 dry→dry 有两个 leg，dry→water 在岸线门户终止，water→dry（湖泊
出口）从岸线门户起始，water→water 不生成陆地河线和河槽。P5 的湖内连通
仍保留在拓扑中；由于 Sekai 尚无 `ArtificialPath` 语义，本次宁可不画湖内
线，也不把陆地河槽错误穿过水面。

### 4.2 格内扇区

每个 leg 只能位于“格元质心 + 共享边两个端点”组成的权威球面三角扇区。
端点可在边界；所有派生内部点必须位于闭扇区内。不同边的扇区内部不相交，
所以不同河段只能在格元质心处形成与 P5 节点一致的汇流，不能在格内无节点
交叉。

## 5. 无自交、无回折的层级路径

每个 leg 建立以格元质心为投影中心的局部 gnomonic 坐标。所有相关扇区均
远小于半球；gnomonic 把大圆弧映为直线，故球面三角形可用确定性的有向
半平面检查。

路径仍沿用 A6 的确定性三候选地形引导与种子扰动，但候选还必须同时满足：

1. 位于本 leg 的球面扇区；
2. 在 leg 起终点定义的纵轴上严格位于父节点之间；
3. 满足原有蜿蜒走廊。

候选按原有地形代价排序，选择首个合格者；都不合格时使用父大圆弧中点。
中点在 gnomonic 中位于父线段内部，因此回退总是满足约束。递归后顶点的
纵坐标严格递增，折线不可能自交或回折，检查复杂度保持每个节点 O(1)，
不引入全路径 O(n²) 相交扫描。

A8 的四点平滑只在新点仍满足同一扇区、纵向单调和走廊时采用，否则对该点
退回未平滑顶点。河床继续按完整质心—门户—质心累计弧长参数插值，沿下游
不升；`min`-only 雕刻不抬高原地形。

## 6. 河宽唯一事实源

生产侧宽度沿用已冻结 A4 的边界流量幂律和上下界：

```text
width_m = clamp(RIVER_WIDTH_COEFFICIENT × sqrt(mean_discharge_m3_s),
                RIVER_WIDTH_MIN_M,
                RIVER_WIDTH_MAX_M)
```

删除 `RIVER_ORDER_GAIN`。Leopold–Maddock hydraulic geometry 已由流量解释
河宽，Strahler 级又与累计流量相关；两者相乘会对网络层级重复计数，且项目
没有独立证据支持该增益。系数和指数的气候/地质分区重标定明确延后至 R2。

`RiverReach` 的同一 `width_m` 同时供雕刻与 `RiverPolylineSegment` 使用；
显示图元字段由 `strahler_order` 改为 `width_m`。测试不得重新实现宽度公式，
必须读取生产助手或生产 reach 的结果。

## 7. 地图/球面的投影宽度

GPU 宽度为：

```text
display_width_px = max(project_physical_width(width_m, camera, projection),
                       RIVER_RASTER_FLOOR_PX)
```

`RIVER_RASTER_FLOOR_PX` 唯一取 1 像素：这是线段至少覆盖一个栅格样本的
离散化下限，不是视觉调参。地图按当前投影在河段中点两侧投影真实横向
偏移；球面按 `width_m / TerrainAmplifier::radius_m()` 的角宽和当前相机投影
求屏幕宽。
宽度随相机 uniform 连续变化，不因缩放重建或重传整张河网。

## 8. 尺度化河网选择

P5 全河网始终保留。T1 只在构造显示折线时使用当前叶片层级抽稀：

```text
visible(order, leaf_level, max_order) ⇔ order + leaf_level − 1 ≥ max_order
```

即 L1 只显示最大级，之后每放大一级显露一个更低级；当层级达到
`max_order` 时显示全部河段。Strahler 级沿下游不减，因此选择不会留下
“上游可见、下游突然消失”的断尾。规则没有新的经验阈值，并保持相机选择
与河路路径的确定性。

同一河段显示路径深度仍取 from/to 当前叶片层级的较深者并受既有 cap 约束；
地图和球面共用同一 `RiverPolylineSegment` 集合。

## 9. 门禁、性能与 UI 验收

### 9.1 测试门禁

- dry→water 的末点、water→dry 的首点逐位等于共享边门户；水体内部无陆地
  河线/雕刻。
- 所有顶点在权威扇区，纵坐标严格单调；路径无自交，跨河只在 P5 节点相交。
- 河床沿下游不升，雕刻 `min`-only；任意查询顺序逐位一致。
- 相同流量、不同 Strahler 级得到相同物理宽度；图元宽度等于生产 reach。
- LOD 显示集合单调增加且始终保持下游连续；seed 42 L1/L2/L3/L4 的最低
  可见级依次符合 §8。
- 地图与球面在全球尺度命中 1 px 栅格下限，深放大后物理宽度主导。
- P3/P4/P5 工件哈希逐位不变；只刷新实际受影响的 T1/呈现指纹和金样。

### 9.2 性能

约束检查随派生节点 O(1)，路径缓存、深度 cap、并行构建和查询剪枝继续复用
现有实现。不得以全网线段两两求交作为运行时算法。任务收尾复跑既有 M2
构建/深放大预算与全量回归；没有测量前不新增更宽松的时间门禁。

### 9.3 用户 UI 验收

1. 启动原生应用并生成默认 Continents、seed 42。
2. 在“放大地形”地图与球面间切换；全球视图应只显示骨干河网，不再呈现
   密集细线毯。
3. 连续放大同一流域；支流应逐级出现，已显示的下游干流不消失。
4. 观察入海、入湖与出湖处；陆地河线在水边准确终止/起始，水面内不出现
   回折河线。
5. 深放大河道；宽度随流量不同并随缩放增粗，不再始终是同一像素宽，河线
   与实际雕刻河槽重合。
6. 检查汇流区；支流只在共同节点汇入，不发生无节点交叉或自交。

代理测试和离线金样只能作为必要证据，不能替代以上用户验收。

## 10. 修订与指纹记录

### R0 — 初始冻结（2026-08-23）

冻结 §1–§9。预计 P5 及上游 artifact 指纹不变；T1 层级探针、M1 放大器
探针及包含河流的 GPU 金样是否变化，须由实现后的因果 diff 判定。实际刷新
清单、旧值/新值和未变证据将在对应任务提交前追加到本节，禁止预写期望值。

### R1 — 物理角宽半径勘误（2026-08-23，实施计划审计）

§7 原文误写为用仅服务高程色标的 `display_radius_m` 换算球面角宽；实现必须
使用权威 `TerrainAmplifier::radius_m()`。这是一处量纲/事实源勘误，不改变
已冻结产品行为：米制河宽除以行星物理半径才得到弧度。

### R2 — 共享边 leg 实现证据（2026-08-23，Task 1–2）

实现将每条 P5 reach 按共享边门户拆为最多两个 `DryLand` leg；候选与四点
平滑均受球面扇区、gnomonic 纵向单调和 leg 走廊约束，缓存按 reach/leg
独立分域。父规格 A9 的 seed 42、Draft T1 层级探针由
`cab6c758fe2ce2dac477e6d8fb674f73a3863ac66a96a7d411d6a86bc339c7b7`
刷新为
`c43a9a2dd66c241cc5d1695cfb7b972d744aba373df37d44dda564facce355c1`。
因果是河流雕刻路径和无 Strahler 重复增宽改变；L0 高程恒等与深层陆比门禁
仍通过。

同一 release `surface_formation_stage` 运行中，M1 放大器探针仍为
`20fb2405f60ea634b2153474a06f2103fc059073479ba8414ac297c164e36ea5`，
默认 P5 seed 42 工件仍为
`83a67fc6688db690f0a0e691cce280593febbc5b737b26afcb261479717a7f90`；
目标陆比模式 P3/P5 工件也仍为 `8c0ed431…` / `95738e67…`。这证明变化止于
T1 派生，没有反向改写 P5 权威网络或上游产物。§8“较深者”同步勘正为现有
A6 用户反馈修订后的 UI 行为；仅修正文案，不改变实现。

## 11. 每项承重技术的出处

| 技术 | 出处 | 落点 |
| --- | --- | --- |
| 水体边界拆线、交汇成节点、禁止自交/回折、无环网络 | USGS EDH Topology Requirements，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-data-acquisition-specifications-7> | §4、§5、§9 |
| 河线必须留在高程表面可辨识河槽 | USGS EDH Positional Assessment，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-acquisition-specifications-1> | §5 地形候选排序 |
| 顶点沿下游高程不升 | USGS EDH Alignment，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-data-acquisition-specifications-19> | §5 河床门禁 |
| Gnomonic 将球面大圆映为直线，适用范围小于半球 | Snyder (1987), USGS Professional Paper 1395，<https://pubs.usgs.gov/publication/pp1395> | §5 O(1) 单调/扇区证明 |
| 河宽与流量的幂律，系数/指数随位置变化 | Leopold & Maddock (1953), USGS Professional Paper 252，<https://pubs.usgs.gov/publication/pp252> | §6 |
| Strahler 河级定义 | Strahler (1957), *Transactions, AGU* 38(6), 913–920，<https://doi.org/10.1029/TR038i006p00913> | §8 层级抽稀 |
| 河网制图须先做要素选择/密度控制再简化几何 | Stanislawski (2009), USGS SIR 2009-5202，<https://pubs.usgs.gov/sir/2009/5202/> | §8 |
