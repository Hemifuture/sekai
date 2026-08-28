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
- [x] 提交

### 任务 1：删除张开相标签

**文件：** `model.rs`、`initial_state.rs`、`contacts.rs`、`tests/g1d_endstate_crust.rs`

- [x] 删 `opening_phase_lineages` 及全部访问器、`mark_opening_phase_lineages`、
      四个相关测试
- [x] `cargo test` 目标模块；fmt；clippy
- [x] 提交

### 任务 2：板间汇聚闭合分类 + 锁定阻力 + 搬运入账

**文件：** `contacts.rs`、`torques.rs`、`forcing.rs`、`resample.rs`、`model.rs`
（ledger）、`src/world/natural/spherical_tectonics.rs`

- [x] `ContactKind::LockedConvergence`；`classify_pair` 按 §3.2；删
      `colliding_continents` / `InitiationView.colliding`
- [x] torques：锁定阻尼（后改为全板耦合求解，规格 R1.3）
- [x] forcing / runner 穷举匹配更新；锁定不产生强迫
- [x] ledger 记录重采样搬运面积；重采样改为赢家掩膜 + 按板重平衡（R1.1）
- [x] 纯函数测试；提交

### 任务 3：半球帽开局

**文件：** `initial_state.rs`

- [x] Continents：帽中心 + 帽内最远点选核，帽内板合成一块超大陆板
- [x] Archipelago 改回板块代表分散开局（规格 R1.4）
- [x] 开局测试：核在帽内、帽外板全洋、面积/嵌套不变
- [x] 提交

### 任务 4：探针实测与钉系数

**文件：** `tests/g1d_endstate_crust.rs`（ignored 探针）、常量

- [x] 板速分布、锁定/碰撞残余汇聚、搬运份额、内湖数（`probe_g1e_*`）
- [x] 钉 `PLATE_LOCKED_MARGIN_RESISTANCE_PER_M`、复核其余 `PLATE_*`（规格 R1.5）
- [x] 提交

### 任务 5：终态窄集成与身份

- [x] 抬 `natural.spherical-tectonics` 7→8 与 `natural.causal-formation` 4→5
- [x] 窄集成按规格 §5（主要块计数）；冻结哈希/身份矩阵更新
- [x] 提交

### 任务 6：回归与 UI 验证步骤

- [x] fmt / clippy / wasm check；Release 全量 142 套件通过（仅
      `persisted_origin_defaults_new_apps_and_missing_tags_to_spherical` 为
      7323cba 起的既有失败）；调试档全量按 2026-08-28 用户指令不再硬性要求
- [ ] 规格 §6 步骤交用户（UI 验收待用户）

### 任务 7：陆地碎裂根因（规格 R2）

- [x] 采样洞改标记样本（强度量平流），删劈柱 / 链搜索
- [x] 重采样起步厚度取覆盖赢家；粗糙度探针入 `probe_g1e_*`
- [x] 刚体诊断 1.55 km、P3 渲染各预设可分辨；Release 全量 + 身份更新
- [x] 身份 8→9、冻结哈希 / 金样更新、因果门禁改锁符号；提交
- [ ] UI 验收待用户

---

## 每项承重技术的出处

见规格 §8。本计划不新增无出处杠杆。
