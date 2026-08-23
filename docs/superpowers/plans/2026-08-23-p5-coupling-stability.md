# P5 气候—地貌耦合稳定化实施计划

> **For Codex:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 用球面面积加权动态 Aitken 消除 P4↔P5 稳定二周期，只发布经过
未松弛复核的完整物理解，并把耦合诊断接入应用 UI。

**Architecture:** `surface_formation::coupling` 只维护私有高程接口、上一残差
和一个标量系数；`FormationSeaLevelSolver` 为每个接口重建固定水量水线，P4
forcing builder 复用同一高程/海面/陆海生产路径。完整河网、沉积、气候与
artifact 不插值。`FormationCouplingReport` 是 wire 与 UI 的唯一诊断事实源。

**Tech Stack:** Rust 2024、现有 P4/P5 生成器、BLAKE3、serde、egui、cargo。

---

## 执行纪律

- 严格 RED → GREEN → REFACTOR；每一任务独立提交。
- 经验范围只写证据，不钳制、不作为生成拒绝条件；固定点、守恒、拓扑和
  schema 是结构契约。
- 测试必须调用生产 Aitken、海平面、forcing 与摘要助手，不复制公式。
- 每次提交前运行：

```powershell
cargo fmt --all -- --check
$env:CARGO_TARGET_DIR='target/gates'
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --target wasm32-unknown-unknown --all-features --lib
```

- P4/P5 迭代使用 `CARGO_TARGET_DIR=target/probe` 与 `--release`。
- 最终 debug/release 全量都加 `--no-fail-fast`，由 PowerShell
  `Start-Process -WindowStyle Hidden` 分离启动并等待退出码。
- 本会话不推送。

## Task 1：冻结 R2a 规格、因果边界与实施计划

**Files:**

- Create: `docs/superpowers/specs/2026-08-23-p5-coupling-stability-design.md`
- Create: `docs/superpowers/plans/2026-08-23-p5-coupling-stability.md`
- Modify: `docs/superpowers/specs/2026-08-23-natural-world-scientific-remediation-roadmap.md`
- Modify: `docs/superpowers/specs/2026-08-18-coupled-geomorphic-formation-p5-design.md`

### Steps

1. 记录 seed 7 相邻/隔轮残差，明确二周期而非预算不足。
2. 冻结 private elevation interface、面积加权 Aitken、凸组合保护、固定水量
   重求水线和最终未松弛复核。
3. 把 R2 拆成 R2a 耦合、R2b 陆面水量平衡、R2c 河道起始/宽度标定；不提前
   冻结 R2b/R2c 常量。
4. 运行三道门禁并提交。

提交：`Freeze P5 coupling stability design`

## Task 2：实现生产 Aitken、严格报告与私有 P4 地形边界

**Files:**

- Create: `src/generators/natural/surface_formation/coupling.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/generators/natural/global_circulation/forcing.rs`
- Modify: `src/world/natural/surface_formation.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `tests/surface_formation_contracts.rs`

### Step 1：先写 RED

1. 在 production module 测试 `F(x)=-x` 两轮残差得到 `omega=0.5`，接口到达
   固定点；面积不均匀仍使用面积加权公式。
2. 测试首轮 `omega=1` 逐位复制候选、退化/越界系数沿用上一有效值、取消
   及时返回。
3. forcing 测试证明相同高程/水线/陆海的 private interface 与完整
   `FormationTerrainFields` 生成逐位相同 P4 forcing，并拒绝长度、分类和取消
   错误。
4. contracts RED：V2 snapshot、`dynamic-aitken-v1`、严格 bounded 系数向量、
   `final_unrelaxed_verification=true`，篡改均拒绝。

### Step 2：最小实现

- `DynamicAitkenState` 只保留上一 `f64` 残差向量和上一有效系数；第二遍复算
  当前残差后原位更新 `f32` 接口，避免第二份残差缓存。
- `FormationCouplingReport` 保存实际 transition 系数并派生松弛次数、最小和
  最近动态系数。
- `NaturalSurfaceFormationSnapshot` 升 V2；formation model fingerprint 写入
  Aitken、面积权重、保护和复核 identity；dense-owner 增加 13 bytes/cell。
- P4 builder 增加窄的 crate-private elevation/sea/land 入口，完整 terrain
  builder 复用它，不复制 forcing 公式。

### Step 3：GREEN 与回归

```powershell
cargo test --lib surface_formation::coupling::tests -- --nocapture
cargo test --lib global_circulation::forcing::formation_tests -- --nocapture
cargo test --test surface_formation_contracts -- --nocapture
```

### Step 4：门禁与提交

提交：`Add safeguarded P5 Aitken coupling primitives`

## Task 3：接入 P5 外循环并关闭生产二周期

**Files:**

- Modify: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/surface_formation_stage.rs`
- Modify: `tests/surface_formation_generation.rs`
- Modify: `tests/surface_formation_stage.rs`
- Modify: `tests/surface_formation_performance.rs`

