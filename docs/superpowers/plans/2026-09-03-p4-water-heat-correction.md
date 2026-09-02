# P4 水热校正实施计划（2026-09-03，里程碑 A4）

上位：`AGENTS.md`；设计真相
`docs/superpowers/specs/2026-09-03-p4-water-heat-correction-design.md`。
一任务一提交，提交前跑 fmt / clippy / wasm 门禁。测试用
`CARGO_TARGET_DIR=target/probe`（应用锁着 `target/release/sekai.exe`）。

## 任务队列

- [x] Task 0 —— 纬带剖面诊断
      `tests/p4_zonal_profile_probe.rs`（ignored，release）：seed 42 每 5° 带的
      T_air / T_eq / q / RH / P / E / oroP / ASR / OLR / TOA / 季节振幅 / SST / 重建
      初值（ML0、air0）与地球参考。实测见设计 §1：发布 SST = 初值（北极 +10 °C）、
      大气初值 −38 °C、45–55° 冷池 −17 K、降水在 40–50° 倾倒、极区无降水、
      高纬 TOA +13 ~ +28、季节振幅 ≤ 2 K。

- [x] Task 1 —— 储热一致的季节目标、年平均初值、年平均 OLR 线性化
      world 层新增 `seasonal_storage_equilibrium_temperature_c`（线性 EBM 12 月周期解
      的闭式，月均恒等于年目标）、`gray_longwave_slope_w_m2_k`、
      `p4_seasonal_storage_heat_capacities_j_m2_k`（生产剖面 C2 布局：大气 7.38 × 10⁶、
      混合层 4.09 × 10⁸ J/m²/K）。`forcing.rs` 逐月目标改为周期解（混合层 C_ml；大气
      C_air + (1 − land) C_ml），指纹 v5；`state.rs` 年平均初值先平均再钳制；
      `tendency.rs` 辐射步围绕年平均 `A + B T` 线性化，方程指纹 v12。
      测试：闭式（常强迫、单谐波振幅 / 滞后、极夜有限、均值恒等）与夹具（陆地目标
      摆幅 > 20 K、海面 < 6 K、均值 = 年目标）。
      seed 42：北极 SST +10 → −1.9 °C，45–90° N TOA +13 → 0，47.5° N 冷池 −17 → −6 K，
      全球 TOA +7.0 → −3.8；南极暴露平流 – 辐射瞬变（设计 §3.5）。

- [~] Task 2 / 2b —— 海冰面先验、扩散 EBM 年平均态（**实验分支，不进 main**）
      分支 `a4-task2-sea-ice-experiment`（ba5ebdf）：`sea_ice_fraction`、
      `P4_SEA_ICE_SURFACE_ALBEDO`、气–混合层交换 × 0.076、共轭梯度扩散 EBM、
      `annual_initial_temperature_c` 初值，含测试。实测（设计 §4.1–4.2）：海冰单独
      → 45–60° N 冷带 −28 K、夹具 TOA −12.7 破门；扩散 EBM → 无冰、热带 18 °C、
      降水 1.92。结论（设计 §4.3）：灰体辐射先验无纬度结构、已隐含地球的输送，
      显式输送重复计数；须先做"P4 辐射先验"再回到海冰与时间结构，交用户裁定。

- [x] Task 3 —— 语料证据、时延门、裁定备忘
      Task 1 态（设计 §5.1）：17 seed P 2.66 / E 2.77 / TOA −5.0，雨影 5/17、增湿
      6/17，季节相位 17/17（Task 1 的直接后果，见 §5.1）；32 seed 冷启动 Draft 32/32、
      Standard 32/32；全量 Release 回归 151 结果全绿；时延 Draft 20.1 s、Standard
      64.7 s（应用关闭后测，与 A3 持平）。裁定顺序（设计 §6）：辐射先验 → 时间
      结构 A → 海冰 / 含输送初值。

## 用户验证步骤

字段目录选**年均气温（环流）**：极区不再是一整块 −18 °C 的平台，南北极向平衡
值靠拢；45–55° 的冷带消失。选**海表温度**：极区海面为 −2 °C 而非 +10 °C。
选**年降水量（环流）**：40–50° 的倾倒带减弱、极区出现微量降水。季节循环的
真实恢复要等设计 §6 的时间结构裁定。
