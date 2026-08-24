# 地质管线契约恢复与因果当前态 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 恢复 P2→P3→P4→P5 的单一过程所有权，以私有有限物理时间演化生成一个原子当前态 bundle，消除兼容高程污染、绝对地貌稳态误用和跨步 `f32` 状态回流，同时保持现有 UI 字段能力。

**Architecture:** `world` 提供唯一 resolved formation timeline、借用型权威构造视图、当前态 bundle schema 与数值/守恒契约；`generators/natural` 内的领域协调器持有 P2/P3/P4/P5 私有工作状态，逐构造宏步提议、验证并原子接受，不改通用 `engine::Stage`。生产图最终只发布一个 `NaturalFormationBundleArtifact`，UI 从该 bundle 的最终子域快照读取字段，不发布历史、伪时间或求解中间态。

**Tech Stack:** Rust 2024、serde、thiserror、blake3、现有 Stage/Artifact/BuildCancellation、现有 P2–P5 生产算子与 egui/eframe 呈现层；不新增第三方依赖。

**Spec:** `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`

## Global Constraints

- 只发布最终当前态；P2/P3/P4/P5 的逐步状态、候选、拒绝步、伪时间和重试日程不得进入 schema、artifact、缓存恢复契约或 UI。
- P2 只拥有固体地球演化、地壳物质与构造成因；P3 只做同一时刻固体状态到基础地形的投影；P4 只解当前边界上的快平衡；P5 只拥有地表、水圈、沉积和地表载荷 Airy 响应。
- 生产 P3/P4/P5 不得读取 `compatibility.tectonic_elevation_m`；只改这一兼容字段不得改变任何权威下游结果。
- 跨步累计的高程组成和沉积质量库存保留 `f64`；`f32` 只在经过完整 `f64` 校验后生成 wire/GPU 快照，且不得回流成为下一步状态。
- 不得 clamp 科学状态，不得扩大 `ELEVATION_MIN_M`/`ELEVATION_MAX_M`，不得按目标地貌做重映射、经验修形或特殊格元分支。
- 不增加世界年龄、最高程、耦合 cadence、收敛容差或显示范围旋钮；形成时间线是 resolved 输入身份，不是本轮 UI 配置。
- 不增加通用循环 stage、多输出 stage、反馈 trait 或无人消费的适配器；领域协调发生在 `src/generators/natural/`。
- 每个任务严格 RED→GREEN→提交；无法产生 RED 的纯删除/文档步骤使用变异探针证明守门能力，并在提交正文如实说明。
- 当前工作树已有未提交的 P5 R4、`f64`、UI 发布事务和测试改动。不得 reset、checkout、删除或混入无关提交；修改重叠文件时使用 `git add -p -- <paths>`，提交前必须检查 `git diff --cached --name-only` 与 `git diff --cached`。
- 迭代期 P5/全链目标测试使用 `--release`；最终必须运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo check --target wasm32-unknown-unknown --all-features --lib` 和完整调试回归。
- 算法任务在 UI 接入并由用户亲自验证前不得宣称交付完成。

---

## 文件与职责图

### 新建文件

- `src/world/natural/formation_bundle.rs`：最终当前态 bundle、schema、严格验证和只读子域访问器。
- `src/generators/natural/causal_formation.rs`：P2/P3/P4/P5 私有因果协调器；只编排现有算子，不复制方程。
- `src/generators/natural/causal_formation_stage.rs`：单输出 `NaturalFormationBundleArtifact`、质量报告封装和生产 stage/graph。
- `src/generators/natural/surface_formation/state.rs`：跨步 `f64` 高程组成与沉积质量库存；最终 wire 投影的唯一入口。
- `tests/geologic_pipeline_contracts.rs`：模块所有权、兼容字段消融、bundle 原子性和生产图契约。
- `tests/causal_formation_generation.rs`：有限时间、确定性、守恒、无重置与错误分类集成测试。
- `tests/causal_formation_performance.rs`：Release-only 完整时间线、内存、取消和步长误差实测。
- `tests/support/causal_formation.rs`：跨集成测试共享的生产输入/构建 fixture，不重写算法。

### 重点修改文件

- `src/world/natural/formation.rs`：`ResolvedFormationTimeline` 及其在 `ResolvedWorldFormation` 中的身份。
- `src/world/natural/evolved_tectonics.rs`：不暴露兼容高程的 `AuthoritativeTectonicView<'a>`。
- `src/world/natural/surface_formation.rs`：把稳态报告替换为有限时间演化报告并刷新 schema/模型指纹。
- `src/generators/natural/spherical_tectonics/{runner,publication,forcing}.rs`：逐步 P2、当前态预览发布和 forcing 去兼容高程。
- `src/generators/natural/spherical_tectonics/processes/relaxation.rs`：固体年龄推进与 legacy 高程松弛分离。
- `src/generators/natural/{geologic_substrate,primary_relief}.rs`：只消费权威视图，P3 不继承兼容高程。
- `src/generators/natural/surface_formation/{generation,stream_power,sediment,hillslope,coast,isostasy}.rs`：有限物理时间 P5 步进、`f64` 累计状态和无重复构造积分。
- `src/generators/natural/global_circulation/{forcing,generation}.rs`：复用现有“formation terrain→forcing→P4”入口；不新增第二套气候方程。
- `src/app/spherical_formation_display.rs`、`src/app/spherical_presentation.rs`：从单一 bundle 装配当前态文档。
- `src/world/natural/fields.rs`、`src/ui/field/localization.rs`：保持字段注册表/本地化为 UI 文案 SSOT，只做 bundle payload 绑定所需调整。
- `src/generators/natural/mod.rs`、`src/world/natural/mod.rs`：最小可见性导出；生产切换后删除失去消费者的旧 stage/adapter。

### 固定接口流

```text
ResolvedWorldFormation.timeline()
        │
        ▼
EvolvedTectonicStepper::propose_next()
        │  EvolvedTectonicSnapshot
        ▼
AuthoritativeTectonicView ──► GeologicSubstrateGenerator
        │                     PrimaryReliefGenerator
        ▼
FormationState::apply_geologic_delta()
        │
        ├──► GlobalClimateForcingBuilder ─► GlobalCirculationGenerator
        │
        └──► advance_surface_processes()  ─► f64 surface/sediment state
                                                │
                                  validate + accept atomically
                                                │
                                                ▼
                                NaturalFormationBundleArtifact
                                                │
                                                ▼
                              SphericalFormationFieldDocument/UI
```

## 已完成的 Phase A 基线（本计划不重复改写）

- 设计事实已在 commit `e0af32d` 的 `2026-08-24-geologic-pipeline-contract-restoration-design.md` 冻结，并于 2026-08-25 获用户批准。
- `cargo test --release --lib generators::natural::surface_formation::generation::tests::` 已通过 `3/3`；`cargo test --release --test formation_coast_isostasy` 已通过 `7/7`，锁住完整 `f64` 域内/真实越界两个方向。
- Draft/seed `42` 已复现 `CellId(19366)` 的真实下界失败：完整候选 `-11000.000274626422 m`，唯一非零项是 `0.024603449 - 1.040666938 mm/year` 的构造净沉降；这否决外层绝对高程求根，不授权 clamp。
- 当前生产 `compatibility()` 消费清单：`geologic_substrate.rs` 读取 crust kind/thickness/age；`primary_relief.rs` 读取 plate/crust geometry 且错误读取 compatibility elevation；`quality/primary_relief.rs` 做 P3 证据；`app/spherical_formation_display.rs` 只呈现 plate/crust category；`quality/evolved_tectonics.rs` 与 evolved tests 审计 legacy V3 产品。Task 2/3 迁移前三项；呈现与 P2 legacy 审计可继续使用 compatibility，但不得把高程传回权威科学链。
- R4 固定 `100 ka` 探针已记录九组整步/两半步耗时 `13.8–17.9 min`，证明该固定时域与既有产品预算不相容；该数值只作失败基线，不进入新常量。Task 10 重新测量完整因果时间线。

---

### Task 1: 把形成时间线提升为 resolved 输入事实源

**Files:**
- Modify: `src/world/natural/formation.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Modify: `src/generators/natural/spherical_tectonics/publication.rs`
- Test: `tests/world_formation_spec.rs`
- Test: `tests/evolved_tectonic_generation.rs`

**Interfaces:**
- Consumes: `ResolvedWorldFormation::new(u16, WorldFormationPreset, ResolvedWorldFormationPreset)`；该构造器签名保持不变。
- Produces: `ResolvedFormationTimeline::cortial_reference()`, `step_count() -> u16`, `step_duration_kyr() -> u32`, `step_duration_myr() -> f64`, `total_duration_myr() -> f64`，以及 `ResolvedWorldFormation::timeline() -> ResolvedFormationTimeline`。

- [ ] **Step 1: 写时间线身份 RED**

在 `tests/world_formation_spec.rs` 增加：

```rust
#[test]
fn resolved_formation_carries_the_locked_cortial_timeline_in_its_identity() {
    let formation = resolve(42, WorldFormationPreset::Continents).formation().clone();
    let timeline = formation.timeline();
    assert_eq!(timeline.step_count(), 128);
    assert_eq!(timeline.step_duration_kyr(), 2_000);
    assert_eq!(timeline.step_duration_myr().to_bits(), 2.0_f64.to_bits());
    assert_eq!(timeline.total_duration_myr().to_bits(), 256.0_f64.to_bits());

    let encoded = serde_json::to_value(&formation).unwrap();
    assert_eq!(encoded["timeline"]["step_count"], 128);
    assert_eq!(encoded["timeline"]["step_duration_kyr"], 2_000);
}

