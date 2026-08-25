# 地质管线契约恢复与因果当前态 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 恢复 P2→P3→P4→P5 的单一过程所有权，以私有有限物理时间演化生成一个原子当前态 bundle，消除兼容高程污染、绝对地貌稳态误用和跨步 `f32` 状态回流，同时保持现有 UI 字段能力。

**Architecture:** `world` 提供唯一 resolved formation timeline、借用型权威构造视图、P4/P5 sibling 当前态 bundle schema 与数值/守恒契约；`generators/natural` 内的领域协调器先完成 P2 timeline 和最终 P3 投影，再以 start P4 → 一次完整 P5 → endpoint P4/零时间终点诊断执行生产 Lie-style sequential split，验证最终 forcing 后原子发布，不改通用 `engine::Stage`。生产图最终只发布一个 `NaturalFormationBundleArtifact`，UI 从该 bundle 的最终 sibling 快照读取字段，不发布历史、伪时间或求解中间态。

**Tech Stack:** Rust 2024、serde、thiserror、blake3、现有 Stage/Artifact/BuildCancellation、现有 P2–P5 生产算子与 egui/eframe 呈现层；不新增第三方依赖。

**Spec:** `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`

**2026-08-25 最终态/效率显式修订：** 本计划已按规格 §0 删除每宏步三次 P4、
`2/1/0.5 Myr` 三层细化发布门和人工误差包络。生产只实现单次 Lie-style 分裂与
endpoint closure；原三次 P4 路径仅保留为一个 `Standard`/seed `42` 离线参考
探针。P4/P5 sibling 所有权和最终 forcing 身份要求不变。

## Global Constraints

- 只发布最终当前态；P2/P3/P4/P5 的逐步状态、候选、拒绝步、伪时间和重试日程不得进入 schema、artifact、缓存恢复契约或 UI。
- P2 只拥有固体地球演化、地壳物质与构造成因；P3 只做同一时刻固体状态到基础地形的投影；P4 只解当前边界上的快平衡；P5 只拥有地表、水圈、沉积和地表载荷 Airy 响应。
- 一次完整生产构建必须执行：P2 完成自己的 resolved timeline → 最终 P3 投影 → start P4 → P5 完整消费 `SURFACE_FORMATION_HORIZON_YEARS` → endpoint P4 → 零时间终点诊断；不得在每个 P2 宏步调用 P4，也不得把 P2 的 `128 × 2 Myr` 时域误传给 P5。终点 P4 forcing 必须由最终完整地形及其 `SurfaceWaterGeometry` 重建。
- P4 climate 与 P5 surface formation 在 bundle 中是 sibling；P5 snapshot 不嵌入 climate，只以 `formation_climate_checkpoint_fingerprint` 绑定最终 P4。
- 生产 P3/P4/P5 不得读取 `compatibility.tectonic_elevation_m`；只改这一兼容字段不得改变任何权威下游结果。
- 跨步累计的高程组成和沉积质量库存保留 `f64`；`f32` 只在经过完整 `f64` 校验后生成 wire/GPU 快照，且不得回流成为下一步状态。
- 不得 clamp 科学状态，不得扩大 `ELEVATION_MIN_M`/`ELEVATION_MAX_M`，不得按目标地貌做重映射、经验修形或特殊格元分支。
- 不增加世界年龄、最高程、耦合 cadence、收敛容差或显示范围旋钮；形成时间线是 resolved 输入身份，不是本轮 UI 配置。
- 后期“统一噪声”只表示同一物理算法的某些参数可由常数推广为有出处的空间分布。当前计划只记录 §0/规格 §2.6 的输入边界，不增加分布 schema、用户旋钮、中央噪声 stage 或无人消费的泛型抽象。
- 不增加通用循环 stage、多输出 stage、反馈 trait 或无人消费的适配器；领域协调发生在 `src/generators/natural/`。
- 每个任务严格 RED→GREEN→提交；无法产生 RED 的纯删除/文档步骤使用变异探针证明守门能力，并在提交正文如实说明。
- 当前工作树已有未提交的 P5 R4、`f64`、UI 发布事务和测试改动。不得 reset、checkout、删除或混入无关提交；修改重叠文件时使用 `git add -p -- <paths>`，提交前必须检查 `git diff --cached --name-only` 与 `git diff --cached`。
- 迭代期 P5/全链目标测试使用 `--release`；最终必须运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo check --target wasm32-unknown-unknown --all-features --lib` 和完整调试回归。
- 算法任务在 UI 接入并由用户亲自验证前不得宣称交付完成。
- Task 10 的公开 artifact/UI 迁移门只检查终点身份、硬不变式、既有质量包络和既有性能/取消包络；离线参考原始差值是研发证据，不新增发布阈值。

---

## 文件与职责图

### 新建文件

- `src/world/natural/formation_bundle.rs`：最终当前态 bundle、schema、严格验证和只读子域访问器。
- `src/generators/natural/causal_formation.rs`：P2/P3/P4/P5 私有因果协调器；只编排现有算子，不复制方程。
- `src/generators/natural/causal_formation_stage.rs`：单输出 `NaturalFormationBundleArtifact`、质量报告封装和生产 stage/graph。
- `src/generators/natural/surface_formation/state.rs`：P5 子步间保留的 `f64` 高程组成与沉积质量库存；最终 wire 投影的唯一入口。
- `tests/geologic_pipeline_contracts.rs`：模块所有权、兼容字段消融、bundle 原子性和生产图契约。
- `tests/causal_formation_generation.rs`：有限时间、确定性、守恒、无重置与错误分类集成测试。
- `tests/causal_formation_performance.rs`：Release-only 生产时间线、内存和取消实测；高成本参考只在 Task 9 ignored probe 中运行。
- `tests/support/causal_formation.rs`：跨集成测试共享的生产输入/构建 fixture，不重写算法。

### 重点修改文件

- `src/world/natural/formation.rs`：`ResolvedFormationTimeline` 及其在 `ResolvedWorldFormation` 中的身份。
- `src/world/natural/evolved_tectonics.rs`：不暴露兼容高程的 `AuthoritativeTectonicView<'a>`。
- `src/world/natural/surface_formation.rs`：把稳态报告替换为有限时间演化报告，移除嵌套 P4，保留最终 climate checkpoint 指纹绑定并刷新 schema/模型指纹。
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
EvolvedTectonicGenerator::generate_final()
        │  final EvolvedTectonicSnapshot
        ▼
