# Sekai 因果岛屿与多尺度地貌噪声设计

日期：2026-08-03  
状态：已批准实施（含发布级验收修订）

## 1. 背景

当前自然生成主链已经拥有彼此独立的板块、地壳、构造边界、地幔热点和 Relief 字段，但海洋地貌仍过于干净：大陆地壳之外主要是连续深海，热点只形成宽而圆的平滑隆起，洋—洋俯冲弧也主要表现为连续的构造抬升。结果缺少现实世界常见的海山、火山岛组和离散岛弧。

旧 `src/terrain/noise.rs` 已实现基础 octave/fBm，但它属于旧地形路径并使用 `egui::Pos2`。正式自然生成主链不得反向依赖旧 terrain 或 UI 类型，因此需要一个纯领域、按世界坐标采样的 Relief 内部噪声组件。

## 2. 目标

- 在保持大陆预设宏观形态不变的前提下，为海洋加入有成因的小型岛屿和海山群。
- 使用多尺度 fBm、低频 domain warp、幂塑形和局部 ridged 信号形成自然的宽尺度基座、局部峰顶和不规则轮廓。
- 让热点产生当前切片可解释的火山体和短程方向性岛组；方向只读取当前板块速度，不生成年龄、历史事件或演化轨迹。
- 让洋—洋俯冲边界在既有宽尺度弧形抬升上产生离散岛弧峰；贡献仍属于 `tectonic_offset_m`。
- 保持确定性、封闭地图海洋边框、分量恒等式、性能上界和现有公开字段结构。

## 3. 非目标

- 不在全球海床上直接阈值化噪声生成无成因岛屿。
- 不改变世界形成预设、板块种子、板块归属、速度或边界类型。
- 不生成热点年龄、旧火山年代、历史事件或时间线。
- 不把岛屿形态写入 Mantle、Tectonics 或显示层；这些模块只提供各自已有的因果输入或只读显示。
- 不接入旧 `terrain::noise`，不引入 UI/GPU 依赖。
- 不静默扩张既有 V2 的 `volcanic_offset_m: 0..=4_000 m` 语义；发布级验收确认该范围不足后，以显式 Relief V3 契约把当前上限扩为 `6_000 m`，V2 数据继续按旧上限验证。

## 4. 所有权与依赖方向

```text
WorldFormationPreset ──→ crust macro morphology / narrow mantle bias
TectonicSnapshot ──────→ current boundaries + current plate velocity
MantleSnapshot ────────→ current hotspot centers, strengths, supports
                              │
                              ▼
Relief island morphology ──→ tectonic_offset_m + volcanic_offset_m
                              │
                              ▼
Display / Hydrology / Erosion consume formal fields read-only
```

- Mantle 继续不知道板块、地壳和 Relief。
- Tectonics 继续不知道热点和最终高程。
- Relief 是唯一把当前因果场解释为构造高程、火山高程和正式海陆的阶段。
- 噪声是 Relief 的内部形态调制器，不是新的世界真值字段。

## 5. 多尺度噪声

内部连续噪声采用归一化 fBm：

```text
F(p) = sum(gain^i * noise_i(frequency * lacunarity^i * p)) / sum(gain^i)
```

- `lacunarity` 约为 `2`，频率按 octave 几何增长；
- `gain/persistence` 小于 `1`，振幅按 octave 几何衰减；
- 每个 octave 使用从同一机制种子派生的独立 Perlin 源，并旋转采样坐标，降低格点方向和重复纹理；
- octave 在当前单元分辨率可表达的尺度停止：以世界矩形面积/单元数的平方根估算代表性采样间距，只保留物理波长至少覆盖两个采样间距的 octave，避免用不可见高频制造 aliasing；极粗网格仍保留基础 octave，使已有因果形态不会因降采样完全消失。

低频向量噪声只扭曲采样坐标：

```text
p' = p + warp(p) * bounded_strength
```

warp 强度以局部热点支撑半径或世界短边比例表达，不能把形态移出其因果支撑。

稀疏峰值使用归一化幂曲线而不是无界 `exp`：

```text
peak = saturate(signal)^gamma, gamma > 1
```

ridged 信号只用于火山峰和岛弧峰，不作为全球海床底噪。

## 6. 热点火山岛组

每个 `Hotspot` 独立生成一个局部形态贡献：

1. 从 `source_cell`、`support_radius_m` 和 `strength_permille` 建立紧支撑包络。
2. 洋壳热点包含一个当前中心火山体和一个沿当前所属板块速度方向延伸的短程各向异性支撑；近静止板块只形成径向岛组。
3. 方向性支撑不保存时间、年龄或“旧火山”实体，只是当前地貌对当前运动场的可解释形态响应。
4. 低频 warp 改变中心线和宽度；fBm 形成中尺度起伏；幂塑形/ridged 信号把少数位置推为峰顶。
5. 大陆热点使用更宽、更低、方向性更弱的形态。
6. 多热点使用有界最大/组合，不无界累加；最终贡献在 `volcanic_offset_m` 正式范围内。
7. `mantle.volcanic_influence == 0` 的单元必须保持 `volcanic_offset_m == 0`。
8. Mantle 发布的正影响域是权威支撑；Relief 可在整个域内保留低幅、快速衰减的海山基座，再用欧氏局部核塑造主峰，但不得用第二套距离判定把合法的正影响硬裁成零。

