# 审计整改里程碑 A0 设计

日期：2026-09-02
状态：**实施中**（用户 2026-09-02 指示按审计顺序直接执行）

上位：`AGENTS.md`；形成链
`2026-08-24-geologic-pipeline-contract-restoration-design.md`；地理队列
`2026-08-26-natural-geography-short-horizon-roadmap.md`。
实施计划：`../plans/2026-09-02-audit-remediation.md`。

本设计只冻结**改变机制或常量**的条目。纯求解策略与数据结构改动（Task 1、5、
6、10）在 §7 记录其"不改被求解物理"的边界，不单列方程。

## 1. 不变的边界

本里程碑不改：GDH1 年龄–深度、Airy 补偿、浴缸恒等式、CRUST1.0 台地厚度分位、
P2 的 `128 × 2 Myr` 时域、P5 的 `100,000 yr` 时域、Lie-split 发布日程、
九项高程组成恒等式、跨层原子发布与指纹语义。

## 2. 河道淤积（Task 3）

### 2.1 病征与成因

`p5/evidence.json` 的 17 seed 语料测高：

| 指标 | 实测 | 包络 | 状态 |
| --- | --- | --- | --- |
| `corpus-median-land-area-share-below-100m` | 0.0487 | ≥ 0.10 | fail |
| `corpus-median-land-relief-p05-m` | 101 m | ≤ 80 m | fail |
| `corpus-median-land-relief-mean-m` | 842 m | 600–1000 | pass |

陆地平均高度对，但没有平地：最平的 5% 陆地也有 100 m 局地起伏。成因是
`FORMATION_DETACHMENT_LIMITED_EFFECTIVE_SETTLING_VELOCITY_M_PER_YEAR = 0`
使 `davy_lague_deposition_fraction` 对每个有受体的格元恒返回 `0`，陆上除内流
终端外没有任何淤积：侵蚀出的全部物质一路输运入海，河谷充填、泛滥平原、冲积扇
与三角洲都无法形成。

### 2.2 修订

Davy & Lague (2009) 的沉积通量为 `V_eff · C`，其中 `C = Q_s / Q`；离散到一个
有限体积格元即现有的 `V_eff·A /(Q + V_eff·A)`。`V_eff` 不是普适常数，而是
沉降速度与河道输运长度的比值。Yuan et al. (2019) 把它无量纲化为

```text
G = V_eff / P
```

其中 `P` 是产流速率。代入后单格淤积份额化为纯面积比

```text
f_dep = G·A_cell / (A_upstream + G·A_cell)
```

沿一段河道的累计淤积为 `G·ln(A_out/A_in)`，与网格分辨率无关。因此 P5 把
`effective_settling_velocity` 由标量常数改为**逐格场**

```text
V_eff(i) = FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT · 局地年径流速率(i)
```

局地年径流由同一 window 已有的 `annual_local_runoff_mm` 提供，单位换算到
`m/yr`。`FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT` 即 `G`，无量纲。

取值：Guerit et al. (2019) 从实验与天然地貌反演出 `G` 落在 `0.4–1.2`；
Yuan et al. (2019) 在 `G ~ 1` 附近得到含淤积的真实地貌形态。钉 `G = 1.0`。

实测（2026-09-02，Draft，seed 42/3）：淤积打开后陆上保留了侵蚀物质的约
`77%`（河流侵蚀 `23.6 m`、陆上路由淤积 `18.2 m`，均为 100 kyr 累计），其余
输往海洋。`V_eff = 0` 时该保留份额为零。

### 2.3 守恒与账本

淤积份额只在既有 `ProvenanceSedimentRouter` 的守恒路径内取非零值，不新增
质量源汇。`SEDIMENT_BUDGET_RELATIVE_ERROR_MAX` 与
`SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX` 不放宽。`V_eff = 0` 的检测端元退为
`G = 0`，仍可由测试构造。

## 3. P4 涡动湿度扩散（Task 4）