#[test]
fn resolved_formation_rejects_a_forged_timeline() {
    let mut encoded = serde_json::to_value(
        resolve(42, WorldFormationPreset::Continents).formation(),
    )
    .unwrap();
    encoded["timeline"]["step_count"] = serde_json::json!(127);
    assert!(serde_json::from_value::<ResolvedWorldFormation>(encoded).is_err());
}
```

- [ ] **Step 2: 运行 RED**

Run: `cargo test --test world_formation_spec resolved_formation_`

Expected: 编译失败，指出 `timeline`/`ResolvedFormationTimeline` 尚不存在。

- [ ] **Step 3: 实现唯一时间线类型并迁移 P2**

在 `formation.rs` 使用整数 kyr 作为序列化身份，避免 `f64` 的 `Eq`/serde 歧义：

```rust
/// Number of finite evolution steps in the project reference schedule from
/// Cortial et al. (2019), DOI 10.1111/cgf.13628.
pub const CORTIAL_FORMATION_STEP_COUNT: u16 = 128;
/// Duration of one reference step, stored as integer kyr for identity-stable serde;
/// the 2 Myr value follows the same Cortial et al. (2019) reference schedule.
pub const CORTIAL_FORMATION_STEP_DURATION_KYR: u32 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedFormationTimeline {
    step_count: u16,
    step_duration_kyr: u32,
}

impl ResolvedFormationTimeline {
    pub const fn cortial_reference() -> Self {
        Self {
            step_count: CORTIAL_FORMATION_STEP_COUNT,
            step_duration_kyr: CORTIAL_FORMATION_STEP_DURATION_KYR,
        }
    }

    pub const fn step_count(self) -> u16 { self.step_count }
    pub const fn step_duration_kyr(self) -> u32 { self.step_duration_kyr }
    pub fn step_duration_myr(self) -> f64 { f64::from(self.step_duration_kyr) / 1_000.0 }
    pub fn total_duration_myr(self) -> f64 {
        f64::from(self.step_count) * self.step_duration_myr()
    }

    pub fn validate(self) -> Result<(), WorldFormationSpecError> {
        if self != Self::cortial_reference() {
            return Err(WorldFormationSpecError::UnsupportedTimeline {
                step_count: self.step_count,
                step_duration_kyr: self.step_duration_kyr,
            });
        }
        Ok(())
    }
}
```

把 `timeline` 嵌入 `ResolvedWorldFormation` 及 wire，`new` 固定写入 `cortial_reference()`，反序列化后同时校验。删除 runner 私有 `EVOLUTION_STEP_COUNT`/`EVOLUTION_DELTA_MYR`，两个 V4/V5 循环都从 `formation.timeline()` 读取；`generate_evolved_spherical` 和 runner 的 formation 参数改为 `&ResolvedWorldFormation`，只在 recipe 选择处调用 `.resolved()`。

- [ ] **Step 4: 运行 GREEN 与 P2 等价回归**

Run: `cargo test --test world_formation_spec`

Run: `cargo test --release --test evolved_tectonic_generation`

Expected: 全部 PASS；已有固定 seed 的 P2 snapshot/fingerprint 断言不变，只有包含 `ResolvedWorldFormationArtifact` 的 stage 输入身份刷新。

- [ ] **Step 5: 提交**

```bash
git add src/world/natural/formation.rs src/world/natural/mod.rs src/generators/natural/spherical_tectonics/runner.rs src/generators/natural/spherical_tectonics/publication.rs tests/world_formation_spec.rs tests/evolved_tectonic_generation.rs
git commit -m "Move formation timing into resolved world state" -m "Make the Cortial step schedule part of validated input identity before causal coupling consumes it."
```

---

### Task 2: 建立不暴露兼容高程的借用型权威构造视图

**Files:**
- Modify: `src/world/natural/evolved_tectonics.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/geologic_substrate.rs`
- Test: `tests/evolved_tectonic_contracts.rs`
- Test: `tests/geologic_substrate_generation.rs`

**Interfaces:**
- Consumes: `EvolvedTectonicSnapshot::{material,forcing,compatibility}`。
- Produces: `EvolvedTectonicSnapshot::authoritative_view() -> AuthoritativeTectonicView<'_>`；view 只提供 plates、plate ids、crust kind/thickness/age/lineation/orogeny、boundaries、material 和 forcing 的借用访问器，不提供 `tectonic_elevation_m` 或完整 compatibility snapshot。

- [ ] **Step 1: 写借用与不可见性 RED**

在 `evolved_tectonics.rs` 文档中增加 compile-fail 契约，并在集成测试验证零复制：

```rust
/// ```compile_fail
/// use sekai::world::natural::AuthoritativeTectonicView;
/// fn forbidden(view: AuthoritativeTectonicView<'_>) {
///     let _ = view.tectonic_elevation_m();
/// }
/// ```
```

```rust
#[test]
fn authoritative_view_borrows_only_causal_fields() {
    let snapshot = evolved_fixture().evolved;
    let view = snapshot.authoritative_view();
    assert!(std::ptr::eq(
        view.crust_thickness_km().as_ptr(),
        snapshot.compatibility().crust_thickness_km().as_ptr(),
    ));
    assert!(std::ptr::eq(view.material(), snapshot.material()));
    assert!(std::ptr::eq(view.forcing(), snapshot.forcing()));
}
```

- [ ] **Step 2: 运行 RED**

Run: `cargo test --doc authoritative_view`

Run: `cargo test --test evolved_tectonic_contracts authoritative_view_`

Expected: integration test 编译失败，指出 `authoritative_view` 尚不存在。

- [ ] **Step 3: 实现最小借用 view 并迁移 substrate**

核心类型固定为：

```rust
#[derive(Debug, Clone, Copy)]
pub struct AuthoritativeTectonicView<'a> {
    snapshot: &'a EvolvedTectonicSnapshot,
}

impl EvolvedTectonicSnapshot {
    pub const fn authoritative_view(&self) -> AuthoritativeTectonicView<'_> {
        AuthoritativeTectonicView { snapshot: self }
    }
}

impl<'a> AuthoritativeTectonicView<'a> {
    pub const fn surface_ref(self) -> SurfaceRef { self.snapshot.compatibility.surface_ref() }
    pub fn plates(self) -> &'a [SphericalPlate] { self.snapshot.compatibility.plates() }
    pub const fn cell_plates(self) -> &'a PlateIdField {
        self.snapshot.compatibility.cell_plates()
    }
    pub const fn material(self) -> &'a SphericalCrustMaterialState { &self.snapshot.material }
    pub const fn forcing(self) -> &'a SphericalTectonicForcingState { &self.snapshot.forcing }
    pub fn crust_kinds(self) -> &'a CrustKindField { self.snapshot.compatibility.crust_kinds() }
    pub fn crust_thickness_km(self) -> &'a [f32] {
        self.snapshot.compatibility.crust_thickness_km()
    }
    pub fn crust_age_myr(self) -> &'a [f32] { self.snapshot.compatibility.crust_age_myr() }
    pub fn lineation_east(self) -> &'a [f32] { self.snapshot.compatibility.lineation_east() }
    pub fn lineation_north(self) -> &'a [f32] { self.snapshot.compatibility.lineation_north() }
    pub fn orogeny_kind(self) -> &'a [SphericalOrogenyKind] {
        self.snapshot.compatibility.orogeny_kind()
    }
    pub fn orogeny_age_myr(self) -> &'a [f32] {
        self.snapshot.compatibility.orogeny_age_myr()
    }
    pub fn boundaries(self) -> &'a [BoundaryRecord] { self.snapshot.compatibility.boundaries() }
    pub fn boundary_segments(self) -> &'a [SphericalBoundarySegment] {
        self.snapshot.compatibility.boundary_segments()
    }
}
```

以上就是本任务允许的完整访问器集合；禁止返回 `&SphericalTectonicSnapshot`、`compatibility()` 或 `tectonic_elevation_m()`。`GeologicSubstrateGenerator::generate` 先取得 `let tectonic = evolved.authoritative_view();`，材料、forcing 和 crust 字段全部经该 view 读取。

- [ ] **Step 4: 运行 GREEN**

Run: `cargo test --doc authoritative_view`

Run: `cargo test --release --test evolved_tectonic_contracts --test geologic_substrate_generation`

Expected: 全部 PASS；compile-fail doctest 证明 view 无兼容高程入口，借用测试证明没有新 snapshot/数组复制。

- [ ] **Step 5: 提交**

```bash
git add src/world/natural/evolved_tectonics.rs src/world/natural/mod.rs src/generators/natural/geologic_substrate.rs tests/evolved_tectonic_contracts.rs tests/geologic_substrate_generation.rs
git commit -m "Isolate authoritative tectonic inputs" -m "Give P3 a borrowed cause-only view that cannot expose compatibility elevation."
```

---

### Task 3: 让 P3 成为无历史权威投影并删除事后高程修形

**Files:**
- Modify: `src/generators/natural/primary_relief.rs`
- Modify: `src/generators/natural/spherical_island_relief.rs`
- Modify: `src/generators/natural/spherical_relief.rs`
- Modify: `src/generators/natural/spherical_relief/directed_noise.rs`
- Modify: `src/generators/natural/spherical_mantle.rs`
- Modify: `src/generators/natural/geologic_substrate.rs`
- Modify: `src/world/natural/primary_relief.rs`
- Test: `tests/geologic_pipeline_contracts.rs`
- Test: `tests/primary_relief_generation.rs`
- Test: `tests/primary_relief_quality.rs`

**Interfaces:**
- Consumes: `AuthoritativeTectonicView<'_>`、`GeologicSubstrateSnapshot`、`ReliefSpec`。
- Produces: `PrimaryReliefGenerator::generate` 的签名保持不变；新增 crate-private `GeologicSubstrateGenerator::generate_from_streams` 与 `PrimaryReliefGenerator::generate_from_streams` 供协调器复用同一随机身份；内部所有构造读取改走 view；`dynamic_tectonic_response_m(uplift_rate_mm_per_year, subsidence_rate_mm_per_year) -> Result<f32, PrimaryReliefGenerationError>` 不再接收累计兼容高程。

- [ ] **Step 1: 写兼容高程消融 RED**

在新测试文件中复用生产 P2/P3 fixture，唯一变异 compatibility elevation：

