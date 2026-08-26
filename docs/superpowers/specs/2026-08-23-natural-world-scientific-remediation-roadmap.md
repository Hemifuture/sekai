# 自然世界生成科学性整改路线图（2026-08-23）

状态：**已废止**（2026-08-26）。本文的 R0–R5 执行顺序不再有效，不得再作为
实施顺序、依赖门或任务排队依据。

取代文件：`2026-08-26-natural-geography-short-horizon-roadmap.md`。

废止原因：形成链已改为
`2026-08-24-geologic-pipeline-contract-restoration-design.md` 的 Lie-split
当前态发布（P2 完整时域 → P3 一次投影 → P4 快平衡 → P5 100 kyr → 终点
P4）；产品是地图生成器的最终地理，不是气候/水文预测器。旧顺序把河网显示
和 P4 水热校正放在构造/主地形之前，与当前 UI 病征（陆地破碎、洋底均一）
和 2026-08-25 产品边界冲突。

下文为废止时的原文，仅作历史记录。其中各里程碑的技术出处与已完成的局部规格
（T1 v2.1、P4 校正草案、P5 水文 v2 草案等）仍可被后续工作引用，但不再构成
当前队列。

## 1. 结论（历史原文，2026-08-23）

当前管线的大尺度因果顺序是合理的：P2 构造板块与地壳，P3 形成主地形和
海面，P4 求解大气—海洋气候，P5 以地形和气候形成水文与侵蚀，T1 再把
发布的 L0 事实派生为可缩放细节。现阶段最先需要修的不是 P5 的受水者图，
而是 T1 对河网的几何表达：它把每条 P5 河段独立蜿蜒，既没有以水体边界
裁线，也没有把生产侧物理河宽传到图元和 GPU。随后必须先纠正 P4 的水热
收支和时间语义，才能用可信降水重新标定 P5 的河道起始密度。

因此冻结以下顺序：

| 顺位 | 里程碑 | 解决的问题 | 前置关系 |
| --- | --- | --- | --- |
| R0 | T1 v2.1 河网完整性、物理宽度、尺度化显示 | 交叉、回水假象、入水后又出水的显示、恒定像素宽、全球视图过密 | 立即执行 |
| R1 | P4 科学性校正与 warm start | 真实物理时间/季节、降水偏湿、反照率与能量/水量闭合、Standard 性能债 | R0 后执行；不依赖河密度调参 |
| R2 | P5 水文 v2 标定 | ET、土壤、地下水、雪冰、分辨率一致的河道起始、河宽系数标定 | 必须使用 R1 的可信气候强迫 |
| R3 | P2/T0 构造与主地形结构性修复 | 仍属经验代理的地壳/造山结构与测高分布偏差 | R2 后，避免同时扰动水文标定 |
| R4 | T1 v3 分阶段实现 | 级联上下文、支流与更丰富的亚格点地貌 | R0–R3 先稳定；支流不得放大旧河网缺陷 |
| R5 | M3 生物群系、材质与低优先级呈现 | 地表生态和视觉语义 | 消费前述稳定产物 |

## 2. 为什么不先调河流阈值

P5 seed 42 当前发布 3,155 条河段，最大 Strahler 级为 4；工作树当前
T0 后 17-seed P4 证据（`target/natural-quality/p4/evidence.json`）的全球降水
为 8.87068–11.18974 mm/day（均值 10.29649），而 GPCP v3.1 给出的地球
全球均值为 2.81 mm/day。
当前 `DEFAULT_CHANNEL_DISCHARGE_THRESHOLD_M3_S` 与有效径流代理共同决定
河网密度。若在修正 P4 前只提高阈值，会把上游偏湿折进一个新的经验常量，
使未来校准不可辨识。因此 R0 只修几何、物理宽度传递和显示抽稀；P5 的
权威河网与阈值逐位不动。

## 3. 各里程碑的交付边界

### R0 — T1 v2.1

- P5 河段/受水者关系不变；T1 以共享格边为河流跨格门户。
- 陆地河线在水体多边形边界终止或起始，水体内部不伪造陆地河槽。
- 每条可见路径无自交、无回折；不同路径只在 P5 的合法汇流节点相交。
- 河宽只由生产侧流量—宽度定律给出并进入显示图元；GPU 随投影和缩放显示
  物理宽度，同时保留一个栅格采样像素的下限。
- 全球视图按 Strahler 层级抽稀，放大逐级显露完整权威河网。
- 地图与球面 UI 都可由缩放操作直接验收。

### R1 — P4 校正