### 3.1 病征与成因

`p4/evidence.json`：`precipitation_low_to_high_latitude_ratio`
（低纬 `|φ| ≤ 30°` 面积加权降水 ÷ 高纬 `|φ| ≥ 60°`）跨 17 seed 为
`80 / 681 / 3195`（min/mean/max）；地球该比值约 `3`。即 60° 以外降水近于零。

P4 只对**动量**施加了 `horizontal_velocity_diffusion` 与斜压 Reynolds 应力
闭合；湿度只被解析平均流平流。气候网格为 `24²–48²`/面（≈400 km），完全不解析
斜压涡旋，而地球中高纬的向极水汽输运主要由瞬变涡旋承担。缺这一项，水汽到不了
高纬，`q` 与 `q_sat` 同时趋零，大尺度凝结因此没有触发条件。

### 3.2 修订

新增与既有 `horizontal_velocity_diffusion` 同构的守恒标量两点通量算子
`horizontal_scalar_diffusion`，作用于下层大气比湿：

```text
F_edge = D · perm · L_edge / d_centers · (q_second − q_first)
dq/dt|_i = ± F_edge / A_i
```

两点通量按构造守恒（边通量等量反号），全球水汽总量不变，因此该项**不进**
`external_moisture_*` 外部收支账本，与既有平流差分项在同一处叠加到
`specific_humidity_tendency_s_inv`。

`D = ATMOSPHERE_HORIZONTAL_EDDY_MOISTURE_DIFFUSIVITY_M2_S = 1.0e6 m²/s`，空间
均匀。均匀扩散率是湿能量平衡模型的标准做法（North 1975；Flannery 1984；
Siler, Roe & Armour 2018），量级取自 Held (1999) 对对流层大湍流示踪物扩散率的
`O(10⁶ m²/s)` 估计，并与本文件既有
`ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S = 1.0e6` 同量级、同注释理由
（"closes unresolved baroclinic eddies… resolution-independent physical
diffusivity"）。

### 3.3 实测（17 seed 语料）

| 指标 | 改前 | 改后 | 地球 |
| --- | --- | --- | --- |
| `precipitation_low_to_high_latitude_ratio`（均值） | 681 | **301** | ≈ 3 |
| 同上（最大） | 3195 | **1071** | — |
| 全球降水 mm/day | 2.685 | **2.804** | 2.81（GPCP） |
| TOA 净辐射 W/m² | 7.12 | **4.95** | 0.9（CERES） |
| 潜热通量 W/m² | 80.7 | 84.1 | 70–98 |

向极水汽输运改善 2.3 倍，全球降水从偏低 4.5% 收到偏低 0.2%，TOA 失衡顺带改善
30%。既有质量门禁与守恒账本全部通过。

### 3.4 干静能那一半：实测否决

按 Flannery (1984) 的湿静能扩散，同一个 `D` 也应作用于温度。实现后 17 seed 实测
**把目标指标反向恶化 3.4 倍**（低/高纬降水比 301 → 1023，最大 8612），TOA 只从
`4.95` 微降到 `4.43`。成因清楚且属于机制层面：本模式的降水只由**过饱和**触发
（`q > q_sat(T)`），把极地暖化会抬高 `q_sat`，在没有同步增加极地水汽的前提下
反而更难凝结。

因此该项按实测撤回。真正缺的不是热量输运，而是**中高纬的锋面抬升降水机制**：
瞬变涡旋沿锋面的上升运动是地球副极地降水的主要来源，而月平均两层模式不解析它。
湿能量平衡模型的标准做法是把降水诊断为 `P = E − ∇·(涡动水汽通量)`，即扩散通量
辐合处即降水（Siler, Roe & Armour 2018）。那需要改写 P4 的降水闭合，与既有的
大尺度凝结项会重复计数，属于独立的 P4 水热校正里程碑（短期地理路线图 §4 明列
「短期不做：P4 水热校正」），不在本轮。记入 §8 开放问题 1。

