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

- [ ] Task 1 —— 储热一致的季节目标、年平均初值、年平均 OLR 线性化
      `forcing.rs`：每格 ASR_ann、T_eq,ann（面 / 气，含直减）、B_ann；逐月目标改为
      线性 EBM 周期解（混合层 C_ml；大气 C_air + (1 − land) C_ml）；指纹 v5。
      `state.rs`：年平均初值取 T_eq,ann。`tendency.rs`：`apply_external_radiation`
      围绕年平均线性化；方程指纹 v12。测试：周期解对单谐波振幅 / 滞后的闭式、
      极夜有限、初值 = 年均目标、OLR 在 T = T_eq,ann 处等于 ASR_ann。
      证据：seed 42 纬带表前后对照。

- [ ] Task 2 —— 海冰面先验
      `forcing.rs`：`ice` 诊断（无冰 T_eq,ann,surface < −2 °C）、
      `P4_SEA_ICE_SURFACE_ALBEDO`、辐射权重 / 蒸发可用度 `(1 − land)(1 − ice)`、
      气–海交换 `water_scale` 乘 1/13；指纹再升。度量清单不变。
      证据：seed 42 极区 RH / 降水 / TOA；17 seed 硬门。

- [ ] Task 3 —— 语料证据、时延门、时间结构裁定备忘
      17 seed（P / TOA / 雨影 / 增湿 / 非纬向 / 季节相位）、32 seed 冷启动、全量
      Release 回归、Draft / Standard 时延；设计 §6 补 Task 1–2 之后的纬带表，
      作为选项 A / B / C 的裁定输入。

## 用户验证步骤

字段目录选**年均气温（环流）**：极区不再是一整块 −18 °C 的平台，南北极向平衡
值靠拢；45–55° 的冷带消失。选**海表温度**：极区海面为 −2 °C 而非 +10 °C。
选**年降水量（环流）**：40–50° 的倾倒带减弱、极区出现微量降水。季节循环的
真实恢复要等设计 §6 的时间结构裁定。
