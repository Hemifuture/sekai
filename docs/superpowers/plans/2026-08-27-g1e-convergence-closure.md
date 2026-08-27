# G1e 汇聚的物理闭合与张开相开局 实现计划

> **给代理：** 规格是 `docs/superpowers/specs/2026-08-27-g1e-convergence-closure-design.md`。
> 一任务一提交；系数先测后钉；未进 UI 不得称交付。

**目标：** 删掉按预设禁止过程的张开相门闩；板间汇聚要么俯冲要么被锁定阻力
刹住，不再由重采样静默吞掉；Continents / Archipelago 从拼合态开局跑分散半程。

**架构：** 全部留在 `foundation/tectonics`（contacts / torques / initial_state /
model / resample）与 `src/world/natural/spherical_tectonics.rs` 常量。

---

### 任务 0：规格与计划

- [x] 写规格与计划
- [ ] 提交

### 任务 1：删除张开相标签

**文件：** `model.rs`、`initial_state.rs`、`contacts.rs`、`tests/g1d_endstate_crust.rs`

- [ ] 删 `opening_phase_lineages` 及全部访问器、`mark_opening_phase_lineages`、
      四个相关测试
- [ ] `cargo test` 目标模块；fmt；clippy
- [ ] 提交

### 任务 2：板间汇聚闭合分类 + 锁定阻力 + 搬运入账

**文件：** `contacts.rs`、`torques.rs`、`forcing.rs`、`resample.rs`、`model.rs`
（ledger）、`src/world/natural/spherical_tectonics.rs`

- [ ] `ContactKind::LockedConvergence`；`classify_pair` 按 §3.2；删
      `colliding_continents` / `InitiationView.colliding`
- [ ] torques：锁定阻尼；常量占位注明待任务 4 钉值
- [ ] forcing / runner 穷举匹配更新；锁定不产生强迫
- [ ] ledger 记录重采样搬运面积
- [ ] 纯函数测试；提交

### 任务 3：半球帽开局

**文件：** `initial_state.rs`

- [ ] Continents / Archipelago：帽中心 + 帽内最远点选核，允许同板多核
- [ ] 开局测试：核在帽内、帽外板全洋、Archipelago 有同板双核、面积/嵌套不变
- [ ] 提交

### 任务 4：探针实测与钉系数

**文件：** `tests/g1d_endstate_crust.rs`（ignored 探针）、常量

- [ ] 板速分布、锁定/碰撞残余汇聚、搬运份额、内湖数
- [ ] 钉 `PLATE_LOCKED_MARGIN_RESISTANCE_PER_M`、复核其余 `PLATE_*`
- [ ] 提交

### 任务 5：终态窄集成与身份

- [ ] 抬 `natural.spherical-tectonics` 与 `natural.causal-formation` version
- [ ] 窄集成按规格 §5；冻结哈希/身份矩阵更新
- [ ] 提交

### 任务 6：回归与 UI 验证步骤

- [ ] fmt / clippy / wasm check；受影响套件 Release；完整调试回归
- [ ] 规格 §6 步骤交用户

---

## 每项承重技术的出处

见规格 §8。本计划不新增无出处杠杆。