```rust
#[test]
fn compatibility_elevation_alone_cannot_change_authoritative_p3() {
    let fixture = authoritative_p3_fixture(RootSeed::new(42));
    let mut wire = serde_json::to_value(&fixture.evolved).unwrap();
    let values = wire["compatibility"]["crust"]["tectonic_elevation_m"]
        .as_array_mut()
        .unwrap();
    for (index, value) in values.iter_mut().enumerate() {
        *value = serde_json::json!(-8_000.0_f32 + index as f32 * 0.125);
    }
    let mutated: EvolvedTectonicSnapshot = serde_json::from_value(wire).unwrap();

    let original = generate_p3_from_evolved(&fixture, &fixture.evolved);
    let changed = generate_p3_from_evolved(&fixture, &mutated);
    assert_eq!(changed.substrate, original.substrate);
    assert_eq!(changed.relief, original.relief);
}
```

`authoritative_p3_fixture` 与 `generate_p3_from_evolved` 写在同一测试文件，直接调用 `GeologicSubstrateGenerator`、`PrimaryReliefGenerator` 和固定 `derive_stage_seed`；不得复制 P3 公式。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --test geologic_pipeline_contracts compatibility_elevation_alone_`

Expected: FAIL，当前大陆 `dynamic_tectonic_offset_m`/最终 relief 随变异改变。

- [ ] **Step 3: 删除兼容继承并改为 typed 越界**

将动态项改为只消费当前成因率：

```rust
pub fn dynamic_tectonic_response_m(
    uplift_rate_mm_per_year: f32,
    subsidence_rate_mm_per_year: f32,
) -> Result<f32, PrimaryReliefGenerationError> {
    let response = DYNAMIC_RATE_RESPONSE_M_PER_MM_PER_YEAR
        * (uplift_rate_mm_per_year - subsidence_rate_mm_per_year);
    if !response.is_finite() || !(TECTONIC_OFFSET_MIN_M..=TECTONIC_OFFSET_MAX_M).contains(&response) {
        return Err(PrimaryReliefGenerationError::DynamicTectonicOutOfRange { found: response });
    }
    Ok(response)
}
```

删除 `DYNAMIC_ACCUMULATED_RESPONSE_WEIGHT` 与 `causal_accumulated_response_m`。把 hotspot、passive-margin、conditioned-detail helper 参数改为 `AuthoritativeTectonicView<'_>` 或其最窄字段。删除 `reconcile_primary_safety`、`constrain_regional_pair`、`adjust_component` 和 clamp diagnostic；以完整 `f64` 和校验替代：

```rust
fn compose_primary_elevation(
    isostatic: &[f32],
    dynamic: &[f32],
    volcanic: &[f32],
    passive: &[f32],
    detail: &[f32],
) -> Result<Vec<f32>, PrimaryReliefGenerationError> {
    let mut elevation = Vec::with_capacity(isostatic.len());
    for index in 0..isostatic.len() {
        let exact = [isostatic[index], dynamic[index], volcanic[index], passive[index], detail[index]]
            .into_iter()
            .map(f64::from)
            .sum::<f64>();
        if !exact.is_finite()
            || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&exact)
        {
            return Err(PrimaryReliefGenerationError::ElevationOutOfRange {
                cell: CellId::from_raw(index as u32),
                found: exact,
            });
        }
        elevation.push(exact as f32);
    }
    Ok(elevation)
}
```

现有各物理分量自身有出处的输入域限制保持；任何生成结果超出分量或总高程 artifact 域都 typed fail，不对结果 clamp。另把 `MantleGenerator::generate_spherical_from_streams`、`GeologicSubstrateGenerator::generate_from_streams` 与 `PrimaryReliefGenerator::generate_from_streams` 定为 crate-private；现有 public `generate` 只负责 capture 一次 `LabeledSubstreams` 后转调，协调器则在每个宏步复用同一组标签身份。

- [ ] **Step 4: 运行 GREEN 与 P3 质量否决门**

Run: `cargo test --release --test geologic_pipeline_contracts compatibility_elevation_alone_`

Run: `cargo test --release --test primary_relief_generation --test primary_relief_quality --test primary_relief_evidence`

Expected: 消融 PASS，P3 质量/证据全 PASS。若 morphology envelope 失败，本任务停在 RED，输出失败指标并按规格登记“物质/过程缺口”；禁止恢复兼容高程、恢复修形或调系数。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/primary_relief.rs src/generators/natural/spherical_island_relief.rs src/generators/natural/spherical_relief.rs src/generators/natural/spherical_relief/directed_noise.rs src/generators/natural/spherical_mantle.rs src/generators/natural/geologic_substrate.rs src/world/natural/primary_relief.rs tests/geologic_pipeline_contracts.rs tests/primary_relief_generation.rs tests/primary_relief_quality.rs
git commit -m "Restore P3 as an authoritative projection" -m "Remove compatibility elevation inheritance and reject unsupported complete elevations without post-hoc reshaping."
```

---

### Task 4: 拆分 P2 固体年龄推进与 legacy 表面响应

**Files:**
- Modify: `src/generators/natural/spherical_tectonics/processes/relaxation.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/mod.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Modify: `src/generators/natural/spherical_tectonics/forcing.rs`
- Test: `tests/evolved_tectonic_forcing.rs`
- Test: `tests/evolved_tectonic_material.rs`
- Test: `tests/evolved_tectonic_quality.rs`
- Test: `tests/evolved_tectonic_evidence.rs`

**Interfaces:**
- Consumes: 当前 V4/V5 runner 与 `CrustSample`。
- Produces: `advance_solid_crust_ages(next: &mut TectonicState, delta_myr: f32)`；`relax_legacy_compatibility_elevation(...)` 只被 V4 compatibility loop 调用；V5 forcing 与该 legacy 高程正交。

- [ ] **Step 1: 写 forcing/所有权 RED**

在 forcing 单元测试中构造两个除 `tectonic_elevation_m` 外逐位相同的 `TectonicState`：

```rust
#[test]
fn authoritative_forcing_is_independent_of_legacy_elevation() {
    let (surface, topology, state, recipe) = forcing_fixture();
    let mut changed = state.clone();
    for (index, sample) in changed.samples.iter_mut().enumerate() {
        sample.tectonic_elevation_m = -9_000.0 + index as f32;
    }
    assert_eq!(
        evaluate_present_day_forcing(&surface, &topology, &state, recipe).unwrap(),
        evaluate_present_day_forcing(&surface, &topology, &changed, recipe).unwrap(),
    );
}

#[test]
fn forcing_support_domain_rejects_instead_of_clamping() {
    let cell = CellId::from_raw(7);
    let error = checked_forcing_rate(cell, "uplift_rate_mm_per_year", 500.25).unwrap_err();
    assert!(matches!(
        error,
        ForcingError::Invalid(EvolvedTectonicValidationError::ForcingRateOutOfRange {
            field: "uplift_rate_mm_per_year",
            cell: found_cell,
            found,
            max,
        }) if found_cell == cell
            && found.to_bits() == 500.25_f32.to_bits()
            && max.to_bits() == MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR.to_bits()
    ));
}
```

在 relaxation 测试中分别断言年龄推进不改高程、legacy 路径仍满足冻结 V4 行为。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --lib spherical_tectonics::forcing::tests`

Expected: 编译失败，因为 `checked_forcing_rate` 尚不存在；实现最小 helper 后，独立性测试仍 FAIL，因为 `descending_relief_weight` 继续读取 legacy elevation。

- [ ] **Step 3: 最小拆分并删除 V5 表面过程**

```rust
pub(super) fn advance_solid_crust_ages(
    next: &mut TectonicState,
    delta_myr: f32,
) -> Result<ProcessStats, ProcessError> {
    validate_delta_myr(delta_myr)?;
    for sample in &mut next.samples {
        if sample.kind == CrustKind::Oceanic {
            sample.age_myr = (sample.age_myr + delta_myr).min(MAX_CRUST_AGE_MYR);
        }
        if sample.orogeny != SphericalOrogenyKind::None {
            sample.orogeny_age_myr =
                (sample.orogeny_age_myr + delta_myr).min(MAX_CRUST_AGE_MYR);
        }
    }
    Ok(ProcessStats { relaxed_samples: next.samples.len() as u32, ..ProcessStats::default() })
}
```

V5 loop 只调用 `advance_solid_crust_ages`；V4 loop 顺序调用年龄推进和显式命名的 `relax_legacy_compatibility_elevation`。`evaluate_present_day_forcing` 删除 `descending_relief_weight`，subduction uplift 直接使用已有 `subduction_profile` 输出。同步删除 subduction/collision forcing 写入处的全部 `clamp(0.0, 500.0)`；现有 `MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR` 仍只是 artifact 支持域，由同一 typed validator 拒绝越界：

```rust
fn checked_forcing_rate(
    cell: CellId,
    field: &'static str,
    exact: f64,
) -> Result<f32, ForcingError> {
    let found = exact as f32;
    if !exact.is_finite()
        || !(0.0..=f64::from(MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR)).contains(&exact)
    {
        return Err(EvolvedTectonicValidationError::ForcingRateOutOfRange {
            field,
            cell,
            found,
            max: MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR,
        }
        .into());
    }
    Ok(found)
}

let rate = checked_forcing_rate(
    sample.anchor,
    "uplift_rate_mm_per_year",
    f64::from(uplift_step_m) * METRES_PER_STEP_TO_MM_PER_YEAR,
)?;
```

subduction subsidence、subduction uplift、collision shortening 与 collision uplift 四个写入点共用该 helper；这里的乘数 `1` 表示删除无权威依据的兼容高程 modifier，不新增常量、新阈值或新方程。上面的越界测试锁定 typed failure，且错误中的值没有被改成支持域上界。

- [ ] **Step 4: 运行 GREEN 与 V4/V5 回归**

Run: `cargo test --lib spherical_tectonics::`

Run: `cargo test --release --test evolved_tectonic_forcing --test evolved_tectonic_material --test evolved_tectonic_quality --test evolved_tectonic_evidence --test spherical_tectonic_generation`

Expected: 全部 PASS；V5 material budget 保持闭合，V4 compatibility 测试保持冻结。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/spherical_tectonics/processes/relaxation.rs src/generators/natural/spherical_tectonics/processes/mod.rs src/generators/natural/spherical_tectonics/runner.rs src/generators/natural/spherical_tectonics/forcing.rs tests/evolved_tectonic_forcing.rs tests/evolved_tectonic_material.rs tests/evolved_tectonic_quality.rs tests/evolved_tectonic_evidence.rs
git commit -m "Separate solid-earth aging from legacy relief" -m "Keep surface erosion and trench fill out of the V5 authority path and remove compatibility elevation from forcing."
```