### Step 1：先写 RED

1. 报告每个 transition 的实际系数，最终轮必须是未松弛输入；缩短预算时若
   只差复核，错误明确携带 `unrelaxed_verification_pending`。
2. 新增 ignored Release seed 7 产品回归：应用同一路径在 8 轮内成功、至少
   一个系数 `<1`、最终复核为真、重复指纹一致。
3. stage identity RED：`natural.surface-formation@2`；P0–P4 stage set/version
   不变。

### Step 2：接入状态机

- 初始 interface/P4 为 P3；每轮仍从 P3 重跑完整 8 宏步。
- 总是对 raw candidate 生成自己的 P4 与 final hydrology，保留现有五分量
  residual 语义。
- 失败后调用 production Aitken；`omega=1` 复用 candidate P4，`omega<1`
  才重求 interface 水线与 P4。
- 松弛输入后的候选即使 residual 通过也不发布；先强制一轮 `omega=1` 复核。
- 发布构造 `FormationCouplingReport`；失败不产生 report/artifact。

### Step 3：Release GREEN

```powershell
$env:CARGO_TARGET_DIR='target/probe'
cargo test --release --test surface_formation_generation -- --nocapture
$env:SEKAI_P5_SEED='7'
$env:SEKAI_P5_PROFILE='draft'
$env:SEKAI_P5_TRACE='1'
cargo test --release --test surface_formation_stage probe_formation_fixed_point_seed -- --ignored --exact --nocapture
cargo test --release --test surface_formation_stage seed_seven_dynamic_aitken_regression -- --ignored --exact --nocapture
```

### Step 4：门禁与提交

提交：`Stabilize the P5 climate terrain fixed point`

## Task 4：把耦合事实接入形成链 UI

**Files:**

- Modify: `src/ui/field/localization.rs`
- Modify: `src/app/spherical_formation_display.rs`
- Modify: `src/app.rs`
- Modify: `tests/surface_formation_stage.rs`

### Step 1：先写 RED

1. `FormationAreaSummary` 的 `P5CouplingSummary` 与最终 solve report 的轮数、
   松弛次数、最小系数、复核状态和最终残差逐位一致。
2. map/globe 共享 document 时摘要相同；localization 拥有全部新标签。
3. 字段注册表 hash、field payload 与 GPU primitive 不因只读摘要增加而改变。

### Step 2：最小 UI

- 摘要只复制 world report，不在 app 重算 Aitken。
- 左侧 P5 组显示方法、轮数、松弛更新、最小系数、未松弛复核和最终残差。
- 不增加求解器旋钮，不改变地图/球面字段集合。

### Step 3：GREEN、门禁与提交

```powershell
cargo test --test surface_formation_stage -- --nocapture
cargo test --lib app::tests -- --nocapture
```

提交：`Expose P5 coupling convergence in the UI`

## Task 5：证据、身份、全量回归与 R2a 完成记录

**Files:**

- Modify: `tests/surface_formation_evidence.rs`
- Modify: `tests/surface_formation_performance.rs`
- Modify: `tests/surface_formation_atlas.rs`（仅实际像素变化时）
- Modify: `tests/surface_formation_stage.rs`（仅实际指纹变化）
- Modify: `src/app/spherical_natural_display.rs`（仅实际 registry hash 变化）
- Modify: `tests/spherical_presentation_gpu.rs`（仅实际 sampled IDs/golden 变化）
- Modify: `docs/superpowers/specs/2026-08-23-p5-coupling-stability-design.md`
- Create: `docs/superpowers/specs/2026-08-23-p5-coupling-stability-completion.md`
- Modify: `docs/superpowers/plans/2026-08-23-p5-coupling-stability.md`
- Modify: `docs/superpowers/plans/2026-08-23-p4-physical-budget-correction.md`

### Steps

1. 17 粒 Release evidence 增加系数轨迹、松弛次数、P4/P5 solve 次数、最终
   复核、wall clock 与新指纹；经验形态结果只记录不钳制。
2. 跑 seed 42/3/7 确定性、P5 quality/stage/performance/atlas 与受影响 T1/GPU
   套件；仅刷新因 P5 结果真实变化的 identity/golden。
3. 列出 P0–P4 不变以及 P5→T1 实际刷新清单；回补被二周期阻塞的 R1 Task 7
   完成证据。
4. 用 PowerShell 分离运行并等待两档全量：

```powershell
cargo test --workspace --all-targets --all-features --no-fail-fast
cargo test --release --workspace --all-targets --all-features --no-fail-fast
```

5. 最终 fmt/clippy/WASM；完成记录写明自动证据、已知边界和用户 UI 验收步骤。
6. 勾完计划并提交；不推送。

提交：`Record P5 coupling stability evidence`