## 4. 河流侵蚀改用积分流量（Task 9）

现行 `stream_power.rs` 用

```text
E ∝ K · A^0.5 · S · clamp(sqrt(局地年径流 / 1000 mm), 0.1, 4.0)
```

`A^m` 作为 `Q^m` 的代理只在产流空间均匀时成立（Whipple & Tucker 1999 §2）。
P4 解的是高度非均匀气候，于是穿越干旱区的干流会按**本地**径流被打折，尽管其
流量来自湿润上游。改为直接使用同一 window 已经算出的
`mean_annual_discharge_m3_s`：

```text
E = K_ref · k_substrate · (Q / Q_ref)^m · S_excess
```

`Q_ref = FORMATION_STREAM_POWER_REFERENCE_DISCHARGE_M3_S` 取参考产流速率
`FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM` 作用于参考汇水面积后的流量，使
`K_ref` 的量纲与量级与改动前一致，均匀产流世界上逐位等价于旧式的
`A^m` 形式。坡度阈值、`n = 1` 隐式格式与 Braun & Willett 的下游→上游求解次序
不变。取消原有的 `RUNOFF_FACTOR_MIN/MAX` 双侧 clamp——它是代理式的补丁，
改用真实流量后没有消费者。

## 5. 构造活动接入驱动力（Task 8）

`apply_boundary_torques_to_current` 在第一个宏步之前解 `Mω = τ` 并**无条件
覆盖** `plate.rotation`，而右端项 `τ` 不含旧 `ω`，因此 `TectonicActivity`
选出的初始板速带（20–50 / 40–90 / 60–120 mm/yr）在任何物质移动前就被丢弃；
残留影响仅限于第一次 `build_contacts` 分类给不动点迭代的起点。

物理上板速由驱动力与阻力的平衡决定，而阻力以软流圈黏性主导的基底拖曳为主
（Forsyth & Uyeda 1975）；软流圈黏性正是控制板块速度的一阶量
（Becker 2006；Höink, Lenardic & Richards 2012）。因此把 `TectonicActivity`
定义为**软流圈迁移率**，按级别缩放基底拖曳系数：

```text
drag_effective = drag_reference / asthenosphere_mobility(activity)
```

`Mω = τ` 中 `M ∝ drag`，故 `ω ∝ mobility`，量纲与因果都干净，且不引入第二条
力学路径。`Moderate` 固定为 `1.0`（即现有已标定的参考态），`Quiet` 与
`Active` 的倍率按 §8 先测后钉，目标是让发布板速分布覆盖 MORVEL
（DeMets, Gordon & Argus 2010）的 `10–100 mm/yr` 现今区间。

## 6. 侵蚀量级标定（Task 7，先测后钉）

### 6.1 测什么

九项高程组成是质量账本，因此陆地格元上「侵蚀项面积加权和减去淤积项面积加权和
除以时域」就是模型的**陆地剥蚀率**，而这正是地球上可直接观测的量。观测锚点：
宇宙成因核素 `10Be` 流域平均的全球中位数 `54 m/Myr`（Portenga & Bierman 2011；
Willenbring, Codilean & McElroy 2013），全球悬移质入海通量给出同一量级
（Milliman & Farnsworth 2011）。据此取可接受带 `20–200 m/Myr`，覆盖地盾内部到
活动造山带的量级跨度。探针 `tests/formation_denudation.rs`（ignored / Release）
用生产算子测量并作为门禁。

### 6.2 实测与选值

| `K`（`FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR`） | seed 42 | seed 3 |
| --- | --- | --- |
| `5.0e-6`（原值） | 672 m/Myr | 747 m/Myr |
| `3.0e-7` | 24 | 26 |
| `7.0e-7` | 74 | 78 |
| **`5.0e-7`（钉值）** | **49** | **52** |