---

### Task 5: 把 P2 runner 提取为可提议/接受的逐步演化器

**Files:**
- Create: `src/generators/natural/spherical_tectonics/stepper.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Modify: `src/generators/natural/spherical_tectonics/publication.rs`
- Modify: `src/generators/natural/spherical_tectonics/workspace.rs`
- Modify: `src/generators/natural/random.rs`
- Test: `tests/evolved_tectonic_generation.rs`
- Test: `tests/spherical_tectonic_causality.rs`

**Interfaces:**
- Consumes: `ProfileSurfaceBundle`, `ResolvedWorldFormation::timeline()`, P2 process kernels与 conservative remap。
- Produces: crate-private `EvolvedTectonicStepper<'a>`、`TectonicStepCandidate`、`propose_next()`, `accept()`, `finish()`；协调器只能取得候选权威 snapshot，不能直接改 P2 workspace。

- [ ] **Step 1: 写 monolithic/stepper 等价 RED**

在 `evolved_tectonic_generation.rs` 增加测试辅助入口（仅 `#[cfg(test)]` 对外）：

```rust
#[test]
fn accepted_stepper_sequence_matches_the_monolithic_p2_product() {
    for seed in [3_u64, 7, 42] {
        let fixture = evolved_fixture_for_seed(seed);
        let monolithic = generate_evolved(&fixture);
        let stepped = generate_evolved_step_by_step(&fixture);
        assert_eq!(stepped, monolithic, "seed {seed}");
    }
}
```

另加拒绝候选测试：`propose_next` 后不调用 `accept`，再次提议必须产生与第一次逐位相同的 candidate。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --test evolved_tectonic_generation accepted_stepper_sequence_`

Expected: 编译失败，指出 stepper 测试入口不存在。

- [ ] **Step 3: 提取逐步状态机而不改变 P2 方程顺序**

固定接口：

```rust
pub(in crate::generators::natural) struct EvolvedTectonicStepper<'a> {
    bundle: &'a ProfileSurfaceBundle,
    timeline: ResolvedFormationTimeline,
    next_step: u16,
    workspace: TectonicWorkspace,
    material_ledger: EvolutionMaterialLedger,
    lineage_ledger: EvolutionLineageLedger,
    streams: LabeledSubstreams,
    last_snapshot: EvolvedTectonicSnapshot,
}

pub(in crate::generators::natural) struct TectonicStepCandidate {
    step_index: u16,
    workspace: TectonicWorkspace,
    material_ledger: EvolutionMaterialLedger,
    lineage_ledger: EvolutionLineageLedger,
    snapshot: EvolvedTectonicSnapshot,
}

impl<'a> EvolvedTectonicStepper<'a> {
    pub fn new(
        bundle: &'a ProfileSurfaceBundle,
        spec: &TectonicSpec,
        formation: &ResolvedWorldFormation,
        streams: LabeledSubstreams,
    ) -> Result<Self, EvolvedPublicationError>;
    pub fn current_snapshot(&self) -> &EvolvedTectonicSnapshot;
    pub fn propose_next(&self) -> Result<TectonicStepCandidate, EvolvedPublicationError>;
    pub fn accept(&mut self, candidate: TectonicStepCandidate) -> Result<(), EvolvedPublicationError>;
    pub fn finish(self) -> Result<EvolvedTectonicSnapshot, EvolvedPublicationError>;
}
```

给 `LabeledSubstreams`、workspace 与 ledger 增加仅为候选事务所需的 `Clone`。`new` 从初始固体状态生成并保存 `last_snapshot`，让协调器能在第一宏步前建立 P3/P5 初态。`propose_next` 复制当前/next 双缓冲和两个 ledger，按原 runner 的既定顺序执行恰好一宏步，在临时候选上做必要 resample/remap 和预算校验；失败不触及 `self`。`accept` 校验 `candidate.step_index == self.next_step` 后替换工作状态与 `last_snapshot`。`finish` 只允许 `next_step == timeline.step_count()`，返回最后一个已接受 snapshot。原 `evolve_control_state_v5` 改为循环 `propose_next`/`accept`/`finish`，成为等价适配器。

- [ ] **Step 4: 运行 GREEN、确定性和预算回归**

Run: `cargo test --release --test evolved_tectonic_generation --test spherical_tectonic_causality --test evolved_tectonic_material --test evolved_tectonic_publication`

Expected: 全部 PASS；三 seed 最终产品逐位等价，拒绝候选不改变下一次提议。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/spherical_tectonics/stepper.rs src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/runner.rs src/generators/natural/spherical_tectonics/publication.rs src/generators/natural/spherical_tectonics/workspace.rs src/generators/natural/random.rs tests/evolved_tectonic_generation.rs tests/spherical_tectonic_causality.rs
git commit -m "Expose atomic tectonic evolution steps" -m "Let the natural formation coordinator couple to P2 without publishing history or mutating rejected candidates."
```

---

### Task 6: 建立跨步 `f64` 地形组成状态

**Files:**
- Create: `src/generators/natural/surface_formation/state.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/surface_formation/isostasy.rs`
- Test: `src/generators/natural/surface_formation/generation.rs`
- Test: `tests/formation_coast_isostasy.rs`

**Interfaces:**
- Consumes: `PrimaryReliefSnapshot`, `FormationElevationComponents`, `LocalAiryIsostasy::response_from_validated_surface`。
- Produces: `FormationState::from_primary(&PrimaryReliefSnapshot) -> Result<Self, FormationStateError>`、`apply_geologic_delta(&mut self, previous_primary_m: &[f32], current_primary_m: &[f32]) -> Result<(), FormationStateError>`、`apply_surface_displacement_f64(&mut self, displacement_m: &[f64]) -> Result<(), FormationStateError>`、`current_elevation_exact_m(&self) -> &[f64]`、`current_elevation_f32(&self) -> &[f32]` 和 `wire_components(&self) -> Result<FormationElevationComponents, FormationStateError>`；`#[cfg(test)] pub(super) from_primary_values(Vec<f32>)` 只供 `surface_formation` 模块树内解析测试。所有累计值保存在 `f64`，不另设 f32 位移写入口。

- [ ] **Step 1: 写亚 ULP、地质差量和真实越界 RED**

在 `state.rs` 单元测试固定三个解析契约：

```rust
#[test]
fn sub_ulp_surface_changes_accumulate_without_f32_feedback() {
    let mut state = FormationState::from_primary_values(vec![9_000.0]).unwrap();
    state.apply_surface_displacement_f64(&[-0.0003]).unwrap();
    state.apply_surface_displacement_f64(&[-0.0003]).unwrap();
    assert_eq!(state.surface_adjustment_m()[0].to_bits(), (-0.0006_f64).to_bits());
    assert!(state.current_elevation_exact_m()[0] < 9_000.0);
}

#[test]
fn geologic_delta_preserves_the_retained_surface_history() {
    let mut state = FormationState::from_primary_values(vec![100.0]).unwrap();
    state.apply_surface_displacement_f64(&[-12.0]).unwrap();
    state.apply_geologic_delta(&[100.0], &[130.0]).unwrap();
    assert_eq!(state.primary_relief_exact_m(), &[130.0]);
    assert_eq!(state.surface_adjustment_m(), &[-12.0]);
    assert_eq!(state.current_elevation_exact_m(), &[118.0]);
}

#[test]
fn exact_f64_state_rejects_a_true_overflow_before_wire_rounding() {
    let mut state = FormationState::from_primary_values(vec![ELEVATION_MAX_M]).unwrap();
    assert!(matches!(
        state.apply_surface_displacement_f64(&[0.000_01]),
        Err(FormationStateError::ElevationOutOfRange { found, .. }) if found > 9_000.0
    ));
}
```

- [ ] **Step 2: 运行 RED**

Run: `cargo test --lib surface_formation::state::tests`

Expected: 编译失败，`FormationState` 尚不存在。

- [ ] **Step 3: 抽取现有 `ComponentState` 并锁定唯一 wire 边界**

```rust
pub(super) struct FormationState {
    primary_relief_m: Vec<f64>,
    surface_adjustment_m: Vec<f64>,
    current_elevation_m: Vec<f64>,
    elevation_f32: Vec<f32>,
}

impl FormationState {
    fn rebuild_and_validate(&mut self) -> Result<(), FormationStateError> {
        for index in 0..self.current_elevation_m.len() {
            let exact = self.primary_relief_m[index] + self.surface_adjustment_m[index];
            if !exact.is_finite()
                || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&exact)
            {
                return Err(FormationStateError::ElevationOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: exact,
                });
            }
            self.current_elevation_m[index] = exact;
            self.elevation_f32[index] = exact as f32;
        }
        Ok(())
    }
}
```

`wire_components` 是唯一 `f64→f32` 组成转换，并复用 `formation_elevation_from_components` 验证发布恒等式。删除 `generation.rs` 私有 `ComponentState`，所有过程位移和 Airy `Vec<f64>` 先进入 `FormationState`，再取只读 `elevation_f32` scratch 给现有 kernel；scratch 不得被复制回 exact state。

- [ ] **Step 4: 运行 GREEN 与原失败回归**

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Run: `cargo test --release --test formation_coast_isostasy`

Expected: 全部 PASS；`9000.000260834617` 一类 f32 身份误报不再出现，真实越界仍返回未裁剪 `f64`。

- [ ] **Step 5: 提交**

```bash
git add -p -- src/generators/natural/surface_formation/state.rs src/generators/natural/surface_formation/mod.rs src/generators/natural/surface_formation/generation.rs src/generators/natural/surface_formation/isostasy.rs tests/formation_coast_isostasy.rs
git diff --cached --check
git commit -m "Retain formation elevation in f64" -m "Prevent wire precision from feeding back into causal terrain accumulation and preserve exact range diagnostics."
```

---

### Task 7: 把沉积库存改为 `f64` 质量事实源

