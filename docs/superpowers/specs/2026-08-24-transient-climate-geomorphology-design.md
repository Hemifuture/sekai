# P5 瞬态气候—地貌共演与全水圈守恒设计

日期：2026-08-24
状态：**冻结**（用户批准会话 `01a028a0-9ac5-7583-8e30-d73f4958d6d9`
的最终结论及建议）
替代：`2026-08-23-p5-coupling-stability-design.md` 的动态 Aitken 路线
上游：`2026-08-23-p4-physical-budget-correction-design.md`、
`2026-08-18-coupled-geomorphic-formation-p5-design.md`

## 1. 冻结结论

P5 不再寻找“从 P3 重启 100 ka 地貌历史”的气候—地形终态不动点。生产语义
改为：P3 只初始化一次，P4 在当前地形上完成快时间尺度平衡，P5 从当前完整
地貌状态向前推进一个慢时间窗，再在新状态上重求水面与气候，直到累计
`SURFACE_FORMATION_HORIZON_YEARS`。

本设计冻结四个不可拆开的因果边界：

1. 一个统一的亚格元水面几何算子同时产生水体积、陆海面积分数、共享边湿润
   比例和离散海陆拓扑；P3、P4、P5 与 UI 不再各自解释海岸。
2. P5 保留并推进同一份高程组成、沉积、陆面水库与水文状态，不在耦合窗之间
   回到 P3。
3. 地貌时间窗以整步/两个半步的生产算子比较控制时间离散误差；经验地球统计
   只做诊断，不能替代数值误差门禁。
4. `water_inventory_ratio` 的产品语义升级为**可迁移全水圈库存**；海洋、湖泊、
   土壤水、地下水、雪冰和大气水共同守恒，实际海洋体积另行发布。

本次偏离的是旧实现，不是物理方程。玩家仍可选择非地球水量；机制必须满足
守恒、非负和因果前向演化。

## 2. 废止终态不动点的实测证据

旧映射为：

```text
C(x) = 在地形 x 上求完整 P4
G(c) = 使用气候 c、每次从同一个 P3 重跑完整 100 ka P5
F(x) = G(C(x))
目标：F(x*) = x*
```

Draft/seed 7 的原始 Picard 轨迹稳定进入二周期：相邻轮
`normalized_max` 为
`14.2779 -> 2.5166 -> 2.5473 -> 2.5502 -> ...`，隔一轮比较则为
`0.891808 -> 0.087302 -> 0.013872 -> 0.001884 -> ...`。这不是少算几轮，
而是 `A -> B -> A -> B`。

未提交动态 Aitken 原型进一步给出：

```text
omega: 1.0 -> 0.986785 -> 0.493519 -> 0.327098 -> 0.140482 -> 0.404956
raw-candidate residual: 0.0535
同一时刻真实接口残差 F(x)-x: 3.681727 m RMS
强制未松弛复核: 2.5438 normalized_max
```

因此低 raw-candidate 残差没有证明松弛接口是根；Aitken 只松弛连续高程，也
不能同步表示半条河、半个湖泊出口或半份沉积来源。更强 Anderson/IQN/SNES
仍只能寻找已定义方程的根，不能把抹除历史的终态方程变成真实地理过程。

这些失败轨迹保留为设计证据；对应原型代码、V2 Aitken wire 和求解器 UI
不得进入生产。

## 3. 权威状态与时间因果

时刻 `t` 的生产状态 `S(t)` 至少拥有：

- `FormationElevationComponents` 的全部累计组成和最终高程；
- 沉积厚度、来源比例、在途质量与累计质量账本；
- 陆面雪水、根区土壤水和慢地下水库；
- 当前海洋、湖泊和大气水量；
- 由当前水面几何派生的海陆分数、湿边与离散水文拓扑；
- 当前地形/水库条件下的完整 P4 快平衡气候。

唯一生产流程为：

```text
S(0) <- P3 初始化一次
while t < horizon:
    水圈库存分配 + 统一水面几何
    C(t) <- 当前 S(t) 上的 P4 快平衡
    月尺度陆面水量与 C(t) 交换 P / ET / runoff
    S_trial <- 从 S(t) 前向推进地貌 dt
    以整步/两半步比较决定接受或缩短 dt
    S(t + dt) <- 接受的两半步状态
```