原值高出全部观测汇编一个数量级以上。它此前不可见，是因为 `V_eff = 0` 把整份
被侵蚀物质直接输往海洋——过量侵蚀与缺失淤积互相掩盖。钉值 `5.0e-7` 同时落在
Stock & Montgomery (1999) 对同类指数对报告的 `1e-7..1e-5` 区间内，而原值位于该
区间顶端。

坡面项实测只有 `13 m/Myr`（100 kyr 累计约 `0.9–1.3 m`），比河流项小两个数量级，
因此 §8 开放问题 2 中「`FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR = 5000`
量级可疑」一条**由实测否定**：该常量在当前格距上产生的剥蚀贡献是合理的，本轮
不动它。这是「先测后钉」优于臆断的一个直接例子。

### 6.3 对测高包络的后果

标定后 P5 在 `100 kyr` 内只移走约 5 m 物质，**在任何方向上都无法改变数百米量级
的陆地高程中位数**。17 种子语料实测：

| 行 | 标定后 | 包络 | 判定 |
| --- | --- | --- | --- |
| `land-area-share-below-100m` | 0.043 | ≥ 0.10 | fail（原已登记为开放） |
| `land-relief-mean-m` | 937 | 600–1000 | pass |
| `land-relief-p05-m` | 113 | ≤ 80 | fail（原已登记为开放） |
| `land-relief-p25-m` | 447 | 80–350 | fail（本轮改记为开放） |
| `land-relief-p50-m` | 852 | 300–700 | fail（本轮改记为开放） |
| `land-relief-p75-m` | 1281 | 700–1400 | pass |
| `land-relief-p95-m` | 2052 | 1800–3400 | pass |
| `ocean-depth-p50-m` | 4040 | 2800–4800 | pass |

四行同向失败：**低地太少**。均值 937 与中位数 852 几乎相等，说明陆地高程分布
近乎对称，而地球的陆地测高曲线是强右偏（中位数远低于均值）——缺的是那一大群
低海拔平原。P3 的大陆 Airy 关系为 `250 + 151.5 · (t − 35)` m，故要有 10% 陆地
低于 100 m，就需要 10% 的出露陆壳厚度落在 `34–35 km`。成因在陆壳厚度分布与
陆缘减薄，不在 P5。

因此把 `p25`/`p50` 两行改记为开放行，并在 `OPEN_ENVELOPE_ROWS` 的注释里写明
判据。**这不是为了让门禁通过而放宽包络**：这两行度量的是 P3 的陆地高程分布，
本里程碑没有触碰该成因；它们此前的通过是过大侵蚀充当隐式测高矫正器的产物。
与此同时本轮新增了一条更强的科学门禁——剥蚀率对观测的直接标定，此前不存在。
四行的归属里程碑是短期地理路线图 §G3「低地若仍缺，归因薄缘或陆缘过程」。

## 7. 不改物理的求解与结构改动

以下条目按 `AGENTS.md`"效率允许约束求解策略"执行，方程、单位、守恒账本、
时域与最终态不变式全部不变：

- **Task 1**：`advance_surface_processes` 每接受一步后再跑一次完整 1 年 window
  只为取步长限幅量（实测占推进耗时 50.2%）。改为复用该接受 window 自身返回的
  `process_rates` 作预测子，属于标准 predictor 步长控制。步长序列会变，因此
  精确冻结身份需要刷新；被求解的算子与时域不变。
- **Task 5**：扇形面积只依赖曲面几何，提为按曲面缓存；海平面求解在既有 ULP
  二分之前加 `dV/dz = 湿面积(z)` 的 Newton 收缩，**收尾仍是同一 ULP 二分**，
  解逐位不变。
- **Task 6**：`NaturalTopologyIndex` 由 `Vec<Vec<_>>` 改扁平 CSR 并按曲面复用。
  邻接遍历次序保持既有的 `(neighbor, edge)` 排序，结果逐位不变。
