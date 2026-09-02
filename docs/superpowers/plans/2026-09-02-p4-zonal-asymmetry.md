# P4 近地面风场纬向不对称实施计划（2026-09-02，里程碑 A2）

上位：`AGENTS.md`；设计真相
`docs/superpowers/specs/2026-09-02-p4-zonal-asymmetry-design.md`。
一任务一提交，提交前跑 fmt / clippy / wasm 门禁。

## 任务队列

- [ ] Task 0 —— 度量 `near-surface-wind-non-zonal-variance-fraction`
      年平均近地面风偏离 5° 纬带面积加权平均的方差占比；只记录不设门。新增
      `tests/wind_asymmetry_probe.rs`（ignored / Release）打印 Draft seed 42 的该
      度量与纬带剖面，作为每个任务的前后对照。

- [ ] Task 1 —— 低层大气连续方程带地形
      `z_b = land_fraction · max(elevation − sea_level, 0)`，钳制 `H_ref − z_b ≥ H_ref/6`；
      快、慢两条连续路径的施主厚度改为 `H_ref − z_b + η`。方程指纹 v9 → v10。
      验证：Task 0 探针前后；P4 套件。

- [ ] Task 2 —— 陆海 Rayleigh 摩擦对比
      `r_lower = r_sea · (1 + (ρ − 1) · land_fraction)`，`ρ = 3`；记录 ρ = 2 / 3 / 4
      扫描。验证同上。

- [ ] Task 3 —— 语料验证与收尾
      17 seed 气候证据（既有门全过、新度量分布）、32 seed 冷启动扫描、产品级时延
      门、全量 Release 回归；更新 P4 设计文档的方程节。

## 用户验证步骤

`cargo run --release`，字段目录选**盛行风（环流）**：陆地上风应明显偏转与减弱，
山脉两侧不再是一条直线；南北半球不再镜像。切换「新种子并重建」看几个 seed。