接受后只保留时间更细的两半步结果。拒绝步不改变已发布/已接受状态，不产生
artifact。取消也必须保持原子性。

## 4. 统一亚格元水面几何 SSOT

### 4.1 连续地形重建

权威球面格元已有共享顶点和共享边。水面算子采用确定性的 P1 分片线性模型：

1. 每个共享顶点从所有相邻格元中心高程作 Shepard 距离权重凸组合；权重只由
   权威球面几何决定，不引入地学系数。
2. 每个格元以其球面质心和有序边界顶点组成三角扇；每个扇三角上的高程按
   重心坐标线性变化。
3. 给定海平面后，在参考三角形上按零水深线裁剪；湿面积和线性水深积分解析
   计算。球面三角面积用于物理缩放，并按格元存储面积归一，保证所有扇面积
   严格回收到该格元面积。
4. 共享边湿润比例只由同一对共享顶点高程与海平面求得，因而两侧格元逐位
   一致。

一个生产调用同时派生：

- `ocean_area_fraction` 与 `land_area_fraction = 1 - ocean_area_fraction`；
- `wet_edge_fraction`；
- 每格湿面积、水体积和平均湿区水深；
- 总水体积；
- 以格元中心是否被淹没定义的 `LandOceanField`，仅服务离散水文终端。

面积分数和湿边连续变化；海峡开闭、分水岭改变等真实拓扑事件仍允许离散
发生，但不再让一个格元的全部通量因厘米级水线变化同时翻转。

### 4.2 海平面求解

`water_volume_at_sea_level_m3` 和海平面求解必须调用同一亚格元积分，旧的
“平顶格元水柱”公式不再作为第二份事实源。体积函数连续、单调；求解区间由
生产高程物理域和库存给出，停止条件只来自既有水量闭合与浮点可分辨性。

P4 单元陆海分数由权威源分数保守重映射；P4 工作网格共享边通透性由工作网格
上的同一 P1 湿边几何生成，不再使用两格 water fraction 的 `min`。P5 海岸
交换、P4 蒸发/反照率和 UI 海岸字段消费同一结果。

## 5. 多速率前向共演与时间误差

气候平衡相对地貌演化快多个数量级，因此 P4 是每个慢耦合窗的快组件，P5
是保留历史的慢组件。既有 `SURFACE_FORMATION_MACRO_STEP_YEARS` 只作为首次
试探步，不是保证精度的常量。

对每个试探 `dt`：

- 分支 A：在起点气候强迫下推进一个整步；
- 分支 B：推进半步，在中点重求水面/P4/陆面水量，再推进第二个半步；
- 用生产字段和生产面积计算高程组成、沉积质量、水库和水文拓扑的时间误差；
- 若误差不满足冻结精度，丢弃两个分支并缩步；满足时接受分支 B；
- `dt/2`、`dt/4` 语料必须证明误差随缩步下降，不能用输出重映射伪造收敛。

误差归一尺度和最小时间窗在 Task 4 先用生产种子测量，再依据方法阶数、已有
量化语义和文献钉入本规格修订；本版不先拍数值。资源预算只限制工作量，不能
让未达时间精度的状态发布。

## 6. 守恒陆面水量

在尚无生物群系的阶段，最小陆面模型只表示可辨识的物理水库，不伪造植被：

- 雪水当量 `S_snow`；
- 单层根区/活动层土壤水 `S_soil`；
- 慢地下水库 `S_groundwater`。

每格每月满足：

```text
P_rain + melt
  = ET + Q_fast + recharge + delta(S_soil)
P_snow - melt = delta(S_snow)
recharge = Q_base + delta(S_groundwater)
Q_total = Q_fast + Q_base
```

实际 ET 由 P4 的辐射、温度、湿度和风给出潜在需求，并受可用土壤水限制；
入渗/补给受基质渗透性与饱和度限制；地下水连续释放基流。具体相变、容量、
排泄时间尺度和陆面空气动力参数必须先跑生产探针，再从同行评审陆面模型或
数据集取值并以修订条目冻结。所有数值常量进入 `src/world/`，测试复用生产
helper。

`Q_total` 驱动汇流和侵蚀；ET 作为潜热和水汽源回到 P4。不得继续使用
`precipitation * [0.15 + 0.70 * (1 - permeability)]` 作为最终径流机制，也
不得用提高河流阈值掩盖缺失水量过程。