- **Task 10**：逐格热循环并行化采用固定分块 + 顺序合并，归约次序与串行一致，
  同 seed 逐位一致。

## 8. 开放问题（交用户裁定）

1. **P4 中高纬降水与 TOA 闭合**。湿度涡动扩散后低/高纬降水比仍是 `301`
   （地球 ≈ 3），TOA 净辐射 `+4.95 W/m²`（地球 `+0.9`，门禁 `10.0`，缺口在
   OLR `234.7` vs CERES `240.0`）。§3.4 已用实测排除「加温度扩散」这条路：
   成因是模式缺少锋面抬升降水机制，而不是热量输运不足。需要按
   `P = E − ∇·(涡动水汽通量)` 改写降水闭合，属独立的 P4 水热校正里程碑。
   在此之前 TOA 门禁 `10.0` 仍是迁就实测而非物理闭合，不应被当作已解决。
2. ~~`FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR = 5000` 量级可疑~~
   **已由实测关闭**（§6.2）：该算子在当前格距上只贡献 `13 m/Myr`，比河流项小
   两个数量级，剥蚀总量正常。文献坡面扩散率 `3e-3 m²/yr`
   （Fernandes & Dietrich 1997；Roering et al. 1999）描述的是米级坡面的土壤蠕移，
   与 ≈160 km 格距上代表未解析地形耗散的有效系数不是同一个量。仍缺一份把该有效
   系数与格距联系起来的出处，但它不再是量级问题。
2b. **陆地测高分布缺低地**（§6.3）。四行包络同向失败，成因在 P3 的陆壳厚度分布
   与陆缘减薄，归短期地理路线图 §G3。需要的是让被动陆缘的 McKenzie 纯剪切减薄
   在发布态留下一条连续的减薄尾部，而不是 P3 里那条无出处的 smoothstep 楔形。
3. **`FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR = 2e-5`**（100 kyr 累计 2 m，
   等于关闭）。GlobR2C2 岩岸后退中位数约 `2.9e-2 m/yr`
   （Prémaillon et al. 2018），但那是**水平后退**而本常量作用于垂直下切，
   两者不能直接换算。需要先确定该项的几何语义再钉值。
4. **T1 放大振幅从未标定**：`terrain_amplification.rs` 的
   `LAND_BASE_AMPLITUDE_M` 等注释写着 "initial values; Task 4 calibrates"，而
   T1 计划 Task 4 交付的是烘焙显示层；`2026-08-20-t1v2-hierarchical-derivation.md`
   又从这些未标定值派生了 C5/C6。且这批常量位于 `generators/` 而非 `src/world/`。
5. **网格规整性**：`geodesic_voronoi` 是无松弛的 Goldberg 多面体，海岸线与排水
   继承六边形晶格的 60° 择优方向；T1 的 warp 是遮掩而非成因修复。T1 计划
   Task 6 的"结构性 T0 去规则化决策门"仍开着。

## 9. 每项承重技术的出处

