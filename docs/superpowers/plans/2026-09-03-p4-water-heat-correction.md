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

## 用户裁定后的追加队列（2026-09-03，设计 §6.1–6.3）

用户裁定：本产品是地图编辑器，挑效率高、效果接近的做，近似即可。先验负责纬向
温度结构，动力学负责非纬向偏差。选项 A 与辐射先验重标定取消。

- [x] Task 4 —— 热力学时间压缩（Bryan 1984 失真物理）
      `GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION = 30`：外部辐射与全部层间
      配对热交换共用压缩后的有效热容，并各按闭式隐式因子 `1/(1+Δt·k)` 阻尼
      （配对项的 `k` 由两侧倾向反读）。动量、水汽、输运不变。方程指纹 v12 → v13。
      压缩比上限是实测的，不是选的：365 会撞破未压缩的水循环（径流 5262 mm）
      与共享 f32 倾向数组的配对相对门（5.23e-6 对 1e-6），设计 §6.2.1 有三点扫描。
      收益（seed 42）：降水 2.43 → 2.85（GPCP 2.81），dT 1.15 → 0.38，
      TOA −3.79 → −1.64，南极 Tjul−Tjan −0.3 → −11.0，季节符号正确。
      17 seed：降水 2.66 → 3.01、TOA −5.0 → −1.58、季节相位失败 17/17 → 9/17、
      增湿比 6/17 → 2/17；全量回归 152 全绿；**Standard 时延 64.7 → 56.1 s 首次
      达标**（收敛变好，终点 P4 6 轮 → 5 轮），Draft 20.1 → 18.4 s。
      要盯：降水由低于 GPCP 5 % 变为高出 7.1 %，压在包络上限，Task 5 重分配
      赤道雨带时须连总量一起看。

- [ ] Task 5 —— 副热带下沉干燥与赤道雨带
      用既有 `diagnose_axisymmetric_circulation` 的垂直速度调制凝结：下沉干燥、
      上升增湿。修 40–50° 堆积与热带降水缺口。

- [ ] Task 6 —— 海冰诊断层
      回收分支 `a4-task2-sea-ice-experiment`（ba5ebdf）的海冰先验，Task 4 之后
      复测 TOA 门。

- [ ] Task 7 —— 洋流陆地泄漏
      `WorkClimatology::project` 的既有缺陷，`ocean-current-land-leakage-max-m-s`
      17/17 失败。

## 用户验证步骤

字段目录选**年均气温（环流）**：极区不再是一整块 −18 °C 的平台，南北极向平衡
值靠拢；45–55° 的冷带消失。选**海表温度**：极区海面为 −2 °C 而非 +10 °C。
选**年降水量（环流）**：40–50° 的倾倒带减弱、极区出现微量降水。季节循环的
真实恢复要等设计 §6 的时间结构裁定。
