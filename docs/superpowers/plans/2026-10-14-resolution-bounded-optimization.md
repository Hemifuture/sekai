# 目标尺度优化：首轮实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans；本会话原生执行，任务用复选框跟踪。

**Goal:** 在 Draft / Standard 验证并落地有最终态证据的低成本近似，交付实际 UI 可用的首轮提速。

**Architecture:** 先复用生产 FAS 和完整生成链，测量粗迭代成本、最终场质量及预算。
将牛顿/拟牛顿类方法作为降低昂贵算子调用次数的候选，不预设其优于简单的有界近似。
不为试验添加生产公共接口；获选路径更新身份并进入原有 UI 发布流程。

**Tech Stack:** Rust、已有 cargo 测试/ignored 探针、现有球面生产算子；不增加依赖。

**Spec:** `../specs/2026-10-14-resolution-bounded-optimization-design.md`

## Global Constraints

- 用户已审阅设计并明确“开始实施”；本次在隔离 worktree 原生执行，不重复索取启动许可。
- 首轮 Draft + Standard；High 不静默改用未经验证的近似。
- 只追求目标尺度最终态正确；不要求复刻旧轨迹或旧模型逐位结果。
- 守恒、有限性、身份、原子发布、取消和下游一致性保持；新预算先测后钉。
- 性能测量串行 Release，无 RUSTFLAGS；复用已有基线，不跑无消费者的大语料。
- 全链优化按成本推进；首轮优先 P4/P5，其他模块先核查消费者和成本，不推测性重写。

## Review Focus

- 近似初猜不应写坏层厚/动量或导致原先能发布的世界失败。
- Draft 与 Standard 不同工作域的结果和性能都需检查；High 支持边界必须明确。
- 取消发生在近似或回退中时应返回 Cancelled，不能被数值拒绝吞掉。
- 新迭代历史只在私有求解中存在，不进入 artifact/UI schema 或持久缓存。
- 旧模型可能已有质量失败；分别报告旧失败、新回归和目标尺度可接受差异。

### Task 1: 生产消费者、成本与近似敏感性

**Files:** `src/generators/natural/formation/causal.rs` 的 ignored 对照模块；
`global_circulation/generation.rs` 的既有 `offline` 捕获；本规格与计划。

**Interfaces:** 消费 `offline::run(OfflineSchedule, solve)`、生产
`generate_capturing_start_correction`、P5 及 `generate_endpoint`。
输出为仓外测量 JSON 与规格中的选型证据；不改变生产路径。

- [x] 在新 worktree 跑最低成本现有 FAS 合约作为基线。
- [x] 增加窄 ignored 探针：同一 seed/profile 与相同上游地形下，对比生产粗迭代预算
  和较低预算；每个候选完整经过起点 P4、P5、终点 P4。
- [x] 复用生产质量评估/账本，分别记录失败、场差、质量与耗时；离线逐点 RMS 只作证据，不新增门槛。
- [x] 检查时间主要用于迭代还是每次初始化；据此选 Task 2 的最小候选并在规格明确修订。
- [x] 文档和测试代码格式/检查通过后，单独提交测量基础。

### Task 2: 最小近似候选与生产选择

**实施修订：** 粗预算对照没有显示足够收益，按上位规格 §3.3/§4B 转测共同物理工作域，
保留原方程和粗预算。拟牛顿未采用，因此不添加无人消费的加速实现或测试。

**Files:** `global_circulation/generation.rs`、`split_explicit.rs`、`tendency.rs`；
`src/world/natural/profile.rs`、`global_circulation_stage.rs` 及直接契约测试。

**Interfaces:** 原快照/发布契约不变；工作尺度在 profile 定义，原有工作区借用跨慢步复用。

- [x] 测原工作域、较小工作域和共同中间域；拒绝新增质量失败的候选。
- [x] 先测后钉共同域，在规格 §10 明确原模型差异、质量结论和适用范围。
- [x] 工作区复用先写最小跨步身份/取消测试，观察 RED 后实现并验证 GREEN。
- [x] 旧工作域全链逐位对照工作区复用，新工作域完整链验证获选近似。
- [x] 删除失败生产候选；High 保持原工作尺度和数值策略。
- [x] 版本测试先 RED 后更新工作域阶段身份；档位、映射、表面测试通过。

### Task 3: 全链核查、门禁与 UI

**Files:** 生产入口 `src/app/spherical_presentation.rs` / `src/app.rs` 只在接入需要时改；
既有 `tests/generation_latency.rs`、相关 P4/P5 套件和规格完成记录。

**Interfaces:** 保持 `run_spherical_world_build` 的最终态原子发布；
性能证据同时报告阶段和实际 UI 发布。

- [x] 核查几何、重采样、校验和分配剩余成本；删除已定位的 UI 重复 P1，GPU 由实际发布测量收尾。
- [x] 串行运行获选候选的同机成对 Release 计时；两档各三对，以中位数报告。
- [x] 删除 UI 丢弃的控制网格/映射重建；P1 身份夹具 RED→GREEN，Draft 真实应用验证通过。
- [x] 跑 fmt、clippy、WASM lib check；共享求解器变更按项目要求扩到 Release 回归，
  不运行无理由的全量 debug。
- [ ] 隔离应用 Draft 生成与字段已验证，已提供用户入口；Standard / 参数修改与取消因
  Unreal 预览遮挡等待前台安排，不能标记为通过。
- [x] 按 executing-plans 要求做一次独立最终审查：gpt-6-astra 审查无 Critical / Important / Minor。
- [x] 提供代码与首轮测量，明确尚待 UI 验收；简化物理模型/拟牛顿路线未实施，剩余瓶颈见规格 §10.5。

## 每项承重技术的出处

- 原模型、FAS、初猜与失败回退：A5/A8b 冻结记录与当前生产实现；
  `GLOBAL_CIRCULATION_MECHANICAL_COARSE_CYCLES` 是当前预算 SSOT。
- 拟牛顿类加速候选：Walker & Ni (2011), *Anderson Acceleration for Fixed-Point
  Iterations*, SIAM J. Numer. Anal. 49, 1715–1735，
  https://users.wpi.edu/~walker/Papers/Walker-Ni%2CSINUM%2CV49%2C1715-1735.pdf ；
  PETSc SNES 的 Newton、quasi-Newton 与 Anderson 工业实现：
  https://petsc.org/main/manual/snes/ 。
- 不直接形成稠密 Jacobian：已有高维生产算子的调用成本及上述固定点加速依据；
  是否最终采用仅由测量决定。
- 守恒/尺度/重映射与完整消费者约束：上位规格 S1–S8；测试按 AGENTS.md 最小充分证据。