**Files:**
- Modify: `src/generators/natural/surface_formation/state.rs`
- Modify: `src/generators/natural/surface_formation/sediment.rs`
- Modify: `src/generators/natural/surface_formation/hillslope.rs`
- Modify: `src/generators/natural/surface_formation/coast.rs`
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Test: `src/generators/natural/surface_formation/state.rs`
- Test: `tests/formation_sediment.rs`
- Test: `tests/formation_hillslope.rs`
- Test: `tests/formation_coast_isostasy.rs`

**Interfaces:**
- Consumes: 现有五来源 `SEDIMENT_PROVENANCE_SOURCE_COUNT`、removed/deposited `f64` mass arrays。
- Produces: `SedimentStockState`，内部唯一状态为每格每来源 `kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>`；`apply_transfer(&mut self, removed_by_source_kg, deposited_by_source_kg) -> Result<(), FormationStateError>` 原子更新库存；`to_wire_fields(&self, cell_area_m2: &[f64], bulk_density_kg_m3: &[f64]) -> Result<FormationSedimentFields, FormationStateError>` 只在候选验证/发布时换算厚度与比例；`#[cfg(test)] empty` 与 `mass_by_source_kg` 只供同模块解析测试。

- [ ] **Step 1: 写质量累计 RED**

```rust
#[test]
fn sub_wire_sediment_mass_survives_repeated_steps() {
    let mut stock = SedimentStockState::empty(1);
    for _ in 0..1_000 {
        stock.apply_transfer(
            &[[0.0, 0.0, 0.0, 0.0, 0.0]],
            &[[0.01, 0.0, 0.0, 0.0, 0.0]],
        )
        .unwrap();
    }
    assert!((stock.mass_by_source_kg()[0][0] - 10.0).abs() <= 1.0e-12);
}

#[test]
fn wire_projection_never_becomes_the_next_stock() {
    let mut stock = SedimentStockState::empty(1);
    stock.apply_transfer(
        &[[0.0, 0.0, 0.0, 0.0, 0.0]],
        &[[0.01, 0.02, 0.03, 0.04, 0.05]],
    )
    .unwrap();
    let _wire = stock.to_wire_fields(&[1_000_000.0], &[2_000.0]).unwrap();
    assert_eq!(stock.mass_by_source_kg()[0], [0.01, 0.02, 0.03, 0.04, 0.05]);
}
```

两个解析测试放在 `state.rs` 的 `#[cfg(test)]` 模块；integration tests 只通过现有 public sediment kernels 验证端到端守恒，不为测试放宽 `SedimentStockState` 可见性。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --lib generators::natural::surface_formation::state::tests`

Expected: 编译失败，`SedimentStockState` 尚不存在。

- [ ] **Step 3: 让所有移除/沉积直接记入 f64 质量库存**

固定状态接口：

```rust
pub(super) struct SedimentStockState {
    mass_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

impl SedimentStockState {
    pub fn apply_transfer(
        &mut self,
        removed_by_source_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
        deposited_by_source_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    ) -> Result<(), FormationStateError> {
        if removed_by_source_kg.len() != self.mass_by_source_kg.len()
            || deposited_by_source_kg.len() != self.mass_by_source_kg.len()
        {
            return Err(FormationStateError::SedimentLengthMismatch {
                expected: self.mass_by_source_kg.len(),
                removed: removed_by_source_kg.len(),
                deposited: deposited_by_source_kg.len(),
            });
        }
        let mut candidate = self.mass_by_source_kg.clone();
        for cell in 0..candidate.len() {
            for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
                let removed = removed_by_source_kg[cell][source];
                let deposited = deposited_by_source_kg[cell][source];
                let next = candidate[cell][source] - removed + deposited;
                if !removed.is_finite()
                    || !deposited.is_finite()
                    || removed < 0.0
                    || deposited < 0.0
                    || !next.is_finite()
                    || next < 0.0
                {
                    return Err(FormationStateError::SedimentMassOutOfRange {
                        cell: CellId::from_raw(cell as u32),
                        source,
                        found: next,
                    });
                }
                candidate[cell][source] = next;
            }
        }
        self.mass_by_source_kg = candidate;
        Ok(())
    }
}
```

`FormationStateError` 同步增加上面使用的 `SedimentLengthMismatch { expected, removed, deposited }` 与 `SedimentMassOutOfRange { cell, source, found: f64 }`；不得把负库存归零。

`remove_fluvial_sediment_cover`、hillslope、coast 和 router 改为读取 exact stock；每个候选先在 clone 上扣除/增加质量，任何负库存或 budget mismatch 都拒绝整个候选。`FormationSedimentFields` 保留为最终/计算 scratch，但不再作为下一步库存输入。

- [ ] **Step 4: 运行 GREEN 与全沉积守恒族**

Run: `cargo test --release --test formation_sediment --test formation_hillslope --test formation_coast_isostasy`

Expected: 全部 PASS；五来源与全球质量预算逐位闭合，wire round-trip 不改变 retained stock。

- [ ] **Step 5: 提交**

```bash
git add -p -- src/generators/natural/surface_formation/state.rs src/generators/natural/surface_formation/sediment.rs src/generators/natural/surface_formation/hillslope.rs src/generators/natural/surface_formation/coast.rs src/generators/natural/surface_formation/generation.rs tests/formation_sediment.rs tests/formation_hillslope.rs tests/formation_coast_isostasy.rs
git diff --cached --check
git commit -m "Retain sediment inventory as f64 mass" -m "Make five-source solid stock the cross-step truth and keep thickness and fractions as validated wire projections."
```

---

### Task 8: 提取有限物理时间 P5 步进并退役外层绝对稳态求根

**Files:**
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/surface_formation/stream_power.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/world/natural/surface_formation.rs`
- Test: `src/generators/natural/surface_formation/generation.rs`
- Test: `tests/formation_stream_power.rs`
- Test: `tests/surface_formation_contracts.rs`

**Interfaces:**
- Consumes: `FormationState`、当前 P4、P5 hydrology/stream-power/hillslope/coast/sediment/Airy kernel。
- Produces: `advance_surface_processes(state, inputs, duration_years, cancellation) -> SurfaceAdvanceReport`，恰好消费请求物理时长；`finalize_surface_formation(state, final_inputs, evolution_report, cancellation) -> NaturalSurfaceFormationSnapshot` 是唯一 P5 wire 发布入口；不再把 P2 瞬时率再次积分进地形。

- [ ] **Step 1: 写有限时间与无双计数 RED**

在 `generation.rs` 的 `#[cfg(test)]` 模块写测试，以便直接调用最小可见性的 state/stepper；integration test 不放宽生产可见性：

```rust
#[test]
fn surface_step_consumes_the_complete_requested_duration() {
    let fixture = surface_formation_fixture();
    let mut state = FormationState::from_primary(fixture.primary_relief()).unwrap();
    let report = advance_surface_processes(
        &mut state,
        fixture.surface_process_inputs(),
        2_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(report.accepted_duration_years().to_bits(), 2_000.0_f64.to_bits());
    assert!(report.accepted_substeps() > 0);
}

#[test]
fn tectonic_delta_is_applied_once_by_the_coordinator_boundary() {
    let fixture = zero_surface_process_fixture();
    let mut state = FormationState::from_primary_values(vec![100.0]).unwrap();
    state.apply_geologic_delta(&[100.0], &[125.0]).unwrap();
    advance_surface_processes(
        &mut state,
        fixture.inputs(),
        10_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(state.current_elevation_exact_m(), &[125.0]);
}
```

同一 test module 增加 `zero_surface_process_fixture() -> SurfaceProcessInputs<'static>`，复用已有 production fixture 的 surface/substrate/climate/spec，只通过现有 snapshot constructors 构造零降水、零 active surface-water 和零 erodibility 条件；不复制 stream-power 方程。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Expected: 编译失败；现有入口只有 PTC absolute fixed-point solve。

- [ ] **Step 3: 建立只含表面过程的完整时长推进**

固定接口：

```rust
pub(in crate::generators::natural) struct SurfaceProcessInputs<'a> {
    pub surface: &'a SphericalSurfaceSnapshot,
    pub substrate: &'a GeologicSubstrateSnapshot,
    pub climate: &'a GlobalCirculationSnapshot,
    pub formation_spec: &'a HydroErosionSpec,
    pub water_inventory_m3: f64,
}

pub(in crate::generators::natural) fn advance_surface_processes(
    state: &mut FormationState,
    inputs: SurfaceProcessInputs<'_>,
    duration_years: f64,
    cancellation: &BuildCancellation,
) -> Result<SurfaceAdvanceReport, SurfaceFormationGenerationError>;

pub(in crate::generators::natural) fn finalize_surface_formation(
    state: FormationState,
    final_inputs: SurfaceFormationInputs<'_>,
    evolution_report: FormationEvolutionReport,
    cancellation: &BuildCancellation,
) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError>;
```

实现以 `remaining_years` 循环；每次取 `min(remaining_years, maximum_stable_step_years, maximum_elevation_domain_step_years)`，在 clone 候选上跑完整 hydrology→stream power→hillslope→coast→sediment→Airy→water 验证，成功才扣减 remaining。`ImplicitStreamPowerSolver` 拆成“当前 P2 rate 诊断”和“fluvial height solve”；该函数不再应用 `tectonic_displacement_m`，因为 P3 delta 已由协调器加入。

删除 `solve_geomorphic`、`generate_with_climate_solve_limit`、`EquilibriumOutsideElevationDomain` 和 `NotConverged` 生产路径。把 `FormationSolveReport` 替换为：

```rust
pub struct FormationEvolutionReport {
    accepted_tectonic_steps: u16,
    accepted_surface_substeps: u32,
    climate_solve_count: u16,
    current_rates: FormationResiduals,
    dense_state_bytes: u64,
}
```

`NaturalSurfaceFormationSnapshot` 中原 `solve_report` 字段同步改名为 `evolution_report: FormationEvolutionReport`，并只读公开 `evolution_report()`；不在协调器 output 或 bundle 顶层再存一份。`current_rates` 是诊断，不要求 `normalized_max() <= 1`；schema 和 `surface_formation_model_fingerprint` 同步升版。内部 P4 仍可使用既有快平衡求解，不把 PTC 扩回外层地貌。

