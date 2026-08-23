# P5 气候—地貌耦合稳定化设计

日期：2026-08-23  
状态：**冻结**（2026-08-23 用户批准）  
上游：
`2026-08-18-coupled-geomorphic-formation-p5-design.md`、
`2026-08-23-p4-physical-budget-correction-design.md`、
`2026-08-23-natural-world-scientific-remediation-roadmap.md` 的 R2a

## 1. 结论

P4 水热校正后的生产强迫暴露了 P5 原始 Picard 外循环的稳定二周期。修复采用
**球面面积加权、受保护的向量 Aitken 动态欠松弛**，只作用于 P4 与 P5 之间
私有的最终地形高程接口。每个松弛接口都用 P3 固定
`water_inventory_m3` 重新求解物理海平面和 `LandOceanField`；河网、沉积、
地貌组成、气候快照和任何发布 artifact 都不插值。

发布条件不放宽。加速只负责寻找固定点；最终候选必须再经过一轮不带松弛的
完整生产映射，并继续满足现有五分量残差、守恒、拓扑和确定性契约。经验地球
范围只进入证据和 UI 提示，不钳制、不拒绝生成。

本规格只完成 R2a 数值耦合前置修复，不实现 R2b 的 ET、土壤水、地下水、
雪冰，也不改变 R2c 的河道起始与 hydraulic geometry。

## 2. 生产故障证据

在提交 `5c2a373` 上，通过应用同一路径运行 Draft/seed 7，原始相邻轮
`normalized_max` 为：

```text
14.2779 → 2.5166 → 2.5473 → 2.5502 → 2.5503 → 2.5503 → 2.5505 → 2.5505
```

同一批完整候选改为和前两轮比较时：

```text
0.891808 → 0.087302 → 0.013872 → 0.001884 → 0.002005 → 0.002087
```

这证明轨迹收敛到 `A → B → A → B`，而不是预算不足。末端相邻两相约有
`10.884 m` 高程 RMS、`0.006399` 接收者变化面积、`0.53123`
`log1p(discharge)` RMS、`25.505 m` 沉积厚度 RMS 和 `0.000681` 岸线变化
面积。增加轮数不会消除二周期；放宽残差只会按奇偶轮任意接受一相，禁止这样
处理。

## 3. 固定点与接口

令：

- `C(x)` 为在地形接口 `x` 上运行完整生产 P4；
- `G(c)` 为使用气候 `c`、每次都从冻结 P3 地形开始的完整 8 宏步 P5；
- `F(x) = G(C(x))`。

所求固定点满足 `F(x*) ≈ x*`。原实现是 `x[k+1] = F(x[k])`，即松弛系数
恒为 1。

唯一 Aitken primary data 是权威球面单元上的最终地形高程。P4 从 P5 读取的
地形事实也恰好只有：

- `final_elevation_m`；
- 由固定水量求出的 `sea_level_m`；
- 与二者一致的 `LandOceanField`。

地形梯度、相对高程、海深和 forcing fingerprint 继续由现有生产 P4 builder
从这三个事实推导。私有接口不进入 engine cache、checkpoint、artifact 或
公开字段。

## 4. 动态 Aitken 公式

第 `k` 轮原始接口残差为：

```text
r[k] = F(x[k]) - x[k]
```

球面面积加权内积为：

```text
<a,b>_A = sum_i(cell_area_i * a_i * b_i)
```

从第二个可用残差开始，采用 Irons–Tuck 向量 Aitken 更新：

```text
delta_r = r[k] - r[k-1]
omega[k] = -omega[k-1]
           * <r[k-1], delta_r>_A
           / <delta_r, delta_r>_A

x[k+1] = x[k] + omega[k] * r[k]
```

第一轮 `omega = 1`，逐位复用今日未松弛行为，不新增经验初值。点积按稳定
`CellId` 顺序使用 `f64`；高程接口保留为 P4/P5 生产精度 `f32`。当
`omega == 1` 时直接复制候选，避免代数重算改变位模式。

## 5. 受保护更新

本任务只允许欠松弛，不做地形外推：

- 新系数必须有限且在 `(0, 1]`；
- Aitken 分母为零、非有限或新系数越界时，沿用上一个有效正系数；
- 初始有效系数唯一为 1，不增加最小系数、随机扰动或 seed 特判；
- 凸组合保证私有高程不越出两端生产地形的逐格包络；
- 若受保护 Aitken 仍不能在既有计算预算内闭合，则 typed failure，绝不发布
  最后猜测。

这些是求解器结构约束，不是地理经验门禁。`SURFACE_FORMATION_MAX_OUTER_ITERATIONS`
仍为有限资源预算，本任务不以加轮数代替稳定化。

## 6. 每轮状态机与未松弛复核

初始接口是 P3 地形，初始气候是已发布 P4，因此第一轮天然是未松弛映射。
每轮执行：

```text
current climate
  -> 从同一 P3 重跑完整 P5，得到 raw candidate
  -> 在 raw candidate 上重求固定水量水线
  -> 在 raw candidate 上重跑完整 P4
  -> 用 candidate climate 生成 final hydrology
  -> 与上一 raw candidate 计算现有五分量残差
```

