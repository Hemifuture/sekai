# 生成时延里程碑 A7 设计：C2 大气重力波的时间分裂小步

日期：2026-10-06（实测 2026-10-07）
状态：**实施完成，待用户 UI 验收**（用户 2026-10-06：“继续往前做，继续完善功能和地形
合理程度以及速度”，据此执行 A6 §8.1 的推荐；本文件是对 A1 §2 冻结“分裂结构”的显式
修订）。偏离按 §9 修订条目记录。

上位：`AGENTS.md`；A1 `2026-09-02-generation-latency-design.md`（目标、§2 边界）；
A6 `2026-09-29-a6-generation-latency-recovery-design.md`（§1.1 剖面、§8.1 成因与推荐）；
P4 `2026-08-17-global-atmosphere-ocean-p4-design.md` §6.3（逐阶段重算约束）；
A5 `2026-09-03-p4-water-cycle-and-tropics-design.md` §7.18（共同自由面压力）。

## 1. 问题

A6 §8.1 实测：A5 §7.18 的两层共同自由面压力带来约 313 m/s 的外模，Draft 细网格
每宏步的快子步从 Task 2b 的约 3 个升到 12 个；快张量每个 RK 阶段重算全部动量
物理（P4 §6.3 要求扩散与配对动量交换逐阶段重算），占全链 57 %。

限制步长的只是**线性的高度梯度—散度耦合**（重力波），而每个快子步却重算扩散、
动量输运、海洋快热力学、层间交换等全部昂贵项。

## 2. 机制：Wicker–Skamarock 时间分裂 RK3 + Störmer–Verlet 小步

采用可压缩/浅水模式的标准时间分裂（Wicker & Skamarock 2002；Skamarock et al.
2008 WRF 技术说明 §3；Klemp, Skamarock & Dudhia 2007）：

- **大步**沿用现有快张量 `F` 加冻结慢张量 `S`，以 Wicker–Skamarock RK3 推进，
  三个阶段长度 `Δt/3, Δt/2, Δt`，每阶段都从步首状态 `φⁿ` 出发。
  `R = F + S` 在每个阶段完整重算，P4 §6.3 的“扩散与配对动量交换逐 RK 阶段重算”
  原样成立。
- **小步**只承担 C2 两层大气的重力波算子 `L`。在阶段 `s`，以不含 `L` 的快张量
  得到余项 `R'_s`，冻结 `L` 的系数；从 `φⁿ` 起以 Störmer–Verlet（kick–drift–kick，
  Hairer, Lubich & Wanner 2003）走 `n_s` 个等长小步 `δτ = Δt_s / n_s`：
  半步 `u += δτ/2 (R'_u + L_u(h))`；整步 `h += δτ (R'_h + L_h(u))`；
  半步 `u += δτ/2 (R'_u + L_u(h))`。收尾半步的梯度即下一小步开头的梯度，每小步
  仍只算一次两层高度梯度与一次两层通量散度。
  非大气变量（海洋层、所有标量）只走大步：`φ_{s+1} = φⁿ + Δt_s R(φ_s)`。

### 2.1 小步算子 `L`（与快张量共用事实源）

`L` 恰为快张量中对大气层高度异常 `h_L、h_U` 线性、对速度线性的项，系数冻结在阶段
状态 `φ_s`：

1. 动量：A5 §7.18 的压力矩阵 `G`。逐项展开后，上下层的 `∇h_U` 系数与上层的 `∇h_L`
   系数都是 `−S`，下层 `∇h_L` 系数为 `−S − R`，其中 `S = g − b_U`（共同自由面
   有效重力）、`R = g'_L + g'_U − δb`（界面有效重力）。这把各层自身
   reduced-gravity 项、`apply_common_surface_pressure` 与热压中乘高度梯度的两项
   合为一处；热压中乘温度梯度、乘层厚的项留在 `R'`。`b_U、δb` 与越域检查由同一个
   `atmospheric_column_buoyancies` 给出，完整快张量与算子共用。
2. 连续：`apply_horizontal_momentum_transport` 的通量式连续方程，面深度
   （`reconstruct_layer_faces` 的 `face_depth`）冻结在 `φ_s`；面体积通量由共用的
   `edge_volume_flux_m2_s` 计算，对速度线性。

单元测试 `gravity_wave_operator_restores_the_full_fast_tensor` 断言
“去掉 `L` 的快张量 + `L(φ_s)`”逐项还原完整快张量（相对 `1e-6`）。

### 2.2 守恒与正性

- 连续小步为通量式、面深度冻结，单层柱质量在每个小步精确守恒（至 f32 舍入），
  与大步同一守恒解释（`SharedTendencyExtensiveV1`）。