海平面不是噪声阈值的输入。多数受影响洋壳单元仍是海山；只有热点强度、洋壳基准、区域起伏和峰值形态共同位于分布上尾时才露出海面。

## 7. 洋—洋俯冲岛弧

- 只读取已经分类为 `Subduction` 且两侧均为 `CrustKind::Oceanic` 的边界段。
- 既有宽尺度弧抬升保留；新的稀疏峰值从弧侧候选单元中选择。
- 候选分数由独立的多尺度噪声给出，并做幂塑形；只让固定上尾阈值以上的局部极大值成峰，相邻同分候选由较小 `CellId` 决胜；每个有效段始终保留一个稳定最高分候选作为低分辨率兜底。
- 峰值使用比宽尺度弧更窄的紧支撑核，多个候选可形成断续岛链而不是等宽墙体。
- 该贡献只加到 `tectonic_offset_m`；`volcanic_offset_m` 不重复表达俯冲弧。
- 洋陆俯冲可形成大陆火山弧，但不走“海洋岛弧峰”路径。

## 8. 确定性与版本

Relief 新增两个独立标签子流：

```text
relief-hotspot-morphology-v1
relief-island-arc-v1
```

新增标签不能改变 `relief-regional-v1` 或 `relief-tectonic-detail-v1` 的消费结果。热点和边界段始终按稳定 ID 顺序处理，并列用 `CellId` 决胜。任何 `u64` 种子轮转都先在 `u64` 域内取模、再缩窄为单元索引，避免 wasm32 丢弃高位。

首轮岛屿形态算法把 `ReliefStage` 从版本 `5` 提升到 `6`。发布级验收随后发现两项独立的正式语义变化：Mantle 热点中心需要避开封闭地图的人工边缘支撑带；深海火山体需要超过 V2 的 `4_000 m` 上限。代码审阅又确认分辨率截止和岛弧上尾选择属于缓存可观察的生成语义。最终版本契约为：

- `MantleStage` 版本 `3`：热点中心只从距封闭边界至少世界短边 `10%` 的拓扑候选域中选择；候选不足时依次退化为非边界单元和全域。
- `ReliefStage` 版本 `8`：生成 Relief V3，允许 `volcanic_offset_m: 0..=6_000 m`，并启用按物理采样间距截止的 octave 与阈值化岛弧局部峰选择。
- `ReliefSnapshot` V2 仍可读取并按 `0..=4_000 m` 验证，V3 才接受扩展范围；字段、单位与分量恒等式均不变。

`6_000 m` 是保守的深海火山建筑上限而非海拔上限；[USGS](https://www.usgs.gov/faqs/how-big-are-hawaiian-volcanoes) 给出的 Mauna Kea 从洋底基座到峰顶总高约 `10_211 m`。当前模型仍由 `9_000 m` 最终高程上限和分量协调逻辑兜底。

## 9. 边界与性能

- Mantle 使用与影响场传播一致的拓扑距离把热点中心留在人工边缘带之外；新的正向岛屿贡献仍在既有 `apply_closed_ocean_frame` 中衰减，最终外边界单元和东西可见边框继续保持海洋。
- 热点算法复杂度上界为 `O(cells * hotspots)`，热点最多 16 个。
- 岛弧算法只遍历现有边界段、成员边和有限支撑 stamping；候选分数写入复用的单元索引暂存区，局部极大值通过邻接表一次判定，不随已选峰数量二次扫描，保持 `O(cells + edges)` 量级。
- 连续噪声在量化世界坐标或以支撑半径归一化的局部坐标上采样，不按 UI 像素或单元编号采样。

## 10. 验收

自动测试必须证明：

- 相同输入与种子逐字节复现，改变 Relief 岛屿子流会改变形态但不改变 Mantle/Tectonics；
- 热点外没有火山地貌，零热点世界不出现火山岛；
- 洋壳热点可在深海形成至少一个露出峰，同时保留周围海山；
- 洋—洋俯冲比相同强度的洋陆俯冲多出离散正向岛弧峰；
- 非俯冲边界没有岛弧专属贡献；
- 新增形态不改变符号所有权、正式分量恒等式、海洋边框和各字段范围；
- 固定质量种子中，默认多大陆和火山群岛出现与大陆地壳分离、且可归因于热点或洋—洋俯冲的洋壳岛组；
- Debug、Release、Clippy、格式、WASM 和 golden 测试通过。

视觉验收至少检查 `elevation`、`current-surface`、`volcanic-influence`、`tectonic-offset` 和 `volcanic-offset`：岛屿应形成大小不同的组团或断续弧，不得表现为全球椒盐点、等间距珠串、圆形印章或触及封闭地图边框的陆地。