- [ ] **Step 4: 运行 GREEN 与数值稳定族**

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Run: `cargo test --release --test formation_stream_power --test surface_formation_contracts`

Expected: 全部 PASS；有限时间测试允许非零 `dh/dt`，只要求归因、守恒、时长完整和数值稳定。

- [ ] **Step 5: 提交**

```bash
git add -p -- src/generators/natural/surface_formation/generation.rs src/generators/natural/surface_formation/stream_power.rs src/generators/natural/surface_formation/mod.rs src/world/natural/surface_formation.rs tests/formation_stream_power.rs tests/surface_formation_contracts.rs
git diff --cached --check
git commit -m "Advance P5 over finite physical time" -m "Retire the global absolute-landscape root and integrate surface processes without double-counting tectonic motion."
```

---

### Task 9: 实现领域因果协调器

**Files:**
- Create: `src/generators/natural/causal_formation.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/world/natural/formation.rs`（只增加 `#[cfg(test)]` schedule constructor）
- Test: `src/generators/natural/causal_formation.rs`

**Interfaces:**
- Consumes: Tasks 1–8 的 timeline、P2 stepper、P3 投影、P4 solver、`FormationState` 和 `advance_surface_processes`。
- Produces: crate-private `CausalNaturalFormationGenerator::generate_working(...) -> CausalFormationOutput`；output 只包含最终子域快照，以及只供 bundle factory 评估最终 P4 的私有 forcing，不包含历史或重复演化报告。

- [ ] **Step 1: 写顺序、原子接受、确定性 RED**

在 `causal_formation.rs` 的 `#[cfg(test)]` 模块增加：

```rust
#[test]
fn two_causal_steps_apply_geology_without_erasing_surface_history() {
    let (inputs, mut rng) = causal_inputs_for_test_steps(RootSeed::new(42), 2);
    let output = CausalNaturalFormationGenerator::generate_working(
        inputs,
        &mut rng,
        &BuildCancellation::new(),
    ).unwrap();
    assert_eq!(output.surface.evolution_report().accepted_tectonic_steps(), 2);
    assert!(output.surface.terrain_fields().elevation_components()
        .equilibrium_adjustment_m().iter().any(|value| *value != 0.0));
    assert_eq!(
        output.surface.terrain_fields().elevation_components().primary_relief_m(),
        output.primary_relief.elevation_m(),
    );
}

#[test]
fn a_rejected_surface_candidate_does_not_advance_p2_or_p5() {
    let fixture = causal_failure_fixture();
    let first = fixture.coordinator.propose_next().unwrap_err();
    let second = fixture.coordinator.propose_next().unwrap_err();
    assert_eq!(first.to_string(), second.to_string());
    assert_eq!(fixture.coordinator.accepted_step_count(), 0);
    assert_eq!(fixture.coordinator.formation_state_bits(), fixture.initial_state_bits);
}
```

`causal_inputs_for_test_steps` 只在 `#[cfg(test)]` 通过 `ResolvedFormationTimeline::test_schedule(step_count, step_duration_kyr)` 构造 locked timeline 的测试前缀/半步；production `generate_working` 永远传 `formation.timeline()`。该 constructor 不导出到非测试 library、serde、artifact 或 UI。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --lib generators::natural::causal_formation::tests`

Expected: 编译失败，协调器不存在。

- [ ] **Step 3: 实现唯一宏步顺序**

```rust
pub(in crate::generators::natural) struct CausalNaturalFormationInputs<'a> {
    pub profile_bundle: &'a ProfileSurfaceBundle,
    pub quality_profile: NaturalQualityProfile,
    pub tectonic_spec: &'a TectonicSpec,
    pub formation: &'a ResolvedWorldFormation,
    pub geologic_spec: &'a GeologicSpec,
    pub relief_spec: &'a ReliefSpec,
    pub climate_domain: &'a ClimateWorkDomainSnapshot,
    pub climate_spec: &'a ClimateSpec,
    pub surface_spec: &'a HydroErosionSpec,
}

pub(in crate::generators::natural) struct CausalFormationOutput {
    pub evolved_tectonics: EvolvedTectonicSnapshot,
    pub geologic_substrate: GeologicSubstrateSnapshot,
    pub primary_relief: PrimaryReliefSnapshot,
    pub surface: NaturalSurfaceFormationSnapshot,
    pub final_climate_forcing: GlobalClimateForcing,
}
```

`final_climate_forcing` 只供 bundle factory 对最终 P4 做质量评估，既不进入 world bundle，也不序列化。

每一宏步严格执行：P2 `propose_next` → 当前 substrate/P3 → `apply_geologic_delta` → 在当前完整 terrain 上重建 P4 forcing 并求一次 P4 → P5 以现有稳定子步消费完整 `step_duration_myr() * 1_000_000` 年 → solid/sediment/water/component 校验 → 同时 accept P2 candidate 与 P5 candidate。气候每个构造宏步求一次，不引入 cadence 常量；P2/P3 随机形态都从协调器一次 capture 的 `LabeledSubstreams` 固定标签派生，重复调用不消耗相邻模块随机流。

- [ ] **Step 4: 运行 GREEN 的短前缀解析/确定性测试和步长实测**

Run: `cargo test --release --lib generators::natural::causal_formation::tests`

Expected: 全部 PASS；两步 fixture 的 accepted 顺序、无重置和拒绝原子性成立。

同一模块增加 ignored `measure_one_macro_step_doubling`：在缩小的生产球面 fixture 上比较一个 `2 Myr` 步与两个 `1 Myr` 半步的 exact elevation、五来源 sediment mass 和 water volume，写出 `target/natural-quality/causal-formation/step-doubling.json` 及 blake3；它只测量误差，不新增通过阈值。

Run: `cargo test --release --lib measure_one_macro_step_doubling -- --ignored --nocapture`

Expected: probe PASS 并生成非空 JSON；文件明确记录两条路径都消费 `2 Myr`，不进入 artifact。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/causal_formation.rs src/generators/natural/mod.rs src/world/natural/formation.rs
git commit -m "Coordinate causal natural formation" -m "Couple P2 through P5 over the resolved timeline with atomic candidate acceptance."
```

---

### Task 10: 定义最终当前态 bundle 与单输出生产 stage

**Files:**
- Create: `src/world/natural/formation_bundle.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `src/generators/natural/causal_formation_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/quality/global_circulation.rs`
- Modify: `src/generators/natural/quality/mod.rs`
- Create: `tests/support/causal_formation.rs`
- Modify: `tests/support/mod.rs`
- Test: `tests/geologic_pipeline_contracts.rs`
- Test: `tests/causal_formation_generation.rs`
- Test: `tests/causal_formation_performance.rs`
- Modify: `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`（只追加实测修订）

**Interfaces:**
- Consumes: `CausalFormationOutput` 与现有 P2/P3/P4/P5 `NaturalQualityReport` evaluator。
- Produces: `NaturalFormationBundle`、`NaturalFormationBundleArtifact`、`CausalNaturalFormationStage`、`causal_natural_formation_graph()`。

- [ ] **Step 1: 写 schema、只读访问与 graph RED**

```rust
#[test]
fn production_graph_publishes_one_atomic_current_formation_bundle() {
    assert_eq!(NaturalFormationBundleArtifact::KEY.as_str(), "world.natural-formation-bundle");
    let graph = causal_natural_formation_graph().unwrap();
    assert_eq!(
        graph.stage_ids(),
        vec!["natural.climate-work-domain", "natural.causal-formation"],
    );
}

#[test]
fn bundle_contains_final_domains_but_no_history() {
    let bundle = build_causal_bundle(RootSeed::new(42));
    assert_eq!(bundle.tectonics().surface_ref(), bundle.surface_ref());
    assert_eq!(bundle.substrate().surface_ref(), bundle.surface_ref());
    assert_eq!(bundle.primary_relief().surface_ref(), bundle.surface_ref());
    assert_eq!(bundle.surface_formation().surface_ref(), bundle.surface_ref());
    let json = serde_json::to_value(bundle).unwrap();
    for forbidden in ["history", "checkpoints", "pseudo_time", "rejected_steps"] {
        assert!(json.get(forbidden).is_none());
    }
}
```

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --test geologic_pipeline_contracts`

Expected: 编译失败，bundle/stage/graph 尚不存在。

- [ ] **Step 3: 实现 world bundle 与原子 artifact**

```rust
pub const NATURAL_FORMATION_BUNDLE_SCHEMA_V1: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalFormationBundle {
    schema_version: u16,
    surface_ref: SurfaceRef,
    timeline: ResolvedFormationTimeline,
    tectonics: EvolvedTectonicSnapshot,
    substrate: GeologicSubstrateSnapshot,
    primary_relief: PrimaryReliefSnapshot,
    surface_formation: NaturalSurfaceFormationSnapshot,
    tectonic_quality: NaturalQualityReport,
    primary_relief_quality: NaturalQualityReport,
    climate_quality: NaturalQualityReport,
    surface_quality: NaturalQualityReport,
}
```

另定义字段完全相同的 private `NaturalFormationBundleWire: Deserialize`，手写 `Deserialize for NaturalFormationBundle`，反序列化只能转调 `NaturalFormationBundle::new(...)` 并执行 `validate()`；不得 derive 一个绕过 contextual validation 的 artifact。

`validate()` 逐域调用既有 validator，并校验共同 `SurfaceRef`、timeline，以及最终 P4 必须等于 `surface_formation.formation_climate()`；每份 quality report 继续由自己的既有 validator 校验 profile、fingerprint，并在该报告 schema 已支持时校验 subject/surface 绑定，不给报告新增第二套通用身份字段。P4 quality evaluator 新增有两个真实消费者的窄 enum：

```rust
pub(crate) enum ClimateQualityTerrain<'a> {
    Primary(&'a PrimaryReliefSnapshot),
    Formation(&'a FormationTerrainFields),
}
```

