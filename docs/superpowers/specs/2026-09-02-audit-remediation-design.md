# 审计整改里程碑 A0 设计

日期：2026-09-02
状态：**实施中**（用户 2026-09-02 指示按审计顺序直接执行）

上位：`AGENTS.md`；形成链
`2026-08-24-geologic-pipeline-contract-restoration-design.md`；地理队列
`2026-08-26-natural-geography-short-horizon-roadmap.md`。
实施计划：`../plans/2026-09-02-audit-remediation.md`。

本设计只冻结**改变机制或常量**的条目。纯求解策略与数据结构改动（Task 1、5、
6、10）在 §6 记录其"不改被求解物理"的边界，不单列方程。

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
Yuan et al. (2019) 在 `G ~ 1` 附近得到含淤积的真实地貌形态。本里程碑先钉
`G = 1.0`，再按 §7 用生产算子实测两条失败包络确认。

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

### 3.3 本轮不做

温度的涡动扩散不在本轮：极区降水的直接成因是水汽输运缺失，不是 `q_sat`；而
温度倾向参与 `GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX` 账本，需要单独的
守恒证据。TOA 净辐射偏差（实测均值 `+7.12 W/m²`，地球 `+0.9`，门禁 `10`）
记为 §7 开放问题，本轮只观察湿度扩散对它的影响，不放宽门禁。

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
`Active` 的倍率按 §7 先测后钉，目标是让发布板速分布覆盖 MORVEL
（DeMets, Gordon & Argus 2010）的 `10–100 mm/yr` 现今区间。

## 6. 不改物理的求解与结构改动

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

## 7. 开放问题（交用户裁定）

1. **P4 TOA 净辐射 `+7.12 W/m²`**（地球 `+0.9`，门禁 `10.0`）。缺口在 OLR：
   `234.1` vs CERES `240.0`。门禁当前被设在实测值上沿，属于迁就而非闭合。修法
   需要改灰体长波方案或加温度涡动输运，超出本里程碑范围。
2. **`FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR = 5000`**。文献坡面扩散率为
   `3e-3 m²/yr` 量级（Fernandes & Dietrich 1997；Roering et al. 1999），相差
   5–6 个数量级。在 ≈160 km 格距上该算子实际代表的是未解析的河谷切割网络而非
   土壤蠕移，没有直接对口出处。Task 3 打开淤积后先实测其对测高的贡献，再决定
   重钉、改名归因还是删除。
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

## 8. 每项承重技术的出处

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
| 岩岸后退率数据库 | Prémaillon et al. (2018), *Earth Surf. Dynam.* 6, 651–668, DOI `10.5194/esurf-6-651-2018` | §7 开放问题 3 |
| 坡面扩散率 `3e-3 m²/yr` 量级 | Fernandes & Dietrich (1997), *WRR* 33, 1307–1318；Roering, Kirchner & Dietrich (1999), *WRR* 35, 853–870, DOI `10.1029/1998WR900090` | §7 开放问题 2 |

## 9. 修订

- R0（2026-09-02）：初稿，冻结 Task 3/4/8/9 的机制与常量取值路径，登记 5 项
  开放问题。