## 7. 全水圈库存语义

用户裁定 `water_inventory_ratio` 代表相对地球参考的可迁移总库存。任一接受
状态必须满足：

```text
V_total = V_ocean + V_lake + V_soil + V_groundwater
          + V_snow_ice + V_atmosphere
```

所有项非负；大气、陆面和海洋之间的通量成对记账。海平面只使用当时的
`V_ocean`，不再把总库存每次全部灌回海洋。P3 初始分配、P4 大气初始化与陆面
空/平衡启动策略必须在生产探针后冻结，且默认世界的总库存仍严格等于
`water_inventory_ratio` 对应值。

UI 将原“海水量”更名为“可迁移水库存”，并独立显示实际海洋体积及各水库
占比。合法范围与建议带仍属于世界参数事实源；不因这次语义升级复制数值。

## 8. Schema、身份与模块边界

- `world` 拥有水面几何 payload、水库状态、守恒报告和有出处常量；
  `generators` 拥有重建、裁剪、求根、月水量和时间推进算法。
- P4 只通过验证后的连续地表 forcing 读取地形/水库，不读取 P5 内部累计器。
- P5 snapshot 升版只在第一个真实新 wire 提交发生；废止的 Aitken V2 从未提交，
  不占用 schema 版本。
- model/stage/fingerprint 在方程实际改变的任务中刷新；P0–P2 不因下游改变而
  刷新。P3 水面事实、P4 forcing、P5/T1 和呈现金样按实测因果链登记。
- checkpoint 必须保存前向状态、累计时间和库存，不能再用 outer fixed-point
  iteration 表示工作量。

## 9. UI 交付

地图与球面字段注册表至少增加：连续海洋面积分数、土壤水、地下水、雪水、
实际 ET、总径流和基流。左侧形成摘要直接复制权威报告，显示：

- 已演化时间、接受/拒绝时间窗、最小时间窗和最大时间误差；
- 总库存、海洋、湖泊、土壤、地下水、雪冰和大气水；
- 降水、ET、快速径流、基流与全水圈闭合；
- 连续陆地面积及离散水文海陆面积。

玩家继续通过现有世界参数和视图字段操作/观察，不增加时间步、误差容差或
求解器旋钮。只有地图、球面和摘要均可见后，本里程碑才进入用户验收。

## 10. 验收边界

硬结构门禁：

- 总水、沉积质量及成对能量通量闭合；
- 全部水库非负、面积分数有界且逐格和为 1；
- 海平面体积函数单调并达到声明闭合；共享湿边两侧逐位相同；
- receiver 邻接、无环、排水面不逆坡；
- 整步/半步时间误差达到冻结精度；拒绝步与取消不污染状态；
- 确定性、schema、checkpoint、artifact 与字段身份一致。

只检测、不拒绝生成：全球降水、陆地比例、平均海深、各水库地球占比、河网
密度、Strahler 分布、侵蚀率、海岸迁移速度、耗时和迭代/重试次数。

## 11. 每项承重技术的出处

- 前向地形—气候共演：Paik & Kim (2021), *Simulating the evolution of the
  topography–climate coupled system*, DOI `10.5194/hess-25-2459-2021`。
- 多速率组件耦合：Gladstone et al. (2021), *The Framework For Ice
  Sheet–Ocean Coupling (FISOC) V1.1*, DOI `10.5194/gmd-14-889-2021`。
- 动态海岸的分数掩膜与守恒：Meccia & Mikolajewicz (2018), *Interactive
  ocean bathymetry and coastlines... MPI-ESM-v1.2*, DOI
  `10.5194/gmd-11-4677-2018`；CMEPS *Fractional grids* 工业实现。
- 共享顶点距离权重：Shepard (1968), *A two-dimensional interpolation
  function for irregularly-spaced data*, DOI `10.1145/800186.810616`。
- P1 线性单元上的裁剪与积分：Dunavant (1985), *High degree efficient
  symmetrical Gaussian quadrature rules for the triangle*, DOI
  `10.1002/nme.1620210612`；本实现对线性湿深使用解析低阶积分。
- 整步/半步误差控制：Hairer, Nørsett & Wanner (1993), *Solving Ordinary
  Differential Equations I*, second edition, Springer，step doubling / local
  extrapolation。