- 大步余项含的高度源（翻转交换、外部源汇）不变。
- 正厚度由既有 `validate_against` 与 `fluid_layer_thickness_m` 检查拒绝；不加夹值。

## 3. 步长规划

- **大步**：沿用 `fast_substep_plan`，但 C2 大气的波速项回到
  `GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S`（P4 积分器选择冻结的第一斜压模上界，
  Task 2b 的规划口径）加实测最大流速（`estimate_reference_wave_cfl`）；Coriolis、
  动量交换率约束与 CFL 目标 `GLOBAL_CIRCULATION_FAST_CFL_TARGET` 不变。
- **小步**：每个大步以既有快模波速上界（含 313 m/s 外模，`estimate_cfl`）乘阶段
  长度，按 `GLOBAL_CIRCULATION_GRAVITY_WAVE_SMALL_STEP_CFL_TARGET` 取每阶段小步数
  （向上取整、至少 1）。该常量由精度而非稳定性决定，取值过程见 §7.2。
- 端点重规划（标量端点后取更严者）保留；小步的快模上界取自标量端点后的状态，
  标量冷却加快的压力模由小步承担；诊断 `maximum_cfl` 报大步与小步 CFL 的
  较大者，两者都不超过各自目标。

## 4. 不变的边界

- 方程、空间离散、宏步、慢步上限、强迫相位、成形残差目标、硬闭合门、P5 物理与
  发布语义均不变；慢/快 Lie 分裂（标量端点 + 冻结慢张量）不变。
- 本修订**改变数值**：C2 快层时间积分从经典 Kutta RK3 子步改为 WS RK3 大步 +
  Verlet 小步。按 AGENTS“产品边界”，这是求解策略近似，以更高成本的经典路径为参考
  离线对照（§7）。
- 模型指纹 `global_circulation_model_fingerprint` 的 C2 分支登记语义 ID
  `gravity-wave-time-split-ws-rk3-stormer-verlet.v1` 与小步 CFL 目标；审计平台金样
  按实测重钉，科学界限不放宽。
- C1 单层档位、`advance_closed_no_source` 闭合固定装置、显式 RK3 / IMEX 参考积分器
  不改。

## 5. 非目标

FAS 周期数、粗网格、CFL 目标、宏步、时间压缩比；发散阻尼或离中。

## 6. 被否决的变体（实测）

- **Coriolis 一并进小步**（仿海洋正压子循环）：Draft seed 42 在小步 CFL 0.8 时
  P4 成形残差 0.43 → 2.56 发散，0.1 时风场相对误差 67 %。显式 Coriolis 与前向—
  后向格式组合本身不稳定（Mesinger & Arakawa 1976），撤回。
- **一阶前向—后向小步**（Mesinger 1977）：误差随小步步长一阶下降，CFL 0.8 时降水
  误差 11.7 %、0.025 时仍 4.8 %，成本远高于同精度的 Verlet。

## 7. 验证（最小充分证据）

### 7.1 单元

`L + R'` 与 `R` 一致（§2.1）。

### 7.2 参考对照（Draft seed 42，Windows Release，2026-10-07）

参考路径：经典分裂路径、`GLOBAL_CIRCULATION_FAST_CFL_TARGET` 临时改为 0.2
（四倍子步，仅离线构建，不进产品）。经典路径自身收敛干净：CFL 0.4 对参考差 0.74 %
（近地面风），生产 0.8 差 3.4 %。下表为终点 P4 发布场相对参考的面积不加权 RMS 误差
（除以参考 RMS）：

| 场 | 生产（经典 0.8） | A7 小步 CFL 0.8 | **A7 小步 CFL 0.2** |
| --- | ---: | ---: | ---: |
| 近地面风 | 3.4 % | 7.4 % | **5.4 %** |
| 高层风 | 1.7 % | 3.5 % | **2.7 %** |
| 表层海流 | 1.9 % | 3.3 % | **3.0 %** |
| 降水 | 5.0 % | 9.5 % | **4.0 %** |
| 地形降水 | 2.1 % | 4.4 % | **3.5 %** |
| 蒸发 | 2.1 % | 3.3 % | **2.5 %** |
| 比湿 | 0.28 % | 0.29 % | **0.19 %** |
| 气温 | 0.08 % | 0.11 % | **0.09 %** |
| 下/上层高度异常 | 1.1 / 1.0 % | 2.1 / 1.7 % | **1.3 / 1.1 %** |