原 P4 evaluator wrapper 走 `Primary`，bundle factory 对最终 P4 走 `Formation`；两者共享同一 metric 实现，不复制质量方程。artifact 只实现 `Serialize`，crate-private `new`，唯一 public factory 是 `NaturalFormationBundleArtifact::generate(inputs, rng, cancellation)`。

`CausalNaturalFormationStageInputs` 的 dependencies 固定为 profile、resolved tectonic/world formation/geologic/climate/hydro、relief spec、surface、climate work domain；stage version 从 1 开始，id 为 `natural.causal-formation`。把仍有真实外部消费者的 `NaturalQualityProfileArtifact` 与 `ReliefSpecArtifact` 定义迁入该文件，保持 artifact key 不变；通用 `Stage` 不修改。

stage error code 固定为：`causal-formation.invalid-input`、`causal-formation.numerical-stability`、`causal-formation.solid-budget`、`causal-formation.sediment-budget`、`causal-formation.water-budget`、`causal-formation.climate-not-converged`、`causal-formation.elevation-out-of-range`、`causal-formation.resource-limit` 与既有 `engine.cancelled`。错误只携带诊断值，不携带可发布的中间 bundle。

- [ ] **Step 4: 运行 GREEN、serde 伪造与原子失败测试**

Run: `cargo test --release --test geologic_pipeline_contracts --test causal_formation_generation`

Expected: 全部 PASS；取消、数值越界、预算失败都没有 `NaturalFormationBundleArtifact`，错误分别映射到 stable diagnostic code。

- [ ] **Step 5: 运行完整 locked timeline 实测，不先写新 cadence/阈值**

`tests/causal_formation_performance.rs` 使用既有 P5 门禁事实：Draft/Standard/High 分别 `15/90/300 s`，High retained dense state `1 GiB`，取消 `250 ms`；测试记录每个宏步的 P2、P3、P4、P5 wall time、P5 substeps、peak RSS 和最终 fingerprint。Task 9 的 `step-doubling.json` 与本次完整时间线证据一起进入决策记录，但不自行钉新误差阈值。

Run: `cargo test --release --test causal_formation_performance -- --ignored --nocapture`

Expected: 生成 `target/natural-quality/causal-formation/performance.json` 与 `step-doubling.json`，且无部分 artifact。

- [ ] **Step 6: 执行不可绕过的决策门并提交**

把机器、编译档、seed、profile、逐阶段耗时、峰值内存、取消延迟、步长误差和文件 blake3 写入设计规格“实测修订”。

- 若完整直接耦合满足既有 profile 时间/内存/取消门禁，且现有数值/守恒门禁全过：提交本任务并继续 Task 11。
- 若任一门禁失败：提交 bundle/协调器短前缀测试与实测证据，停止执行 Task 11–13；向用户提交“多速率策略规格修订”决策，不得减少 `128 × 2 Myr`、跳过 P4、放宽误差或发布未完成状态。

```bash
git add src/world/natural/formation_bundle.rs src/world/natural/mod.rs src/generators/natural/causal_formation_stage.rs src/generators/natural/mod.rs src/generators/natural/quality/global_circulation.rs src/generators/natural/quality/mod.rs tests/support/causal_formation.rs tests/support/mod.rs tests/geologic_pipeline_contracts.rs tests/causal_formation_generation.rs tests/causal_formation_performance.rs docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md
git commit -m "Publish one current formation bundle" -m "Make final tectonics, terrain, climate, surface state, and evidence an inseparable stage product."
```

---

### Task 11: 把应用与字段文档迁移到 bundle

**Files:**
- Modify: `src/app/spherical_presentation.rs`
- Modify: `src/app/spherical_formation_display.rs`
- Modify: `src/app.rs`
- Modify: `src/world/natural/fields.rs`
- Modify: `src/ui/field/localization.rs`
- Test: `tests/natural_field_registry_spherical.rs`
- Test: `tests/field_display_integration.rs`
- Test: `tests/spherical_presentation_integration.rs`
- Test: `src/app.rs`（`natural_app_tests`）

**Interfaces:**
- Consumes: `NaturalFormationBundleArtifact` 与现有 field ids/registry/localization。
- Produces: `SphericalFormationFieldDocument` 只持有 surface、author inputs 和 bundle；现有字段 id、面板操作与 renderer 接口保持不变。

- [ ] **Step 1: 写 bundle-only 文档 RED**

```rust
#[test]
fn formation_document_reads_every_scientific_payload_from_the_bundle() {
    let outcome = causal_formation_outcome(RootSeed::new(42));
    let document = SphericalFormationFieldDocument::from_build_outcome(&outcome).unwrap();
    let bundle = outcome.artifacts.get::<NaturalFormationBundleArtifact>().unwrap();
    assert_eq!(document.formation_snapshot(), bundle.bundle().surface_formation());
    assert_eq!(document.substrate(), bundle.bundle().substrate());
    assert_eq!(document.evolved_compatibility(), bundle.bundle().tectonics().compatibility());
}
```

在 app 单元测试断言 build outcome 缺少旧 `NaturalSurfaceFormationArtifact` 仍可成功安装；缺少 bundle 必须失败且不提交 world/cache/GPU。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --test field_display_integration formation_document_reads_`

Expected: FAIL，document 仍逐个读取旧 P2/P3/P4/P5 artifacts。

- [ ] **Step 3: 迁移装配且不复制 UI 文案**

`build_spherical_formation_candidate_with_lineage` 改用 `causal_natural_formation_graph()`。`SphericalFormationFieldDocument::build` 固定为：

```rust
fn build(
    provenance: BuildProvenance,
    surface: Arc<SphericalSurfaceArtifact>,
    resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
    relief_spec: Arc<ReliefSpecArtifact>,
    formation: Arc<NaturalFormationBundleArtifact>,
    report: &BuildReport,
) -> Result<Self, SphericalFormationDisplayError>;
```

field payload 从 `formation.bundle()` 的最终子快照借用；不新增硬编码 label/range/palette。原有 current elevation、primary relief、surface adjustment、P2 rates、P5 rates、hydrology、sediment 和最终 P4 字段全部保留。形成 timeline 不进入面板，不增加年龄旋钮。

`SphericalFormationFieldDocument::initial_circulation` 当前无消费者且新 bundle 不发布初始 P4 checkpoint，按 YAGNI 删除；需要气候的调用点只读最终 `formation_climate()`。

- [ ] **Step 4: 运行 GREEN 与 UI 装配回归**

Run: `cargo test --release --test natural_field_registry_spherical --test field_display_integration --test spherical_presentation_integration`

Run: `cargo test --release --lib natural_app_tests`

Expected: 全部 PASS；成功只发布 bundle 当前态，失败/取消保持上一个完整世界和 cache。

- [ ] **Step 5: 提交**

```bash
git add -p -- src/app/spherical_presentation.rs src/app/spherical_formation_display.rs src/app.rs src/world/natural/fields.rs src/ui/field/localization.rs tests/natural_field_registry_spherical.rs tests/field_display_integration.rs tests/spherical_presentation_integration.rs
git diff --cached --check
git commit -m "Render formation state from the atomic bundle" -m "Move application and field payload assembly to the single final current-state authority without adding UI knobs."
```

---

### Task 12: 删除失去生产消费者的旧 stage、稳态 API 与兼容适配器

**Files:**
- Delete: `src/generators/natural/surface_formation_stage.rs`
- Delete: `src/generators/natural/evolved_tectonic_stage.rs`
- Delete: `src/generators/natural/primary_relief_stage.rs`
- Modify: `src/generators/natural/global_circulation_stage.rs`（只保留 `ClimateWorkDomainArtifact`/`ClimateWorkDomainStage`）
- Modify: `src/generators/natural/mod.rs`
- Delete: `tests/evolved_tectonic_stage.rs`
- Delete: `tests/primary_relief_stage.rs`
- Delete: `tests/global_circulation_stage.rs`
- Delete: `tests/surface_formation_stage.rs`
- Modify: `tests/evolved_tectonic_evidence.rs`
- Modify: `tests/evolved_tectonic_performance.rs`
- Modify: `tests/evolved_tectonic_quality.rs`
- Modify: `tests/global_circulation_atlas.rs`
- Modify: `tests/global_circulation_evidence.rs`
- Modify: `tests/global_circulation_performance.rs`
- Modify: `tests/global_circulation_quality.rs`
- Modify: `tests/primary_relief_evidence.rs`
- Modify: `tests/primary_relief_performance.rs`
- Modify: `tests/surface_formation_atlas.rs`
- Modify: `tests/surface_formation_evidence.rs`
- Modify: `tests/surface_formation_performance.rs`
- Modify: `tests/surface_formation_quality.rs`
- Test: `tests/geologic_pipeline_contracts.rs`
- Test: `tests/causal_formation_generation.rs`

**Interfaces:**
- Consumes: Task 10/11 已无旧 stage/artifact 生产消费者的事实。
- Produces: 唯一 formation production graph；算法 generator/纯 evaluator 保留，通用 engine 零修改。

- [ ] **Step 1: 写旧 artifact 不可发布 RED**

把 stage contract 写入 `tests/causal_formation_generation.rs`，只接受新 stage 集合，并删除旧 artifact imports：

```rust
#[test]
fn formation_graph_has_no_independent_science_publication_stages() {
    assert_eq!(
        causal_natural_formation_graph().unwrap().stage_ids(),
        vec!["natural.climate-work-domain", "natural.causal-formation"],
    );
}
```

Run: `cargo test --release --test causal_formation_generation formation_graph_has_no_`

Expected: FAIL，旧 `surface_formation_graph()`/stage assertions 仍在测试和 exports 中。

- [ ] **Step 2: 审计真实消费者并形成删除清单**

Run: `rg -n "(SurfaceFormationStage|NaturalSurfaceFormationArtifact|EvolvedTectonicStage|PrimaryReliefStage|GlobalCirculationStage|surface_formation_graph|global_circulation_graph)" src -g '*.rs'`

Expected: 只剩四个待删除 stage 文件、`global_circulation_stage.rs` 中待删的 P4 artifact/stage 部分及 module re-export；legacy foundation 使用不同代际类型。测试引用不视为保留生产 adapter 的理由。

- [ ] **Step 3: 删除无消费者代码并刷新缓存身份测试**

删除三份完整旧 stage 文件和 `global_circulation_stage.rs` 中的 `GlobalCirculationArtifact`/`GlobalCirculationStage`/`global_circulation_graph`，只保留 work-domain artifact/stage。`EvolvedTectonicArtifact`、`GeologicSubstrateArtifact`、`PrimaryReliefArtifact`、`GlobalCirculationArtifact`、`NaturalSurfaceFormationArtifact` 随所属旧包装删除；Task 10 的 bundle factory 已直接运行对应 evaluator。纯 snapshot/generator/quality evaluator 保留。

删除四个只验证已退役 stage 的集成测试；其余上列 evidence/quality/performance 测试改为直接调用 generator/evaluator 或 `causal_natural_formation_graph()`，不得为测试保留 production adapter。

更新 graph cache 测试：同 seed、同全部 resolved inputs 第二次必须命中新 bundle；timeline/schema/stage version 任一变化必须 miss，不双写新旧科学 artifact。

- [ ] **Step 4: 运行 GREEN 与 engine 不变式**

Run: `cargo test --release --test causal_formation_generation --test stage_graph --test engine_execution --test diagnostics_and_provenance`

Run: `git diff -- src/engine/stage.rs src/engine/graph.rs`

Expected: 测试 PASS；最后一条无输出，证明未修改通用 Stage/graph 接口。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/evolved_tectonic_stage.rs src/generators/natural/primary_relief_stage.rs src/generators/natural/global_circulation_stage.rs src/generators/natural/surface_formation_stage.rs src/generators/natural/mod.rs tests/evolved_tectonic_stage.rs tests/primary_relief_stage.rs tests/global_circulation_stage.rs tests/surface_formation_stage.rs tests/evolved_tectonic_evidence.rs tests/evolved_tectonic_performance.rs tests/evolved_tectonic_quality.rs tests/global_circulation_atlas.rs tests/global_circulation_evidence.rs tests/global_circulation_performance.rs tests/global_circulation_quality.rs tests/primary_relief_evidence.rs tests/primary_relief_performance.rs tests/surface_formation_atlas.rs tests/surface_formation_evidence.rs tests/surface_formation_performance.rs tests/surface_formation_quality.rs tests/geologic_pipeline_contracts.rs tests/causal_formation_generation.rs tests/stage_graph.rs tests/engine_execution.rs tests/diagnostics_and_provenance.rs
git diff --cached --check
git commit -m "Retire split formation publication stages" -m "Delete superseded adapters after the causal bundle becomes the only production science artifact."
```