- 陆面水库与全球闭合：Müller Schmied et al. (2021), *WaterGAP v2.2d*,
  DOI `10.5194/gmd-14-1037-2021`。
- 地下水对河网间距和切割的作用：Litwin et al. (2022), DOI
  `10.1029/2021JF006239`。
- 河道起始的面积—坡度关系：Montgomery & Dietrich (1988), DOI
  `10.1038/336232a0`；宽度—流量标度：Leopold & Maddock (1953), USGS
  Professional Paper 252。
- 固定点加速的适用边界：Walker & Ni (2011), DOI `10.1137/10078356X`；
  preCICE *Acceleration configuration*。它们支持“只能求已有根”的否决结论，
  不再作为 100 ka 历史的生产算法。

## 12. R1 修订：统一亚格元水面几何实测冻结（2026-08-24）

Task 3 的生产实现冻结如下；本修订细化 §4，不改变 §1 的批准结论：

1. 顶点重建采用 Shepard (1968) 的 `p = 2` 反距离权重。球面半径对同一顶点的
   所有样本相同，故实现直接使用中心角 `theta_i` 的
   `w_i = 1 / theta_i^2`。为让常数场逐位保持常数，求和写成与原式代数等价的
   锚点差值形式
   `z_v = z_a + sum(w_i * (z_i - z_a)) / sum(w_i)`；这不是新系数或平滑器。
2. 每格球面三角扇先按权威共享顶点计算球面三角面积，再以格元权威面积归一；
   前 `n - 1` 个扇面按比例赋值，最后一个扇面接收精确剩余面积，使每格扇面积
   之和回到唯一的格元面积事实。
3. 每个扇三角使用 P1 线性水深。零水深线与边的交点由端点水深比例解析得到；
   一湿顶点、两湿顶点和全湿三种情形分别直接计算湿面积及
   `integral(max(depth, 0))`，不作采样积分或结果重映射。共享边湿润比例只计算
   一次并按 `EdgeId` 发布。该低阶解析积分与 Dunavant (1985) 的三角形线性
   单元积分事实一致。
4. 海平面在 `f64` 包络内二分，直到中点与某一端点相同，即区间已无可表示的
   内部浮点数；随后量化为 `f32`，并在该值及相邻两个 `f32` 中用同一体积算子
   选择绝对残差最小者，等残差时取数值较小者。不存在经验迭代上限。
5. `physical-land-area-fraction` 从旧格元中心面积统计改为连续 P1 湿区的互补
   面积。按 §10，它是 detection-only observation：保留实测，不以旧
   `0.20..=0.55` 地球经验带拒绝生成，也不另钉一个更低阈值。
6. 旧平顶格元体积公式、排序求根以及 P5 forcing 中的第二份格元中心水深复算
   已删除；体积查询、海平面求根、P3 质量评估和 P5 输入校验消费同一生产算子。

固定 Draft/seed 42 与 17-seed Release 语料的先测后钉结果：

- P3 seed 42 快照 BLAKE3 从
  `051d0907261112e80b59f0c4f014b6a0a9f1d9a5f142b2a74c160ab675b3aede`
  变为
  `4e4dc63c21a61cc0e96ac0c01818ccf9bee7ad87707a97cd5f017a5a19eb6a55`；
  新海平面为 `-64.06 m`，连续陆地面积分数为 `0.206721`。
- 17-seed 连续陆地面积分数中位数为 `0.1943376362323761`；其余八项 P3
  统计指标继续通过。新 evidence JSON/CSV BLAKE3 分别为
  `ce1c9e5343c869bf5493e3f0e42abc0c3df603fa72e755f463c17c5d7bb2c1a6` 与
  `f1c3cc32e57a5a9e727452845cd4de36e7ebb2c8f4936d8b991c39961062132f`。
- 默认 P5 seed 42 Release artifact BLAKE3 从
  `83a67fc6688db690f0a0e691cce280593febbc5b737b26afcb261479717a7f90` 变为
  `04c2e2373c40256f6387565211b33d89989acb4a6fa449422057b199695533bf`；
  T1 probe 从 `20fb2405f60ea634b2153474a06f2103fc059073479ba8414ac297c164e36ea5`
  变为 `a7905840137948fda3e82a0509fef62d4026bc612d9c5ccaf67b0ee421f23271`，
  T1v2 probe 从 `c43a9a2dd66c241cc5d1695cfb7b972d744aba373df37d44dda564facce355c1`
  变为 `6885e498c8b0941914f48177eee606e4bf2b30082abfcff153aaee9997de35f8`。