AuthoritativeTectonicView ──► GeologicSubstrateGenerator
                              PrimaryReliefGenerator
        │
        ▼
FormationState::from_final_p3()
        │
        ▼
start forcing/P4 ─► one complete P5 advance
        │
        ▼
endpoint forcing/P4 ─► terminal P5 diagnostics (zero time advance)
        │
        ▼
validate endpoint identity + budgets; accept atomically
        │
        ▼
NaturalFormationBundleArtifact
        ├── climate: GlobalCirculationSnapshot
        └── surface_formation: NaturalSurfaceFormationSnapshot
                                                │
                                                ▼
                               SphericalFormationFieldDocument/UI
```

### 后期参数分布接缝（本计划不实现）

后期去规则化任务沿用现有 `ResolvedWorldFormation` 输入通道，不给
`CausalNaturalFormationInputs` 增加第二份 noise config。每个真实消费者在自己
的 P2/P3/P5 resolved spec 中声明一个具体参数的分布配方；常数行为就是零离散度
配方。共享实现只复用现有 `morphology::noise`、`FieldRecipe`/`GaborKernel` 与
`LabeledSubstreams`，不创建能改写任意 artifact 的通用 stage/trait。

后期每个具体配方至少需要：参数/方程项/所有者、单位与科学支持域、带出处的
边际分布族及位置/尺度/形状、物理或角相关尺度与频带、确有需要时的各向异性及
方向场、世界球面/物质随体/过程局部采样坐标、因果条件字段、有直接依据时的
联合分布/交叉协方差、版本化子流标签、
必须保持的总体矩或守恒约束，以及 `SurfaceRef`/半径/真实单元面积。工作分辨率
只用于派生可解析频带，不作为 UI 旋钮；分布支持域按构造合法，禁止采样后 clamp。

输出是原算法本次调用所读的私有 `f64` 参数值，不是新的世界字段。配方版本和
标签进入输入/产物指纹，但采样场不进入历史 schema。P2 边界与洋壳条带若未来
去规则化，应优先让已有阻力、裂谷倾向或铺展参数产生有出处的空间异质性，而非
直接给最终边界或年龄数组加噪声。具体参数分布尚无统一直接出处，实施前必须
逐项“先测后钉”；本计划不得预建类型、默认数值或 UI。

## 已完成的 Phase A 基线（本计划不重复改写）

- 原设计事实已在 commit `e0af32d` 冻结并于 2026-08-25 获用户批准；其耦合 cadence 已由规格 §0 的最终态/效率显式修订替代，P4/P5 sibling 所有权继续有效。
- `cargo test --release --lib generators::natural::surface_formation::generation::tests::` 已通过 `3/3`；`cargo test --release --test formation_coast_isostasy` 已通过 `7/7`，锁住完整 `f64` 域内/真实越界两个方向。
- Draft/seed `42` 已复现 `CellId(19366)` 的真实下界失败：完整候选 `-11000.000274626422 m`，唯一非零项是 `0.024603449 - 1.040666938 mm/year` 的构造净沉降；这否决外层绝对高程求根，不授权 clamp。
- 当前生产 `compatibility()` 消费清单：`geologic_substrate.rs` 读取 crust kind/thickness/age；`primary_relief.rs` 读取 plate/crust geometry 且错误读取 compatibility elevation；`quality/primary_relief.rs` 做 P3 证据；`app/spherical_formation_display.rs` 只呈现 plate/crust category；`quality/evolved_tectonics.rs` 与 evolved tests 审计 legacy V3 产品。Task 2/3 迁移前三项；呈现与 P2 legacy 审计可继续使用 compatibility，但不得把高程传回权威科学链。
- R4 在既有 `100 ka` 构造时域上记录九组旧整步/两半步耦合路径耗时
  `13.8–17.9 min`，证明旧求解与互调方式不满足既有产品预算；它不否定 P5
  已冻结的构造时域，也不授权缩短时域。Task 10 重新测量新的单次生产分裂。

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
- Produces: `ResolvedFormationTimeline::sekai_reference()`, `step_count() -> u16`, `step_duration_kyr() -> u32`, `step_duration_myr() -> f64`, `total_duration_myr() -> f64`，以及 `ResolvedWorldFormation::timeline() -> ResolvedFormationTimeline`。

- [ ] **Step 1: 写时间线身份 RED**

在 `tests/world_formation_spec.rs` 增加：

```rust
#[test]
fn resolved_formation_carries_the_sekai_reference_timeline_in_its_identity() {
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
/// Number of finite evolution steps in the user-approved Sekai reference
/// formation horizon. This is a product parameter, not a literature constant
/// or an Earth-age claim.
pub const SEKAI_REFERENCE_FORMATION_STEP_COUNT: u16 = 128;
/// Duration of one reference step, stored as integer kyr for identity-stable serde;
/// the 2 Myr step follows Cortial et al. (2019), DOI 10.1111/cgf.13614.
pub const CORTIAL_FORMATION_STEP_DURATION_KYR: u32 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedFormationTimeline {
    step_count: u16,
    step_duration_kyr: u32,
}

impl ResolvedFormationTimeline {
    pub const fn sekai_reference() -> Self {
        Self {
            step_count: SEKAI_REFERENCE_FORMATION_STEP_COUNT,
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
        if self != Self::sekai_reference() {
            return Err(WorldFormationSpecError::UnsupportedTimeline {
                step_count: self.step_count,
                step_duration_kyr: self.step_duration_kyr,
            });
        }
        Ok(())
    }
}
```

把 `timeline` 嵌入 `ResolvedWorldFormation` 及 wire，`new` 固定写入 `sekai_reference()`，反序列化后同时校验。删除 runner 私有 `EVOLUTION_STEP_COUNT`/`EVOLUTION_DELTA_MYR`，两个 V4/V5 循环都从 `formation.timeline()` 读取；`generate_evolved_spherical` 和 runner 的 formation 参数改为 `&ResolvedWorldFormation`，只在 recipe 选择处调用 `.resolved()`。

- [ ] **Step 4: 运行 GREEN 与 P2 等价回归**

Run: `cargo test --test world_formation_spec`

Run: `cargo test --release --test evolved_tectonic_generation`

Expected: 全部 PASS；已有固定 seed 的 P2 snapshot/fingerprint 断言不变，只有包含 `ResolvedWorldFormationArtifact` 的 stage 输入身份刷新。

- [ ] **Step 5: 提交**

```bash
git add src/world/natural/formation.rs src/world/natural/mod.rs src/generators/natural/spherical_tectonics/runner.rs src/generators/natural/spherical_tectonics/publication.rs tests/world_formation_spec.rs tests/evolved_tectonic_generation.rs
git commit -m "Move formation timing into resolved world state" -m "Keep Sekai's authored horizon and Cortial's sourced step duration distinct inside validated input identity."
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

现有各物理分量自身有出处的输入域限制保持；任何生成结果超出分量或总高程 artifact 域都 typed fail，不对结果 clamp。另把 `MantleGenerator::generate_spherical_from_streams`、`GeologicSubstrateGenerator::generate_from_streams` 与 `PrimaryReliefGenerator::generate_from_streams` 定为 crate-private；现有 public `generate` 只负责 capture 一次 `LabeledSubstreams` 后转调，协调器则在最终 P3 投影时复用同一组标签身份。

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

### Task 5: 保持 P2 单次生产入口并提供 test-only 参考观察点

**Files:**
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Modify: `src/generators/natural/spherical_tectonics/publication.rs`
- Test: `src/generators/natural/spherical_tectonics/runner.rs`

**Interfaces:**
- Consumes: `ProfileSurfaceBundle`、`ResolvedWorldFormation::timeline()`、P2 process kernels 与 conservative remap。
- Produces: 生产仍只通过既有 one-shot P2 入口返回最终 `EvolvedTectonicSnapshot`；另有一个 `#[cfg(test)]`、crate-private 的 accepted-step observer 只供 Task 9 高成本参考探针使用，不返回历史集合。

- [ ] **Step 1: 写 one-shot/observer 最终产品等价 RED**

```rust
#[test]
fn test_only_step_observer_preserves_the_monolithic_p2_product() {
    for seed in [3_u64, 7, 42] {
        let fixture = evolved_fixture_for_seed(seed);
        let monolithic = generate_evolved(&fixture);
        let (observed_final, accepted_steps) =
            generate_evolved_with_test_step_observer(&fixture);
        assert_eq!(observed_final, monolithic, "seed {seed}");
        assert_eq!(accepted_steps, fixture.formation.timeline().step_count());
    }
}
```

该测试放在 `runner.rs` 的 crate 内部 `#[cfg(test)]` 模块；不得为了让 integration
test 访问观察入口而放宽 production library 可见性。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --lib test_only_step_observer_`

Expected: 编译失败，指出 test-only observer 不存在。

- [ ] **Step 3: 在既有 P2 循环增加零成本生产观察边界**

把现有 runner 的循环体提取为 private
`evolve_control_state_v5_with_observer(..., on_accepted_step)`；生产入口传 no-op
closure，`#[cfg(test)]` helper 才把每个已通过 P2 自身预算/发布校验的 snapshot
借给 Task 9 reference closure。observer 不取得 workspace/ledger 可变引用，不保存
snapshot，不改变随机流，也不进入 serde/artifact/UI。

禁止创建 `EvolvedTectonicStepper`、`TectonicStepCandidate`、通用回调 trait，禁止
仅为候选事务给 workspace、ledger 或 `LabeledSubstreams` 增加 `Clone`。生产仍
运行原 one-shot 循环并只返回终点；reference closure 是第二个且唯一的实际
调用者。

- [ ] **Step 4: 运行 GREEN、确定性和预算回归**

Run: `cargo test --release --test evolved_tectonic_generation --test spherical_tectonic_causality --test evolved_tectonic_material --test evolved_tectonic_publication`

Expected: 全部 PASS；三 seed 的 observer/no-op 最终产品逐位等价，observer 次数
等于 timeline step count，生产 API/序列化面没有新增历史状态。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/runner.rs src/generators/natural/spherical_tectonics/publication.rs
git commit -m "Expose a test-only tectonic reference observer" -m "Keep production P2 one-shot while allowing one offline coupling probe to inspect accepted steps without publishing history."
```

---

### Task 6: 建立 P5 `f64` 地形组成状态

**Files:**
- Create: `src/generators/natural/surface_formation/state.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/surface_formation/isostasy.rs`
- Test: `src/generators/natural/surface_formation/generation.rs`
- Test: `tests/formation_coast_isostasy.rs`

**Interfaces:**
- Consumes: `PrimaryReliefSnapshot`, `FormationElevationComponents`, `LocalAiryIsostasy::response_from_validated_surface`。
- Produces: `FormationState::from_primary(&PrimaryReliefSnapshot) -> Result<Self, FormationStateError>`、`apply_surface_displacement_f64(&mut self, displacement_m: &[f64]) -> Result<(), FormationStateError>`、`current_elevation_exact_m(&self) -> &[f64]`、`current_elevation_f32(&self) -> &[f32]` 和 `wire_components(&self) -> Result<FormationElevationComponents, FormationStateError>`；`#[cfg(test)] pub(super) from_primary_values(Vec<f32>)` 只供解析测试，`#[cfg(test)] replace_primary_for_offline_reference(...)` 只服务 Task 9 的单一高成本参考。所有生产累计值保存在 `f64`，不另设 f32 位移写入口。

- [ ] **Step 1: 写亚 ULP、test-only 参考差量和真实越界 RED**

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
fn offline_reference_primary_replacement_preserves_surface_history() {
    let mut state = FormationState::from_primary_values(vec![100.0]).unwrap();
    state.apply_surface_displacement_f64(&[-12.0]).unwrap();
    state.replace_primary_for_offline_reference(&[100.0], &[130.0]).unwrap();
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

`replace_primary_for_offline_reference` 必须受 `#[cfg(test)]` 约束，并只校验/应用
新旧 P3 primary 的 `f64` 差量以保留参考路径的 P5 累计状态；生产 Lie-style 路径从最终
P3 恰好调用一次 `from_primary`，不得暴露或调用该替换入口。

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
- Produces: world 层 `SURFACE_FORMATION_HORIZON_YEARS`（沿用已冻结 P5 规格的 coarse-grained horizon，不从 P2 timeline 派生）；`advance_surface_processes(state, inputs, duration_years, cancellation) -> SurfaceAdvanceReport` 恰好消费请求物理时长；`recompute_surface_diagnostics(state, endpoint_inputs, cancellation) -> TerminalSurfaceDiagnostics` 在零时间推进下重算终点水文/过程率；`finalize_surface_formation(..., upstream, ...) -> NaturalSurfaceFormationSnapshot` 是唯一纯 P5 wire 发布入口，`upstream` 只带最终 climate checkpoint 指纹而不嵌入 P4，也不再把 P2 瞬时率再次积分进地形。

- [ ] **Step 1: 写有限时间与无双计数 RED**

在 `generation.rs` 的 `#[cfg(test)]` 模块写测试，以便直接调用最小可见性的 state/advance functions；integration test 不放宽生产可见性：

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
fn surface_advance_does_not_reapply_final_p3_or_tectonic_rates() {
    let fixture = zero_surface_process_fixture();
    let mut state = FormationState::from_primary_values(vec![125.0]).unwrap();
    advance_surface_processes(
        &mut state,
        fixture.inputs(),
        10_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(state.current_elevation_exact_m(), &[125.0]);
}

#[test]
fn final_surface_snapshot_binds_endpoint_climate_without_owning_it() {
    let (snapshot, endpoint_climate) = finalized_surface_fixture();
    assert_eq!(
        snapshot.checkpoint().upstream().formation_climate_checkpoint_fingerprint(),
        endpoint_climate.checkpoint().fingerprint(),
    );
    let json = serde_json::to_value(snapshot).unwrap();
    assert!(json.get("formation_climate").is_none());
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

pub(in crate::generators::natural) fn recompute_surface_diagnostics(
    state: &FormationState,
    endpoint_inputs: SurfaceProcessInputs<'_>,
    cancellation: &BuildCancellation,
) -> Result<TerminalSurfaceDiagnostics, SurfaceFormationGenerationError>;

pub(in crate::generators::natural) fn finalize_surface_formation(
    state: FormationState,
    surface: &SphericalSurfaceSnapshot,
    quality_profile: NaturalQualityProfile,
    upstream: SurfaceFormationUpstreamFingerprints,
    terminal_diagnostics: TerminalSurfaceDiagnostics,
    evolution_report: FormationEvolutionReport,
    cancellation: &BuildCancellation,
) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError>;
```

实现以 `remaining_years` 循环；每次取 `min(remaining_years, maximum_stable_step_years, maximum_elevation_domain_step_years)`，在 clone 候选上跑完整 hydrology→stream power→hillslope→coast→sediment→Airy→water 验证，成功才扣减 remaining。`ImplicitStreamPowerSolver` 拆成“当前 P2 rate 诊断”和“fluvial height solve”；该函数不再积分 `tectonic_displacement_m`，因为生产 formation state 已从最终 P3 完整初始化，P2 当前率只作为终态诊断/有单位 forcing，不得在 P5 再次生成一份构造高程。

`recompute_surface_diagnostics` 只在终点 P4 下重建 hydrology 和瞬时过程率，
不得调用任何会改变 elevation component、sediment inventory、water reservoir
或累计时长的推进算子。增加前后完整状态字节/指纹相等测试，防止“诊断重算”
偷偷形成第三个 P5 半步。

删除 `solve_geomorphic`、`generate_with_climate_solve_limit`、`EquilibriumOutsideElevationDomain` 和 `NotConverged` 生产路径。把 `FormationSolveReport` 替换为：

```rust
pub struct FormationEvolutionReport {
    accepted_surface_substeps: u32,
    integrated_duration_years: f64,
    current_rates: FormationResiduals,
    dense_state_bytes: u64,
}
```

`NaturalSurfaceFormationSnapshot` 中原 `solve_report` 字段同步改名为
`evolution_report: FormationEvolutionReport`，删除 `formation_climate`，
并只读公开 `evolution_report()`；`SurfaceFormationUpstreamFingerprints` 的
`initial_climate_checkpoint_fingerprint` 改名为
`formation_climate_checkpoint_fingerprint`。P2 accepted step 与 P4 solve
count 只由协调器/性能证据统计，不进入 P5 report；不在协调器 output 或 bundle
顶层复制 `FormationEvolutionReport`。`current_rates` 是诊断，不要求
`normalized_max() <= 1`；schema 和 `surface_formation_model_fingerprint`
同步升版。内部 P4 仍可使用既有快平衡求解，不把 PTC 扩回外层地貌。
`surface_formation_state_fingerprint` 同步删除 climate 参数，只散列实际保留
的 P5 terrain/process/hydrology；P4 绑定只由 checkpoint 的 upstream 指纹承担，
不得在 state fingerprint 内暗中保留第二份气候所有权。

- [ ] **Step 4: 运行 GREEN 与数值稳定族**

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Run: `cargo test --release --test formation_stream_power --test surface_formation_contracts`

Expected: 全部 PASS；有限时间测试允许非零 `dh/dt`，只要求归因、守恒、时长完整和数值稳定；终点诊断不改变状态，P5 wire 不包含 P4 payload。

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
- Consumes: Tasks 1–8 的 timeline、P2 one-shot generator、P3 投影、P4 solver、`FormationState`、`advance_surface_processes` 和 `recompute_surface_diagnostics`；test-only reference 另消费 Task 5 observer。
- Produces: crate-private `CausalNaturalFormationGenerator::generate_working(...) -> CausalFormationOutput`；output 包含最终 P2/P3/P4/P5 子域快照，以及只供终点身份/质量评估的私有 forcing，不包含历史或重复领域报告。

- [ ] **Step 1: 写顺序、失败全弃、确定性 RED**

在 `causal_formation.rs` 的 `#[cfg(test)]` 模块增加：

```rust
#[test]
fn two_p2_steps_feed_one_p5_advance_over_the_p5_horizon() {
    let (inputs, mut rng) = causal_inputs_for_test_steps(RootSeed::new(42), 2);
    let output = CausalNaturalFormationGenerator::generate_working(
        inputs,
        &mut rng,
        &BuildCancellation::new(),
    ).unwrap();
    assert_eq!(
        output.surface.evolution_report().integrated_duration_years().to_bits(),
        SURFACE_FORMATION_HORIZON_YEARS.to_bits(),
    );
    assert!(output.surface.terrain_fields().elevation_components()
        .equilibrium_adjustment_m().iter().any(|value| *value != 0.0));
    assert_eq!(
        output.surface.terrain_fields().elevation_components().primary_relief_m(),
        output.primary_relief.elevation_m(),
    );
}

#[test]
fn production_outer_schedule_is_two_p4_solves_and_one_p5_advance() {
    let trace = production_schedule_trace_for_test(RootSeed::new(42), 2).unwrap();
    assert_eq!(trace.p2_accepted_steps(), 2);
    assert_eq!(trace.p3_projections(), 1);
    assert_eq!(trace.p4_solves(), 2);
    assert_eq!(trace.p5_advances(), 1);
    assert_eq!(trace.terminal_diagnostic_recomputes(), 1);
}

#[test]
fn a_failed_surface_candidate_is_deterministic_and_does_not_mutate_inputs() {
    let fixture = causal_failure_fixture();
    let first = fixture.run_from_pristine_inputs().unwrap_err();
    let second = fixture.run_from_pristine_inputs().unwrap_err();
    assert_eq!(first.to_string(), second.to_string());
    assert_eq!(fixture.source_artifact_fingerprints(), fixture.initial_source_fingerprints());
}

#[test]
fn endpoint_climate_is_forced_by_final_terrain_and_surface_water_geometry() {
    let (output, context) = two_step_causal_output();
    let rebuilt = GlobalClimateForcingBuilder::build_for_formation_terrain(
        context.surface(),
        output.surface.terrain_fields(),
        context.climate_spec(),
        context.climate_domain(),
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(
        output.final_climate.checkpoint().forcing_fingerprint(),
        rebuilt.fingerprint(),
    );
    assert_eq!(
        output.surface.checkpoint().upstream()
            .formation_climate_checkpoint_fingerprint(),
        output.final_climate.checkpoint().fingerprint(),
    );
}
```

`causal_inputs_for_test_steps` 只在 `#[cfg(test)]` 通过
`ResolvedFormationTimeline::test_prefix(step_count)` 构造保持生产
`2 Myr` 步长的 locked timeline 前缀；production `generate_working` 永远
传 `formation.timeline()`。该 constructor 不允许改 P2 step duration，也不
导出到非测试 library、serde、artifact 或 UI。`production_schedule_trace_for_test`
只记录外层调用类别和次数，不携带科学状态，不进入 production、serde、artifact
或 UI；不得为测试公开协调器内部工作区。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --lib generators::natural::causal_formation::tests`

Expected: 编译失败，协调器不存在。

- [ ] **Step 3: 实现唯一生产分裂顺序**

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
    pub final_climate: GlobalCirculationSnapshot,
    pub surface: NaturalSurfaceFormationSnapshot,
    pub final_climate_forcing: GlobalClimateForcing,
}
```

`final_climate` 是将进入 bundle 的 sibling P4；`final_climate_forcing` 只供
协调器和 bundle factory 校验终点 forcing 身份并评估最终 P4，既不进入 world
bundle，也不序列化。

一次完整构建严格执行：

1. 调用 P2 one-shot generator 完整消费 resolved timeline；其内部循环不调用
   P3/P4/P5；
2. 从最终 `AuthoritativeTectonicView` 生成一次 substrate/P3，并据此初始化
   `FormationState`；
3. 从该完整 terrain/`SurfaceWaterGeometry` 重建 forcing 并求 start P4；
4. `advance_surface_processes` 以现有稳定子步恰好消费
   `SURFACE_FORMATION_HORIZON_YEARS`；该值属于 P5，不从 P2 timeline 派生；
5. 从 final terrain/`SurfaceWaterGeometry` 重建 forcing 并求 endpoint P4；
6. 在 endpoint P4 下调用 `recompute_surface_diagnostics`，不推进时间；
7. 校验 solid/sediment/water/component、最终 climate checkpoint 绑定和 endpoint
   forcing identity；
8. 只有全部成功才组装 output；失败或取消不返回部分 P2/P5 artifact。

这是一轮 Lie-style 顺序分裂，不是可配置 cadence。P2 和 P5 保留各自已批准且
来源不同的 resolved horizon，因此不宣称同一 `Δt` 上的形式 Trotter 收敛阶。
P2/P3 随机形态都从协调器一次 capture 的 `LabeledSubstreams` 固定标签派生，
重复调用不消耗相邻模块随机流。实现不得预建 predictor-corrector 分支；只有
离线证据证明该生产路径未满足既有最终态质量包络时，才按规格显式修订为一次
固定校正。endpoint P4 始终必需，禁止以 start P4 发布终态。

- [ ] **Step 4: 运行 GREEN 的短前缀解析/确定性测试和离线参考探针**

Run: `cargo test --release --lib generators::natural::causal_formation::tests`

Expected: 全部 PASS；两步 fixture 的 P2 accepted 顺序、单次 P3/P5、两次外层
P4、终点闭合和拒绝原子性成立。

同一模块增加 `#[cfg(test)]` 且 ignored 的
`compare_production_split_with_high_cost_reference`。它固定使用生产
`Standard`/seed `42`、相同 resolved inputs 和标签子流，分别运行：

- 生产路径：完整 P2 → 最终 P3 → start P4 → 一次完整 P5 → endpoint P4；
- 高成本参考：每个 P2 宏步生成对应 P3 组成变化，把
  `SURFACE_FORMATION_HORIZON_YEARS / timeline.step_count()` 的 P5 时长分配给
  该窗口，并执行 start P4 → half P5 → midpoint P4 → half P5 → endpoint P4。
  所有窗口的 P5 时长之和必须与生产路径相同；不得让 P5 跟随 P2 累计成
  `256 Myr`。

参考 helper、逐步状态和 trace 全部保持 test-private，不进入 library API、serde、
artifact 或 cache。两条路径只输出最终 exact elevation components、
`SurfaceWaterGeometry`、五来源 sediment mass、water reservoirs、climate fields、
既有 quality metrics、守恒残差、wall time、peak RSS 与相关
checkpoint/forcing/bundle fingerprints 到
`target/natural-quality/causal-formation/offline-reference-standard-seed-42.json`
及 blake3。不得生成 `2/1/0.5 Myr` 路径、差值比或新通过阈值。

Run: `cargo test --release --lib compare_production_split_with_high_cost_reference -- --ignored --nocapture`

Expected: probe 完成并生成非空 JSON；两条路径分别消费相同的 P2 timeline 和
相同的 P5 horizon，并共享输入/随机身份，各自满足硬不变式；文件记录原始最终态
差值与成本且不进入 artifact。该
命令只在耦合策略改变或明确的离线研发复核时运行，不属于常规构建/CI 门。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/causal_formation.rs src/generators/natural/mod.rs src/world/natural/formation.rs
git commit -m "Coordinate causal natural formation" -m "Run one production split over the resolved timeline and close the published climate against final terrain."
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
    assert_eq!(bundle.climate().surface_ref(), bundle.surface_ref());
    assert_eq!(bundle.surface_formation().surface_ref(), bundle.surface_ref());
    assert_eq!(
        bundle.surface_formation().checkpoint().upstream()
            .formation_climate_checkpoint_fingerprint(),
        bundle.climate().checkpoint().fingerprint(),
    );
    let json = serde_json::to_value(bundle).unwrap();
    for forbidden in ["history", "checkpoints", "pseudo_time", "rejected_steps"] {
        assert!(json.get(forbidden).is_none());
    }
    assert!(json["surface_formation"].get("formation_climate").is_none());
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
    climate: GlobalCirculationSnapshot,
    surface_formation: NaturalSurfaceFormationSnapshot,
    tectonic_quality: NaturalQualityReport,
    primary_relief_quality: NaturalQualityReport,
    climate_quality: NaturalQualityReport,
    surface_quality: NaturalQualityReport,
}
```

另定义字段完全相同的 private `NaturalFormationBundleWire: Deserialize`，手写
`Deserialize for NaturalFormationBundle`，反序列化只能转调
`NaturalFormationBundle::new(...)` 并执行结构/同包身份 `validate()`；不得
derive 一个绕过验证的 artifact。terrain→forcing 是需要 climate spec/work
domain 的跨层关系，不能伪装成 world 层无上下文校验：它由协调器保留的
`final_climate_forcing` 在唯一 artifact factory 中做 contextual validation。

`validate()` 逐域调用既有 validator，并校验共同 `SurfaceRef`、timeline，
以及 `surface_formation.checkpoint().upstream().formation_climate_checkpoint_fingerprint()`
必须等于 sibling `climate.checkpoint().fingerprint()`。唯一 artifact factory
还必须断言：

```rust
assert_eq!(
    output.final_climate.checkpoint().forcing_fingerprint(),
    output.final_climate_forcing.fingerprint(),
);
```

`final_climate_forcing` 必须是 Task 9 从 final formation terrain/
`SurfaceWaterGeometry` 直接构造
并随同 output 传入的同一个不可变对象，不得仅凭指纹反查或在 bundle factory
另建第二条 forcing 路径。每份 quality report 继续由自己的既有 validator 校验
profile、fingerprint，并在该报告 schema 已支持时校验 subject/surface 绑定，
不给报告新增第二套通用身份字段。P4 quality evaluator 新增有两个真实消费者的
窄 enum：

```rust
pub(crate) enum ClimateQualityTerrain<'a> {
    Primary(&'a PrimaryReliefSnapshot),
    Formation(&'a FormationTerrainFields),
}
```

原 P4 evaluator wrapper 走 `Primary`，bundle factory 对最终 P4 走 `Formation`；两者共享同一 metric 实现，不复制质量方程。artifact 只实现 `Serialize`，crate-private `new`，唯一 public factory 是 `NaturalFormationBundleArtifact::generate(inputs, rng, cancellation)`。

`CausalNaturalFormationStageInputs` 的 dependencies 固定为 profile、resolved tectonic/world formation/geologic/climate/hydro、relief spec、surface、climate work domain；stage version 从 1 开始，id 为 `natural.causal-formation`。把仍有真实外部消费者的 `NaturalQualityProfileArtifact` 与 `ReliefSpecArtifact` 定义迁入该文件，保持 artifact key 不变；通用 `Stage` 不修改。

stage error code 固定为：`causal-formation.invalid-input`、`causal-formation.numerical-stability`、`causal-formation.solid-budget`、`causal-formation.sediment-budget`、`causal-formation.water-budget`、`causal-formation.climate-not-converged`、`causal-formation.endpoint-forcing-mismatch`、`causal-formation.elevation-out-of-range`、`causal-formation.resource-limit` 与既有 `engine.cancelled`。错误只携带诊断值，不携带可发布的中间 bundle。

- [ ] **Step 4: 运行 GREEN、serde 伪造与原子失败测试**

Run: `cargo test --release --test geologic_pipeline_contracts --test causal_formation_generation`

Expected: 全部 PASS；取消、数值越界、预算失败都没有 `NaturalFormationBundleArtifact`，错误分别映射到 stable diagnostic code。

- [ ] **Step 5: 运行完整 locked timeline 实测，不先写新 cadence/阈值**

`tests/causal_formation_performance.rs` 使用既有 P5 门禁事实：Draft/Standard/High
分别 `15/90/300 s`，High retained dense state `1 GiB`，取消 `250 ms`；
测试记录完整 P2 timeline、最终 P3、start P4、一次 P5 advance、endpoint P4、
终点诊断各自的 wall time，P5 stable substeps、forcing/checkpoint fingerprints、
peak RSS、取消延迟和最终 bundle fingerprint。不得为满足预算跳过 endpoint P4、
缩短 P2 timeline/P5 horizon、把二者混成一个时域或新写一个 cadence。

Run: `cargo test --release --test causal_formation_performance -- --ignored --nocapture`

Expected: 生成 `target/natural-quality/causal-formation/performance.json`，生产外层
调用数为两次 P4/一次 P5，且无部分 artifact。Task 9 的离线参考 JSON 单独保留，
不由本性能测试重复运行。

- [ ] **Step 6: 验证既有最终态/性能门并提交**

把机器、编译档、seed、profile、逐阶段耗时、峰值内存、取消延迟、最终
forcing/checkpoint 身份和文件 blake3 写入设计规格“实测修订”。Task 9 的
`Standard`/seed `42` 离线参考另记录两条路径的原始最终态差值、既有质量、
守恒和成本；它不产生新 envelope。迁移门同时要求：

1. endpoint P4 forcing identity 与 sibling checkpoint identity 通过；
2. solid/sediment/water/component、有限性、支持域和现有数值稳定性门禁通过；
3. 既有 P2/P3/P4/P5 最终态质量包络通过；
4. 既有 profile 时间/内存/取消门禁通过；
5. 代表性离线参考文件及 blake3 已记录，且没有把中间轨迹写入 artifact。

- 五项全部通过：提交本任务并继续 Task 11；离线原始差值本身不构成失败。
- 1–3 因耦合离散误差失败：提交可复核证据后停止执行 Task 11–13，判断是否需
  显式修订为一次固定 predictor-corrector。仅第 4 项性能失败时，先定位 P2/P4/
  P5 成本，再按规格允许的近似线性解、多分辨率工作域或内部有界求解另作有出处
  修订；predictor-corrector 会增加成本，不是性能修复。两类失败都不得预先实现
  双路径、减少 P2 timeline/P5 horizon、跳过 endpoint P4、放宽门禁或发布未完成
  状态。
- 仅参考探针因明确资源限制无法完成：记录机器与失败点，不把生产路径伪称为
  已与参考一致；该离线研发缺口不在常规构建中反复支付，也不放宽 1–4。

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
    assert_eq!(document.formation_climate(), bundle.bundle().climate());
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

`SphericalFormationFieldDocument::initial_circulation` 当前无消费者且新 bundle
不发布初始 P4 checkpoint，按 YAGNI 删除；需要气候的调用点只读 sibling
`formation.bundle().climate()`。若文档保留 `formation_climate()` 访问器，
它只能是该 sibling 的借用别名，不得从 `surface_formation` 取值或缓存第二份。

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
            mutated.climate(),
            original.climate(),
            "seed {seed}",
        );
        assert_eq!(mutated.surface_formation(), original.surface_formation(), "seed {seed}");
    }
}
```

该测试放在 `causal_formation.rs` 的 private test module；`generate_with_test_compatibility_mutation` 在最终 P2 snapshot 生成后、任何 P3/P4/P5 消费前，只改 compatibility elevation 数组。helper 与 mutation enum 均受 `#[cfg(test)]` 约束，不增加 production 注入点。

补全 seed `3/7/42` Draft，现有 atlas/evidence corpus，至少一个 Standard release build；断言非零当前 `dh/dt` 合法、所有位移可归因、solid/sediment/water/component budget 闭合。

- [ ] **Step 2: 运行 Release 科学/呈现套件**

Run: `cargo test --release --test geologic_pipeline_contracts --test causal_formation_generation --test surface_formation_contracts --test surface_formation_generation --test surface_formation_quality --test surface_formation_evidence --test surface_formation_atlas`

Run: `cargo test --release --lib compatibility_elevation_ablation_preserves_p3_p4_p5_authority`

Run: `cargo test --release --test natural_field_registry_spherical --test field_display_integration --test spherical_presentation_integration`

Expected: 全部 PASS；atlas/golden 只因已批准的科学状态变化更新，并在完成记录列出旧/新 hash 与因果说明。

- [ ] **Step 3: 运行性能、内存和取消门禁**

Run: `cargo test --release --test causal_formation_performance -- --ignored --nocapture`

Expected: 生产 Lie-style 路径满足规格修订中的既有最终态/性能门；取消 latency、
peak RSS、完整时长和 cache identity 全部 PASS。不得在这里重复运行高成本参考，
也不得要求三层细化或新误差包络。

- [ ] **Step 4: 运行项目级静态门禁**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo check --target wasm32-unknown-unknown --all-features --lib`

Expected: 三条命令退出码均为 0。

- [ ] **Step 5: 运行完整调试回归**

Run: `cargo test --workspace --all-targets --all-features`

Expected: 全部 PASS；按项目说明预留约 40 分钟，保留最终摘要中的 passed/failed/ignored 数量。

- [ ] **Step 6: 写完成记录并提交**

完成记录必须包含：task/commit 对照、所有命令与退出码、生产性能 JSON/hash、
`offline-reference-standard-seed-42.json`/hash、兼容消融 hash、schema/stage version
变化、删除清单、已知开放问题，以及以下用户验收步骤：

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

- P2 球面程序构造机制与 `2 Myr` 离散步：Cortial et al. (2019),
  *Procedural Tectonic Planets*, DOI `10.1111/cgf.13614`，§3 与
  Appendix A 的 `δt = 2 My`。论文支持程序化地质过程近似，不把 P2 升格为
  预测性地球动力学。
- `128` 步参考形成时域与 current/next 双缓冲：步数沿用用户已批准的 Sekai
  产品参数 `EVOLUTION_STEP_COUNT`，作为 resolved model identity；它不是
  Cortial 常量，也不宣称世界真实年龄。双缓冲是现有 P2 one-shot 循环的私有
  实现细节，不开放为跨模块逐步候选接口，也不属于论文科学结论。
- P5 `SURFACE_FORMATION_HORIZON_YEARS`：沿用已批准的
  `2026-08-18-coupled-geomorphic-formation-p5-design.md` §5 `100,000 yr`
  coarse-grained 产品参数；不是地球稳态时间，也不从 P2 的 `128 × 2 Myr`
  timeline 派生。本计划只让 P5 恰好消费一次该 horizon，不新增数值。
- 生产单次顺序分裂：Trotter (1959), *On the Product of Semi-Groups of
  Operators*, DOI `10.1090/S0002-9939-1959-0108732-6`。它支持用顺序算子积
  近似同一演化问题，不给出 Sekai 的总时域、误差或“两次外层 P4”常量。P2/P5
  使用各自 resolved horizon，故本计划只主张 Lie-style 调用结构，不主张形式
  Trotter 收敛阶；这是明确的工程类比和开放问题。
- 当前态由私有有限历史得到、产品只发布终点，且反馈机制保留但可异步/低频
  耦合：Paik & Kim (2021), DOI `10.5194/hess-25-2459-2021`；Shen, Lynch,
  Poulsen & Yanites (2021), DOI `10.1016/j.cageo.2020.104625`。Santos,
  Caldwell & Bretherton (2021), *Cloud Process Coupling and Time Integration
  in the E3SM Atmosphere Model*, DOI `10.1029/2020MS002359`，直接说明顺序
  过程耦合会对 cadence 敏感，因此本计划离线测量而不假定轨迹收敛。
- 一次固定 predictor-corrector 和高成本迭代参考的数值类比：Schüller et al.
  (2025), DOI `10.5194/gmd-18-9167-2025`，比较非迭代/迭代 Earth-system
  coupling 并记录非光滑参数化的非收敛风险；Strang (1968), DOI
  `10.1137/0705041`，只支持离线参考的对称排序背景。本计划不预实现校正路径，
  也不声称全系统二阶。
- 后期空间参数分布的数学入口：Lang & Schwab (2015), DOI
  `10.1214/14-AAP1067`（球面相关随机场与频谱截断）；Lindgren, Rue &
  Lindström (2011), DOI `10.1111/j.1467-9868.2011.00777.x`（流形/三角网格上的
  Matérn/SPDE 场）；Lagae et al. (2009), *Procedural Noise using Sparse Gabor
  Convolution*（方向/频谱控制）；Goff & Jordan (1988), DOI
  `10.1029/JB093iB11p13589`（各向异性海床协方差的地学类比）。这些来源不为
  任何具体 P2/P3/P5 参数给出通用分布；边际/联合分布、尺度、条件关系和值仍须
  逐参数找直接出处，否则明确列为开放问题。
- 参数采样的确定性/正交性沿用现有 `LabeledSubstreams` 工业实现：固定 32-byte
  根材料经长度分帧的 BLAKE3 标签派生 `rand_chacha::ChaCha8Rng`；实现位于
  `src/generators/natural/random.rs`，并由既有跨标签不干扰测试守门。该机制只
  负责可重放与模块隔离，不替代物理分布出处。
- 河流侵蚀的隐式下游栈：Braun & Willett (2013), DOI `10.1016/j.geomorph.2012.10.008`；抬升—侵蚀响应背景：Whipple & Tucker (1999), DOI `10.1029/1999JB900120`。
- 河流侵蚀—输运—沉积连续方程与解析递推：Davy & Lague (2009), DOI `10.1029/2008JF001146`；Barnhart et al. (2019), DOI `10.5194/gmd-12-1267-2019`；pyBadlands/SPACE 的守恒库存实践见 Salles (2018) 与 Shobe et al. (2017)。
- 非线性坡面稳定子步与离散最大值：Eymard, Gallouët & Herbin (2000), DOI `10.1016/S1570-8659(00)07005-8`；Landlab `TaylorNonLinearDiffuser` commit `8f59a66279cefa288b146735a939d95e9a6730c2`。
- 海岸交换、沉积库存和 generalized Exner 质量账本：Paola & Voller (2005), DOI `10.1029/2004JF000274`；具体项目适用边界按上位规格保留。
- 局部 Airy 加载/卸载：Turcotte & Schubert (2014), *Geodynamics*, 3rd ed., ch. 5；响应直接记入完整 `f64` 状态，不按 artifact 边界裁剪。
- PTC 只用于有根的内部快平衡，不再作为全地貌绝对稳态：Kelley & Keyes (1998), DOI `10.1137/S0036142996304796`；PETSc 只作数值实现类比，不提供 Sekai 科学容差。
- 浮点身份、先定义精确状态再做发布舍入：Goldberg (1991), DOI `10.1145/103162.103163`；稳定误差解释参考 Higham (2002), *Accuracy and Stability of Numerical Algorithms*, 2nd ed.
- 原子候选/提交、确定性重放与 artifact 内容寻址沿用本项目现有 engine 工业实践；不修改通用 Stage，不引入第二份科学事实源。