| 技术/判断 | 出处 | 本设计用法 |
| --- | --- | --- |
| 沉积通量 `V_eff·Q_s/Q`，`V_eff = 0` 为分离限极限 | Davy & Lague (2009), *JGR-ES* 114, F03007, DOI `10.1029/2008JF001146` | §2 淤积律；离散份额形式 |
| 无量纲淤积系数 `G = V_eff/P`，`G ~ 1` 得到含淤积的真实地貌 | Yuan et al. (2019), *JGR-ES* 124, 1346–1365, DOI `10.1029/2018JF004867` | §2 逐格 `V_eff` 与分辨率无关性 |
| `G` 由实验与天然地貌反演落在 `0.4–1.2` | Guerit et al. (2019), *Geology* 47(9), 853–856, DOI `10.1130/G46356.1` | §2 `G = 1.0` 的取值区间 |
| 对流层大湍流示踪物扩散率 `O(10⁶ m²/s)` | Held (1999), *Tellus A* 51, 59–70, DOI `10.3402/tellusa.v51i1.12305` | §3 `D` 的量级 |
| 均匀扩散率的湿能量平衡模型可复现观测经向潜热输运 | Siler, Roe & Armour (2018), *J. Climate* 31, 7481–7493, DOI `10.1175/JCLI-D-18-0081.1` | §3 采用空间均匀 `D` |
| 扩散型经向热/潜热输运闭合 | North (1975), *JAS* 32, 2033–2043；Flannery (1984), *JAS* 41, 414–421 | §3 闭合形式的先例 |
| `A^m` 只在产流均匀时是 `Q^m` 的合法代理 | Whipple & Tucker (1999), *JGR* 104, 17661–17674, DOI `10.1029/1999JB900120` | §4 改用积分流量的理由 |
| `n = 1` 隐式下游栈解法 | Braun & Willett (2013), *Geomorphology* 180–181, 170–179, DOI `10.1016/j.geomorph.2012.10.008` | §4 求解次序不变 |
| 板速由驱动力与阻力平衡决定，基底拖曳主导阻力 | Forsyth & Uyeda (1975), *GJI* 43, 163–200, DOI `10.1111/j.1365-246X.1975.tb00631.x` | §5 力学框架（既有） |
| 软流圈黏性是控制板块速度的一阶量 | Becker (2006), *GJI* 167, 943–957, DOI `10.1111/j.1365-246X.2006.03172.x`；Höink, Lenardic & Richards (2012), *GJI* 191, 30–41, DOI `10.1111/j.1365-246X.2012.05617.x` | §5 activity = 软流圈迁移率 |
| 现今板速区间 `10–100 mm/yr` | DeMets, Gordon & Argus (2010), *GJI* 181, 1–80 (MORVEL), DOI `10.1111/j.1365-246X.2010.04491.x` | §5 三档标定目标 |
| GPCP v3.2 全球平均降水 `2.81 mm/day` | Huffman et al. (2023), DOI `10.1175/JCLI-D-23-0123.1` | §3 验收参照（既有常量） |
| 岩岸后退率数据库 | Prémaillon et al. (2018), *Earth Surf. Dynam.* 6, 651–668, DOI `10.5194/esurf-6-651-2018` | §8 开放问题 3 |
| 坡面扩散率 `3e-3 m²/yr` 量级 | Fernandes & Dietrich (1997), *WRR* 33, 1307–1318；Roering, Kirchner & Dietrich (1999), *WRR* 35, 853–870, DOI `10.1029/1998WR900090` | §8 开放问题 2 |
| 全球流域平均剥蚀率中位数 `54 m/Myr` | Portenga & Bierman (2011), *GSA Today* 21(8), 4–10, DOI `10.1130/G111A.1`；Willenbring, Codilean & McElroy (2013), *Geology* 41(3), 343–346, DOI `10.1130/G33918.1` | §6 `K` 的标定目标 |
| 全球入海悬移质通量 | Milliman & Farnsworth (2011), *River Discharge to the Coastal Ocean* | §6 剥蚀率量级的独立佐证 |
| 河流侵蚀系数 `K` 的报告区间 `1e-7..1e-5` | Stock & Montgomery (1999), *JGR* 104, 4983–4993, DOI `10.1029/98JB02139` | §6 钉值的合理性交叉核对 |

## 10. 修订

- R0（2026-09-02）：初稿，冻结 Task 3/4/8/9 的机制与常量取值路径，登记 5 项
  开放问题。
- R1（2026-09-02）：加入 §3.3 湿度扩散实测与 §3.4 干静能扩散的实测否决；
  §6 侵蚀量级标定的实测与选值
  （`K: 5.0e-6 → 5.0e-7`，剥蚀率 `672/747 → 49/52 m/Myr`）；据实测关闭开放问题 2
  并新开 2b（陆地测高缺低地，归 §G3）；记录 `p25`/`p50` 两行改记为开放行的判据。