- `target_land_fraction = 0.38` 的 seed 42 模式中，P3/P5 artifact BLAKE3
  分别从 `8c0ed4313edb4d136c5c41adad879d320ca0f52d87e182ac14cf49fd4021bd27` /
  `95738e6773494eddf765dfccd7117bb259bc5268fd78200ec0cf6c5a1cdc76f8`
  变为 `21a4beda983ad54a65e5ab6ad8e16bccef2aa13969fa5cbb2cfb341f04daeffa` /
  `248d92caefaf50c467e875f015c24fc78fc650cf41fe9be71e2ec245a148c9b1`；
  同算子反求的隐式水量比为 `0.687761329529`，P3 海平面、连续陆地面积分数
  与 P5 海平面分别为 `-1242.500000 m`、`0.379614234`、`-1233.861572 m`。
- dense owner 仍是 profile 权威 surface：Draft/Standard/High 分别为
  `20,252 / 79,212 / 198,812` 格元。Release 探针中 Draft 全 P3 为
  `2,752,963 us`；Standard/High 上游取消延迟为 `372,670 / 851,267 us`，
  均满足既有两秒取消门禁。

## 13. R2 修订：连续海岸贯通 P3、P4 与 P5（2026-08-24）

Task 4 的生产实现冻结如下；本修订细化 §4、§8 和 §10，不改变 §1 的批准
结论：

1. P3 以 `PRIMARY_RELIEF_SCHEMA_V2` 只持有一份权威
   `SurfaceWaterGeometry`。`sea_level_m`、实测水量和离散
   `LandOceanField` 都从该 payload 派生；旧 compatibility payload 仅为既有
   消费者保留，并与权威几何逐位交叉验证，不构成第二事实源。P3 质量评估不再
   重建水面几何。
2. P4 源格元的陆地面积分数定义为
   `l_i = 1 - ocean_area_fraction_i`，工作网格值使用既有守恒球面重映射
   `l_c = sum_i(W_ci l_i)`。工作网格按重映射高程和同一海平面重建 P1 水面，
   有限体积边通透性直接取共享边湿长比例 `k_e = wet_edge_fraction_e`；旧
   `min(1 - l_first, 1 - l_second)` 已删除。只有 `l_i = 1` 的完全干源格才将
   发布海流清零，部分湿格保留海流。
3. P5 在丘坡过程之后、海岸过程之前按固定库存重求水面。对共享边 `e` 上从
   格元 `i` 指向邻格 `j` 的海岸开口，定义
   `A_i->j,e = L_e * wet_edge_fraction_e * land_area_fraction_i`；风与海流暴露
   乘该开口后，以 `i` 的完整格元周长归一。向海洋接收格路由沉积时再乘
   `ocean_area_fraction_j`。这是有限体积控制面边界积分的分数开口离散，不是
   新经验系数；两方向分别计算，使两个部分淹没格元可独立贡献。宏步末在
   沉积与 Airy 响应后的最终高程上再次求解并发布唯一水面几何。
4. P5 形成水文的海洋 terminal 只从该几何派生的 `LandOceanField` 读取；legacy
   平面/球面水文继续保持原厘米高程分类语义。亚格元面积或湿边变化因此连续
   改变气候与海岸交换，而格元中心跨过海平面时仍产生显式、合法的水文拓扑
   事件。
5. 身份随真实契约变化升级：`PrimaryReliefStage` 为 v2；
   `GlobalCirculationStage` 为 v4，forcing/model tag 分别为
   `sekai.global-climate-forcing.v3` / `sekai.global-circulation-equations.v6`；
   `FORMATION_TERRAIN_FIELDS_SCHEMA_V2`、
   `NATURAL_SURFACE_FORMATION_SCHEMA_V2`、`SurfaceFormationStage` v2，以及
   surface-formation equation/state tag v2 同步生效。无人消费的旧 V1 schema
   常量已删除；V1 wire 由 V2 validator 明确拒绝，不保留投机性迁移 API。