只有“本轮输入接口逐位等于上一 raw candidate”且五分量残差通过时才允许发布。
若残差在一个松弛接口之后通过，下一轮强制 `omega = 1`：直接以本轮 raw
candidate 及其 P4 作为输入，形成未松弛复核。复核失败则继续 Aitken；预算
耗尽时错误必须说明仍待未松弛复核。

因此最终发布的地形始终来自完整 `G`，气候始终来自该候选上的完整 `C`，水文
始终使用这份候选气候；松弛高程本身永不发布。

## 7. 物理水线与 P4 私有边界

每个 `omega < 1` 的接口按以下顺序构造：

1. 对两端有效 `f32` 高程做凸组合；
2. 调用现有 `FormationSeaLevelSolver`，以 P3 的
   `water_inventory_m3` 求出海平面、实现水量和陆海分类；
3. 用窄的 crate-private P4 builder 接收
   `elevation_m + sea_level_m + LandOceanField`；
4. 复用现有 `climate_terrain_fingerprint_impl`、保守 remap、海深、相对高程
   和地形梯度生产路径。

禁止为中间接口伪造 `FormationElevationComponents`、沉积厚度或 provenance。
`omega == 1` 时复用已经在 raw candidate 上生成的 P4，避免重复求解。

## 8. 报告、schema 与身份

新增严格 `FormationCouplingReport`：

- `method = dynamic-aitken-v1`；
- `interface_relaxation_factors`：每次通向下一轮的实际系数，最多 7 个；
- `final_unrelaxed_verification = true`。

它提供派生的松弛更新次数、最小/最近动态系数，供证据与 UI 直接使用。向量
长度必须等于 `outer_iterations - 1`，每项必须有限且在 `(0,1]`，成功产品的
未松弛复核必须为真。

身份变化：

- `NaturalSurfaceFormationSnapshot` 升为 V2；V1 严格拒绝，不做静默默认；
- `SurfaceFormationStage.version()` 升为 2；
- `surface_formation_model_fingerprint()` 升至 equation domain V2，并纳入
  Aitken 公式、面积权重、凸组合保护和未松弛复核语义；
- checkpoint wire 不变，但 model/checkpoint fingerprint 必然刷新；
- formation terrain、P4 单独 artifact、P0–P4 stage identity 与字段注册表不因
  私有耦合算法改变；
- P5 artifact、其质量报告 subject、build result，以及实际消费改变后 P5 的
  T1/呈现金样按因果链刷新。

形成侧保守 dense-owner 清单增加每格一份 `f32` 接口高程、一份 `f64` 上轮
残差和一份 `u8` 私有陆海分类；不保存 Anderson 历史矩阵。

## 9. UI 交付

`FormationAreaSummary` 增加只读 `P5CouplingSummary`，直接复制最终
`FormationSolveReport`，左侧 P5 摘要显示：

```text
气候—地貌耦合：动态 Aitken
外层求解：k / 8 轮
松弛更新：n 次｜最小系数 omega
未松弛复核：通过
最终最大残差：value
```

标签进入 localization SSOT。算法没有玩家物理语义，因此不增加系数旋钮；
用户通过生成、换种子/分辨率/世界参数来操作同一生产算法。地图和球面继续
共享一个 formation document 和同一摘要。

## 10. 验收与证据

### 10.1 数值与物理契约

- 合成 `F(x) = -x` 二周期经生产 Aitken helper 求得 `omega = 0.5` 并到达
  固定点；
- `omega = 1` 逐位复制候选；退化/越界系数走确定性保护；
- 每个松弛接口的水量闭合和陆海分类由生产水线求解器重建；
- 私有接口 P4 forcing 与相同完整 terrain 的 forcing 逐位相同；
- 只有未松弛复核轮可以发布；错误预算不发布部分 artifact；
- seed 7 在既有 8 轮预算内收敛，至少使用一次 `omega < 1`；重复运行逐位
  相同；seed 42/3 继续通过相同生产契约。

### 10.2 检测而非钳制

17 粒 evidence 记录每粒轮数、系数轨迹、松弛次数、最终五分量残差、耗时和
artifact/checkpoint 指纹。地球形态、河网密度及水热参考继续作为测量；本任务
不因经验偏离改变算法输出。

### 10.3 性能

运行 P4 的次数、完整 P5 次数和 wall clock 均写入 release 证据。简单收敛
路径在 `omega = 1` 时复用 candidate P4；困难路径允许为松弛接口多算 P4，
但必须比原 8 轮失败有界且不违反既有内存/取消契约。

## 11. 技术出处

- Irons & Tuck (1969), *A version of the Aitken accelerator for computer
  iteration*, DOI `10.1002/nme.1620010306`：向量 Aitken 与单个额外残差向量。
- Degroote et al. (2010), *Performance of partitioned procedures in
  fluid-structure interaction*, DOI `10.1016/j.compstruc.2009.12.006`：黑盒分区
  固定点、Aitken 与准牛顿比较。
- preCICE, *Acceleration configuration*,
  <https://precice.org/configuration-acceleration>：工业隐式耦合的 fixed-point
  加速、primary/secondary coupling data 与动态 Aitken 实践。