大步也压到 600 s 时 A7 误差回到生产水平（风 4.1 %、降水 3.1 %），说明分裂实现
一致；风场与海流剩余的额外误差来自 WS 大步（约 1800 s）的二阶截断。降水、湿度、
温度、高度场达到或优于生产精度，风场与海流误差约为生产的 1.6 倍。

### 7.3 时延（同机成对，阶段探针 `stage_timing_probe`，Draft seed 42）

| 路径 | 全链 | 每次 P4 求解快步数 | 起点/终点终残差 |
| --- | ---: | ---: | --- |
| 生产（main a716138） | 32.8 s | 720 | 0.2140 / 0.2138 |
| A7 小步 CFL 0.8 | 21.6 s | 239 | 0.2159 / 0.2160 |
| A7 小步 CFL 0.2 | 24.4 s | 239 | 0.2231 / 0.2232 |

成形循环数均为 5，终残差门 0.24。生产路径阶段拆分：P4 起点 13.9 s、P5 + P4 终点
15.6 s；A7 小步 CFL 0.8 时为 8.2 s、10.1 s。

### 7.4 语料与收尾

- `tests/formation_seed_sweep.rs`（2026-10-07，完整图含展示外部产物）：Draft
  8 seed × 3 档构造活动 24/24 通过，每世界 26.2–32.1 s；Standard 8/8 通过，
  65.2–69.4 s。
- Release 全量回归（2026-10-07，Windows）：1313 通过，两处按设计变化——
  - 层级探针指纹（审计平台金样）刷新为 `992518cf…`，L0 恒等与陆比漂移 0.0139
    不变（T1 v2 规格修订 A12）；
  - 单元测试 `scalar_cooling_replans_the_thermal_fast_step` 断言经典路径的大步按
    冷端点重规划。A7 大步按参考波速规划，冷端点的压力模改由小步承担，故改写为
    `scalar_cooling_sizes_the_gravity_wave_small_steps`：入口暖态每阶段需 1 个小步、
    冷端点需 2 个，降温运行报告的小步 CFL 须等于冷起点、不同于暖起点；把小步规划
    改用入口态的变异实测会使其失败。
- fmt/clippy/wasm lib check、CI；用户在 UI 上验收生成耗时与气候场外观。

## 8. 开放问题

- Draft 仍未达 A1 的 10–20 s 目标；剩余大头是 FAS 初猜粗周期与大步本身。
- 风场误差若需回到生产水平，可评估三阶多速率方法（MIS，Wensch, Knoth & Galant
  2009；Knoth & Wensch 2014）替代 WS 大步，属另一里程碑。

## 9. 修订日志

- **R1（2026-10-07）实测改小步格式。** 草案的前向—后向小步在 CFL 0.8 下误差为
  生产的 3 倍，改为 Störmer–Verlet，并新增小步 CFL 常量（§3、§6、§7.2）。

## 10. 每项承重技术的出处

| 技术 | 出处 | 用法 |
| --- | --- | --- |
| 时间分裂 RK3（阶段 Δt/3、Δt/2、Δt，小步从步首出发） | Wicker, L. J. & Skamarock, W. C. (2002), *Mon. Wea. Rev.* 130, 2088–2097 | §2 大步结构 |
| 工业实现（大步内重力波小步、阶段步数） | Skamarock, W. C. et al. (2008), NCAR/TN-475+STR §3；Klemp, Skamarock & Dudhia (2007), *Mon. Wea. Rev.* 135, 2897–2913 | §2、§3 |
| Störmer–Verlet（二阶、辛、首尾复用力） | Hairer, E., Lubich, C. & Wanner, G. (2003), *Acta Numerica* 12, 399–450 | §2 小步 |
| 前向—后向格式（被否决的一阶变体） | Mesinger, F. (1977), *Contrib. Atmos. Phys.* 50, 200–210 | §6 |
| 显式 Coriolis 与前后向格式的稳定性 | Mesinger, F. & Arakawa, A. (1976), GARP Publ. Ser. 17 | §6 |
| 分层自由面模式的模分裂 | Higdon, R. L. (2005), *J. Comput. Phys.* 206, 463–504；Shchepetkin & McWilliams (2005), *Ocean Modelling* 9, 347–404 | 背景，A6 §8.1 |
| 多速率无穷小步方法（开放问题） | Wensch, J., Knoth, O. & Galant, A. (2009), *BIT* 49, 449–473；Knoth, O. & Wensch, J. (2014), *Mon. Wea. Rev.* 142, 2067–2081 | §8 |
| 外模来源 | Salmon (2002)，A5 §7.18 | §1 |
| 逐阶段重算约束 | P4 设计 §6.3 | §2 |