6. `PrimaryReliefStage.version()` 同时进入 cache identity 与该阶段 RNG seed。
   因输出 schema 已改变，保留 v1 会让旧 cache identity 与 V2 wire 冲突；因此
   本修订明确接受 v2 带来的 P3 随机语料刷新，不伪装成“只改序列化、不改
   地形”，也不以回退版本号恢复 Task 3 语料。

固定库存 `0.181_f32 m` 平移探针使用 Higham (2002) 的 round-to-nearest 标准
模型，而不要求不同量级的 `f32` 加法逐位平移。令 `u = f32::EPSILON / 2`，
测试上界为：输入加法项 `u max_i|fl(z_i + delta)|`；海平面求解器因只在最近
量化值及相邻两个 `f32` 中择优，其两个发布值合计项为
`3u(|s_0| + |s_1|)`；最终减法项为 `u|fl(s_1 - s_0)|`。实测海平面平移为
`0.1809998 m`，与 `0.181_f32 m` 相差 `2.3841858e-7 m`，落在上述随输入尺度
计算的表示误差包络内；连续海洋面积分数、共享边湿润比例、离散分类和总水量
仍逐位不变。独立亚格元探针同时证明：离散分类不变时，海岸侵蚀与交换质量已
连续改变。

固定 Draft/seed 42 Release 语料的先测后钉结果：

- 默认 P3 snapshot BLAKE3 从
  `4e4dc63c21a61cc0e96ac0c01818ccf9bee7ad87707a97cd5f017a5a19eb6a55`
  变为
  `caa867e9e83ab3413600fdce83e2275bd2fe176580a2d93120c4b1a887441582`；
  默认 P5 artifact 从
  `04c2e2373c40256f6387565211b33d89989acb4a6fa449422057b199695533bf`
  变为
  `14b21a1a863408fcfdac56c78ad2ab82d9994b82695ae86d5ea6e152d8f62437`。
- T1 probe 从
  `a7905840137948fda3e82a0509fef62d4026bc612d9c5ccaf67b0ee421f23271`
  变为
  `d2d966fee7e699e3d84c7396c4476e46fbe8052edaca48ab1bab8e6924393ee6`；
  T1v2 probe 从
  `6885e498c8b0941914f48177eee606e4bf2b30082abfcff153aaee9997de35f8`
  变为
  `8da656cc94f754f92e8ef062c19216d25700a5167684ba890b8951777cba8863`。
- `target_land_fraction = 0.38` 的 seed 42 模式中，P3/P5 artifact BLAKE3
  分别从
  `21a4beda983ad54a65e5ab6ad8e16bccef2aa13969fa5cbb2cfb341f04daeffa` /
  `248d92caefaf50c467e875f015c24fc78fc650cf41fe9be71e2ec245a148c9b1`
  变为
  `6da4f8574c6818933779dce65364eeb3b8fcc9f19add3f97c8db034f2b67170f` /
  `976f72985b2b11569ff217f243182224f83b2b2fc92461e8e4aea70b4b239c23`；
  同算子反求的隐式水量比从 `0.687761329529` 变为 `0.691398882131`，P3
  海平面、连续陆地面积分数与 P5 海平面分别从 `-1242.500000 m`、
  `0.379614234`、`-1233.861572 m` 变为 `-1228.500000 m`、`0.379475296`、
  `-1218.276733 m`。

本修订各承重技术的出处：

- 动态分数海岸与守恒交换沿用 Meccia & Mikolajewicz (2018)，DOI
  `10.5194/gmd-11-4677-2018`，以及 CMEPS *Fractional grids* 的工业语义：
  耦合场保留 `[0, 1]` 面积分数，不先二值化再交换。
- 分数开口乘共享边长来自有限体积控制面的边界通量积分；见 Eymard, Gallouët
  & Herbin (2000), *Finite Volume Methods*, Handbook of Numerical Analysis
  VII, DOI `10.1016/S1570-8659(00)07005-8`。球面共享边一次计通量并由两侧消费
  的实现依据沿用 Skamarock & Gassmann (2011), DOI
  `10.1175/MWR-D-10-05056.1`。
- 浮点包络采用 Higham (2002), *Accuracy and Stability of Numerical
  Algorithms*, second edition, SIAM, DOI `10.1137/1.9780898718027` 的
  `fl(x op y) = (x op y)(1 + delta)`、`|delta| <= u` 标准模型；该包络只描述
  表示误差，不改变任何物理验收阈值。