- 把积分步长、月份和年循环改成一致的物理时间语义。
- 加入可审计的短波/长波、表面反照率及水汽源汇闭合；以独立观测量校验
  全球能量和水量预算，不以 seed 巧合充当机制证据。
- 校正降水强度、纬向结构和季节位相；没有观测依据的系数先测后钉。
- 以确定性的气候 warm start 满足 Standard 预算，快路径不得更换方程。
- 气候字段和质量摘要必须在 UI 可见。

### R2 — P5 水文 v2

- 修订 R2a（2026-08-24，用户批准）：动态 Aitken 原型未通过真实接口残差和
  未松弛复核，且旧映射每轮抹除 100 ka 历史；先偿还 P4 独立 RK3 comparison
  reference 与 stage identity 证据债，不在有偏真值上继续校准。
- R2b 建立统一亚格元水面几何，让水体积、连续陆海分数、共享湿边和离散海陆
  拓扑来自一个事实源；详细规格见
  `2026-08-24-transient-climate-geomorphology-design.md`。
- R2c 将 P5 改成 P3 只初始化一次、P4 快平衡/P5 慢推进的误差控制前向共演，
  再引入显式 ET、土壤蓄水、地下水基流与雪冰储量及全水圈库存守恒。
- R2d 最后标定分辨率一致的河道起始与 hydraulic geometry；不得在不守恒的
  径流或被重启的地貌历史上校准河网。
- 河道起始使用面积/坡度或输水能力的分辨率一致判据，不再仅靠单一绝对
  流量阈值承担所有尺度。
- 用流量及可辨识环境分组校准 hydraulic geometry；不得用 Strahler 级
  再乘一次视觉增益。
- 同时校验河网密度、级序分布、湖泊连通、水量闭合和计算预算，并接入 UI。

### R3–R5

每项在进入实现前另写冻结规格。R3 先修因果源，不以 T1 噪声掩盖 T0；
R4 先实现级联上下文，再增加支流；R5 只消费稳定的物理产物，不反向改变
生成器。

## 4. 决策与执行纪律

- 机制、公式、阈值、门禁须有同行评审论文、官方技术规范或仓库实测证据。
- 可由事实判定的选择（物理守恒、数值稳定、复杂度、确定性）由实现任务
  直接评估；只有会改变产品语义且证据不能唯一决定的取舍才提交用户裁定。
- 每个里程碑一任务一提交；任何算法只有进入地图/球面 UI 并附用户验收步骤
  才算交付。
- 每次提交前执行 fmt、workspace clippy 与 wasm32 门禁；受影响测试即时跑，
  里程碑收尾使用 `--no-fail-fast` 跑 release 与 debug 全量回归。
- 指纹只刷新由因果链真实改变的项目，并在冻结规格的显式修订中列出。

## 5. 每项承重技术的出处

| 技术/判断 | 出处 | 在本路线中的作用 |
| --- | --- | --- |
| 河线在多边形边界拆分、交汇处成节点、禁止自交/回折、网络无环 | USGS, *Elevation-Derived Hydrography Data Acquisition Specifications: Topology Requirements*，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-data-acquisition-specifications-7> | R0 几何验收契约 |
| 河线留在 DEM 可辨识河槽，水体边界匹配高程表面 | USGS, *Positional Assessment Requirements*，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-acquisition-specifications-1> | R0 地形引导与岸线约束 |
| 河流顶点沿下游高程不升 | USGS, *Elevation-Derived Hydrography: Alignment*，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-data-acquisition-specifications-19> | R0 河床单调门禁 |
| 宽度、深度、速度随流量服从幂律且系数/指数依环境变化 | Leopold & Maddock (1953), USGS Professional Paper 252，<https://pubs.usgs.gov/publication/pp252> | R0 去除重复级序增益；R2 再标定 |
| Priority-Flood 的洼地处理 | Barnes et al. (2014), *Computers & Geosciences* 62, 117–127，<https://doi.org/10.1016/j.cageo.2013.04.024> | P5 现有无环排水底座保留 |
| 河道起始受汇水面积与坡度共同控制 | Montgomery & Dietrich (1988), *Nature* 336, 232–234，<https://doi.org/10.1038/336232a0> | R2 替换单阈值承担全部尺度 |
| 全球 DEM 水文校正需显式处理河道、洼地与流向误差 | Yamazaki et al. (2019), *Water Resources Research* 55，<https://doi.org/10.1029/2019WR024873> | R2/R3 全球尺度质量参照 |
| GPCP v3.1 全球平均降水 2.81 mm/day | Adler et al. (2020), NASA NTRS，<https://ntrs.nasa.gov/citations/20205009415> | §2 判定 P4 明显偏湿 |