提交前必须从 staged diff 中撤出任何与本任务无关的既有 dirty hunk；不得用 `git add -A`。

---

### Task 13: 完成证据、全门禁与用户 UI 验收

**Files:**
- Modify: `src/generators/natural/causal_formation.rs`（只增加 `#[cfg(test)]` 完整链消融）
- Modify: `tests/causal_formation_generation.rs`
- Modify: `tests/causal_formation_performance.rs`
- Modify: `tests/surface_formation_atlas.rs`
- Modify: `tests/surface_formation_evidence.rs`
- Modify: `tests/surface_formation_quality.rs`
- Modify: `tests/terrain_audit_probe.rs`
- Modify: `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`（最终实测修订）
- Create: `docs/superpowers/plans/2026-08-25-geologic-pipeline-contract-restoration-completion.md`

**Interfaces:**
- Consumes: 唯一 bundle graph、最终 UI 文档和所有领域报告。
- Produces: 多 seed 科学证据、性能/内存/取消证据、完成记录和用户可执行验收步骤。

- [ ] **Step 1: 增加完整兼容消融与多 seed 守门**

```rust
#[test]
fn compatibility_elevation_ablation_preserves_p3_p4_p5_authority() {
    for seed in [3_u64, 7, 42] {
        let original = generate_with_test_compatibility_mutation(seed, false);
        let mutated = generate_with_test_compatibility_mutation(seed, true);
        assert_eq!(mutated.substrate(), original.substrate(), "seed {seed}");
        assert_eq!(mutated.primary_relief(), original.primary_relief(), "seed {seed}");
        assert_eq!(
            mutated.surface_formation().formation_climate(),
            original.surface_formation().formation_climate(),
            "seed {seed}",
        );
        assert_eq!(mutated.surface_formation(), original.surface_formation(), "seed {seed}");
    }
}
```

该测试放在 `causal_formation.rs` 的 private test module；`generate_with_test_compatibility_mutation` 在每个 P2 candidate 生成后、任何 P3/P4/P5 消费前，只改 compatibility elevation 数组。helper 与 mutation enum 均受 `#[cfg(test)]` 约束，不增加 production 注入点。

补全 seed `3/7/42` Draft，现有 atlas/evidence corpus，至少一个 Standard release build；断言非零当前 `dh/dt` 合法、所有位移可归因、solid/sediment/water/component budget 闭合。

- [ ] **Step 2: 运行 Release 科学/呈现套件**

Run: `cargo test --release --test geologic_pipeline_contracts --test causal_formation_generation --test surface_formation_contracts --test surface_formation_generation --test surface_formation_quality --test surface_formation_evidence --test surface_formation_atlas`

Run: `cargo test --release --lib compatibility_elevation_ablation_preserves_p3_p4_p5_authority`

Run: `cargo test --release --test natural_field_registry_spherical --test field_display_integration --test spherical_presentation_integration`

Expected: 全部 PASS；atlas/golden 只因已批准的科学状态变化更新，并在完成记录列出旧/新 hash 与因果说明。

- [ ] **Step 3: 运行性能、内存和取消门禁**

Run: `cargo test --release --test causal_formation_performance -- --ignored --nocapture`

Expected: 已在 Task 10 获准的完整时间线策略满足规格修订中记录的 gate；取消 latency、peak RSS、完整时长和 cache identity 全部 PASS。

- [ ] **Step 4: 运行项目级静态门禁**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo check --target wasm32-unknown-unknown --all-features --lib`

Expected: 三条命令退出码均为 0。

- [ ] **Step 5: 运行完整调试回归**

Run: `cargo test --workspace --all-targets --all-features`

Expected: 全部 PASS；按项目说明预留约 40 分钟，保留最终摘要中的 passed/failed/ignored 数量。

- [ ] **Step 6: 写完成记录并提交**

完成记录必须包含：task/commit 对照、所有命令与退出码、性能 JSON/hash、兼容消融 hash、schema/stage version 变化、删除清单、已知开放问题，以及以下用户验收步骤：

1. 启动应用：`cargo run --release`。
2. 在自然世界流程选择 Draft，seed 输入 `42`，执行生成。
3. 在字段面板依次查看“当前高程”“基础地形”“地表调整”“构造位移率”“河流侵蚀率”“沉积厚度”和最终气候降水；预期所有字段来自同一次 bundle build，切换字段不重新求解科学状态。
4. 连续生成 seed `3`、`7`、`42`；预期不再出现 `9000.000260834617` 的精度误报，也不再因 `CellId(19366)` 没有绝对稳态根而失败。
5. 在生成中点击取消；预期保留上一个完整世界，不显示部分时间步、不提交工作 cache/GPU 状态。
6. 用户确认地貌、字段切换、取消与错误文案后，才把功能标记为已交付。

```bash
git add src/generators/natural/causal_formation.rs tests/causal_formation_generation.rs tests/causal_formation_performance.rs tests/surface_formation_atlas.rs tests/surface_formation_evidence.rs tests/surface_formation_quality.rs tests/terrain_audit_probe.rs docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md docs/superpowers/plans/2026-08-25-geologic-pipeline-contract-restoration-completion.md
git commit -m "Verify causal formation end to end" -m "Record scientific, performance, cache, UI, and full regression evidence for the restored pipeline."
```

---

## 每项承重技术的出处

- P2 有限构造演化、`2 Myr × 128` reference schedule 与只保留 current/next 双缓冲：Cortial et al. (2019), DOI `10.1111/cgf.13628`；具体 schedule 作为 resolved model identity，不宣称世界真实年龄。
- 当前态由私有有限历史积分得到、产品只发布终点：Paik & Kim (2021), *Simulating the evolution of the topography-climate coupled system*；本计划继承多速率因果顺序，不复制未经实测的 cadence。
- 河流侵蚀的隐式下游栈：Braun & Willett (2013), DOI `10.1016/j.geomorph.2012.10.008`；抬升—侵蚀响应背景：Whipple & Tucker (1999), DOI `10.1029/1999JB900120`。
- 河流侵蚀—输运—沉积连续方程与解析递推：Davy & Lague (2009), DOI `10.1029/2008JF001146`；Barnhart et al. (2019), DOI `10.5194/gmd-12-1267-2019`；pyBadlands/SPACE 的守恒库存实践见 Salles (2018) 与 Shobe et al. (2017)。
- 非线性坡面稳定子步与离散最大值：Eymard, Gallouët & Herbin (2000), DOI `10.1016/S1570-8659(00)07005-8`；Landlab `TaylorNonLinearDiffuser` commit `8f59a66279cefa288b146735a939d95e9a6730c2`。
- 海岸交换、沉积库存和 generalized Exner 质量账本：Paola & Voller (2005), DOI `10.1029/2004JF000274`；具体项目适用边界按上位规格保留。
- 局部 Airy 加载/卸载：Turcotte & Schubert (2014), *Geodynamics*, 3rd ed., ch. 5；响应直接记入完整 `f64` 状态，不按 artifact 边界裁剪。
- PTC 只用于有根的内部快平衡，不再作为全地貌绝对稳态：Kelley & Keyes (1998), DOI `10.1137/S0036142996304796`；PETSc 只作数值实现类比，不提供 Sekai 科学容差。
- 浮点身份、先定义精确状态再做发布舍入：Goldberg (1991), DOI `10.1145/103162.103163`；稳定误差解释参考 Higham (2002), *Accuracy and Stability of Numerical Algorithms*, 2nd ed.
- 原子候选/提交、确定性重放与 artifact 内容寻址沿用本项目现有 engine 工业实践；不修改通用 Stage，不引入第二份科学事实源。
