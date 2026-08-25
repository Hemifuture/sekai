# 地质管线契约恢复与因果当前态 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 恢复 P2→P3→P4→P5 的单一过程所有权，以私有有限物理时间演化生成一个原子当前态 bundle，消除兼容高程污染、绝对地貌稳态误用和跨步 `f32` 状态回流，同时保持现有 UI 字段能力。

**Architecture:** `world` 提供唯一 resolved formation timeline、借用型权威构造视图、P4/P5 sibling 当前态 bundle schema 与数值/守恒契约；`generators/natural` 内的领域协调器先完成 P2 timeline 和最终 P3 投影，再以 start P4 → 一次完整 P5 → endpoint P4/零时间终点诊断执行生产 Lie-style sequential split，验证最终 forcing 后原子发布，不改通用 `engine::Stage`。生产图最终只发布一个 `NaturalFormationBundleArtifact`，UI 从该 bundle 的最终 sibling 快照读取字段，不发布历史、伪时间或求解中间态。

**Tech Stack:** Rust 2024、serde、thiserror、blake3、现有 Stage/Artifact/BuildCancellation、现有 P2–P5 生产算子与 egui/eframe 呈现层；不新增第三方依赖。

**Spec:** `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`

**2026-08-25 最终态/效率与联合审查显式修订：** 本计划已按规格 §0/§0.1 删除每宏步三次 P4、
`2/1/0.5 Myr` 三层细化发布门和人工误差包络。生产只实现单次 Lie-style 分裂与
endpoint closure；原三次 P4 路径仅保留为一个 `Standard`/seed `42` 离线参考
探针。联合审查进一步恢复 P5 对最终 P2 有单位构造强迫的有限时积分、九项最终
因果高程组成、端到端 `f64` kernel 和前置性能归因；这些后写条目替代下文任何
仍暗示“构造率只诊断”“两项 aggregate 组成”或“kernel 读取 f32 scratch”的旧句。
联合终审还删除 P3 中无出处、等效额外积分 `250 kyr` 的当前构造率增益，避免与
P5 的唯一构造位移积分双计数；P4/P5 sibling 所有权和最终 forcing 身份要求不变。

## Global Constraints

- 只发布最终当前态；P2/P3/P4/P5 的逐步状态、候选、拒绝步、伪时间和重试日程不得进入 schema、artifact、缓存恢复契约或 UI。
- P2 只拥有固体地球演化、地壳物质与构造成因；P3 只做同一时刻固体状态到基础地形的投影；P4 只解当前边界上的快平衡；P5 只拥有地表、水圈、沉积和地表载荷 Airy 响应。
- 一次完整生产构建必须执行：P2 完成自己的 resolved timeline → 最终 P3 投影 → start P4 → P5 完整消费 `SURFACE_FORMATION_HORIZON_YEARS` → endpoint P4 → 零时间终点诊断；不得在每个 P2 宏步调用 P4，也不得把 P2 的 `128 × 2 Myr` 时域误传给 P5。终点 P4 forcing 必须由最终完整地形及其 `SurfaceWaterGeometry` 重建。
- P5 从最终 P3 当前态开始，并在自己的 formation horizon 内把最终 P2
  `uplift_rate_mm_per_year - subsidence_rate_mm_per_year` 作为零阶保持 forcing
  积分到 `tectonic_displacement_m`；不得重新叠加 P3 已含的累计几何，也不得把
  构造率降级为只读诊断。
- P3 不得把当前 P2 构造率乘经验响应时间形成位移；删除
  `DYNAMIC_RATE_RESPONSE_M_PER_MM_PER_YEAR` 与对应 rate-response helper。
  P3 只做物质、厚度、年龄、造山类别及具名过程的同一时刻投影；当前率到位移
  的唯一生产所有者是 P5。
- P4 climate 与 P5 surface formation 在 bundle 中是 sibling；P5 snapshot 不嵌入 climate，只以 `formation_climate_checkpoint_fingerprint` 绑定最终 P4。
- 生产 P3/P4/P5 不得读取 `compatibility.tectonic_elevation_m`；只改这一兼容字段不得改变任何权威下游结果。
- retained state 必须分别保存九项 `f64` 因果高程组成：primary、tectonic、
  fluvial erosion、hillslope erosion/deposition、routed sediment deposition、
  coastal erosion/deposition、isostatic response；最终高程只由生产恒等式求和。
  不得用 `equilibrium_adjustment`/`surface_adjustment` aggregate 替代这些最终事实。
- 科学 kernel 的 elevation、displacement、process/hydrology input/output 和沉积
  质量库存端到端保留 `f64`；`f32` 只在完整 `f64` 状态通过校验后生成最终
  wire/GPU 快照，且不得成为任何后续 kernel 输入。
- 不得 clamp 科学状态，不得扩大 `ELEVATION_MIN_M`/`ELEVATION_MAX_M`，不得按目标地貌做重映射、经验修形或特殊格元分支。
- 不增加世界年龄、最高程、耦合 cadence、收敛容差或显示范围旋钮；形成时间线是 resolved 输入身份，不是本轮 UI 配置。
- 后期“统一噪声”只表示同一物理算法的某些参数可由常数推广为有出处的空间分布。当前计划只记录 §0/规格 §2.6 的输入边界，不增加分布 schema、用户旋钮、中央噪声 stage 或无人消费的泛型抽象。
- 不增加通用循环 stage、多输出 stage、反馈 trait 或无人消费的适配器；领域协调发生在 `src/generators/natural/`。
- 每个任务严格 RED→GREEN→提交；无法产生 RED 的纯删除/文档步骤使用变异探针证明守门能力，并在提交正文如实说明。
- 当前工作树已有未提交的 P4/P5 R4、`f64`、UI 发布事务和测试改动。它们是后续
  任务真实依赖的继承输入，不得靠部分暂存制造只在脏工作树中可编译的提交。
  Task 0 先按完整路径清单把这些相关改动修到 GREEN 并落成一个可复现基线；
  `debug.log` 等范围外文件继续保留。不得 reset、checkout、删除或用
  `git add -A`；每次提交前必须检查 `git diff --cached --name-only`、完整 staged
  diff，并在提交后从该提交树复跑任务门禁。
- 迭代期 P5/全链目标测试使用 `--release`；最终必须运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo check --target wasm32-unknown-unknown --all-features --lib` 和完整调试回归。
- 算法任务在 UI 接入并由用户亲自验证前不得宣称交付完成。
- Task 11 的公开 artifact/UI 原子迁移门只检查终点身份、硬不变式、既有质量包络和既有性能/取消包络；离线参考原始差值是研发证据，不新增发布阈值。

---

## 文件与职责图

### 新建文件

- `src/world/natural/formation_bundle.rs`：最终当前态 bundle、schema、严格验证和只读子域访问器。
- `src/generators/natural/causal_formation.rs`：P2/P3/P4/P5 私有因果协调器；只编排现有算子，不复制方程。
- `src/generators/natural/causal_formation_stage.rs`：单输出 `NaturalFormationBundleArtifact`、质量报告封装和生产 stage/graph。
- `src/generators/natural/surface_formation/state.rs`：P5 子步间保留的 `f64` 高程组成与沉积质量库存；最终 wire 投影的唯一入口。
- `tests/geologic_pipeline_contracts.rs`：模块所有权、兼容字段消融、bundle 原子性和生产图契约。
- `tests/causal_formation_generation.rs`：有限时间、确定性、守恒、无重置与错误分类集成测试。
- `tests/causal_formation_performance.rs`：Release-only 生产时间线、内存和取消实测；高成本耦合顺序对照只在 Task 10 ignored probe 中运行。
- `tests/support/causal_formation.rs`：跨集成测试共享的生产输入/构建 fixture，不重写算法。

### 重点修改文件

- `src/world/natural/formation.rs`：`ResolvedFormationTimeline` 及其在 `ResolvedWorldFormation` 中的身份。
- `src/world/natural/evolved_tectonics.rs`：不暴露兼容高程的 `AuthoritativeTectonicView<'a>`。
- `src/world/natural/surface_formation.rs`：把稳态报告替换为有限时间演化报告，移除嵌套 P4，保留最终 climate checkpoint 指纹绑定并刷新 schema/模型指纹。
- `src/generators/natural/spherical_tectonics/{runner,publication,forcing}.rs`：逐步 P2、当前态预览发布和 forcing 去兼容高程。
- `src/generators/natural/spherical_tectonics/processes/relaxation.rs`：固体年龄推进与 legacy 高程松弛分离。
- `src/generators/natural/{geologic_substrate,primary_relief}.rs`：只消费权威视图，P3 不继承兼容高程、不积分当前构造率，并在最终 wire 前保留私有 `f64` working state。
- `src/generators/natural/surface_formation/{generation,hydrology,stream_power,sediment,hillslope,coast,isostasy}.rs`：有限物理时间 P5 步进、端到端 `f64` kernel、九项累计组成和一次构造 forcing 积分。
- `src/generators/natural/quality/surface_formation.rs`：退役绝对稳态发布门，将终点当前过程率保留为非门禁观测或在无消费者时删除。
- `src/generators/natural/global_circulation/{forcing,generation}.rs`：复用现有“formation terrain→forcing→P4”入口；不新增第二套气候方程。
- `src/generators/natural/{terrain_amplification,hierarchical_derivation}.rs`：T1 放大链从 bundle sibling 显式借用最终 P4，不再从 P5 snapshot 反向取得 climate。
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
PrimaryReliefWorkingState (private f64) ─► final P3 wire projection
        │
        ▼
FormationState::from_primary_working()
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

`one complete P5 advance` 显式借用最终 `EvolvedTectonicSnapshot`，在 P5 horizon
内只积分当前构造率；上游 P3 已经包含的累计固体几何不在这里重放。图中的
`PrimaryReliefWorkingState` 与 `FormationState` 内部都是 `f64`；P3 的最终
`f32` wire 只用于已接受 snapshot/呈现，不得作为 P5 初态或任何后续 kernel 输入。
`FormationState` 内部是九项因果组成和最终和的 retained state，图外不存在可
回流的 `f32` 科学 scratch。

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
- 当前生产 `compatibility()` 消费清单：`geologic_substrate.rs` 读取 crust kind/thickness/age；`primary_relief.rs` 读取 plate/crust geometry 且错误读取 compatibility elevation；`quality/primary_relief.rs` 只读取几何做 P3 证据；`app/spherical_formation_display.rs` 只呈现 plate/crust category；`quality/evolved_tectonics.rs` 与 evolved tests 审计 legacy V3 产品；`terrain_amplification.rs` 及转调它的 `hierarchical_derivation.rs` 只读取 crust age、lineation 与 orogeny 几何，不读取 compatibility elevation。Task 2/3 只迁移前两个权威生成消费者；其余几何/呈现/legacy 审计消费者不为架构整齐做无意义迁移，但 T1 在 Task 11 改为从 bundle 显式取得 sibling climate。任何路径都不得把兼容高程传回权威科学链。
- R4 在既有 `100 ka` 构造时域上记录九组旧整步/两半步耦合路径耗时
  `208–279/415–547/829–1075 s`；其中最小 `3.125 ka` 窗口即
  `13.8–17.9 min`。它证明旧求解与互调方式不满足既有产品预算，不否定 P5
  已恢复的固定构造时域，也不授权缩短时域；这些数字可能仍含外层重复求解，不能
  直接断言单次新 kernel 成本。Task 6 先做归因探针，Task 11 再测最终生产分裂。

### 继承 dirty 落地边界

- 本规格的 §0/§0.1 显式修订与本实施计划由协调者在 Task 0 开始前作为一个单独、
  仅含这两个目标文档的 docs-only 提交落地：
  `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`
  与 `docs/superpowers/plans/2026-08-25-geologic-pipeline-contract-restoration.md`。
  提交前必须确认 staged name list 恰好为这两项；Task 0 不得再次吸收它们。这样后续
  Task 6/11/13 对规格追加实测修订时，只会提交各任务真正新增的内容。
- Task 0 接管当前状态清单中全部 P4/P5/R4、T1 与 UI 发布事务相关路径；它们共同
  才构成当前可编译工作树，不能拆成依赖未提交 hunk 的伪独立提交。
- 当前已知 RED 包括
  `formation_terrain_reuses_exact_p4_forcing_and_changes_checkpoint_causally` 与
  P5 scientific files 中的 clippy diagnostics。Task 0 只修这些继承语义自己的
  问题，不提前实现 Tasks 1–13 的新架构。
- `docs/superpowers/specs/2026-08-08-spherical-presentation-design.md`、
  `docs/superpowers/specs/2026-08-24-transient-climate-geomorphology-design.md` 与
  `docs/superpowers/plans/2026-08-24-p5-publish-transaction.md` 随其生产实现一起
  落地；根目录 `debug.log` 是范围外宿主文件，始终不暂存、不修改、不删除。
- Task 0 后所有任务从干净的相关路径开始并整文件暂存其具名清单；
  `ReliefSpecArtifact` 继续定义在现有 `relief_spec.rs`，不因 bundle 迁移挪动事实源。

---

### Task 0: 落地并验证继承的 P4/P5/R4 与 UI 发布事务基线

**Files:**
- Modify: `docs/superpowers/specs/2026-08-08-spherical-presentation-design.md`
- Modify: `docs/superpowers/specs/2026-08-24-transient-climate-geomorphology-design.md`
- Create: `docs/superpowers/plans/2026-08-24-p5-publish-transaction.md`
- Modify: `src/app.rs`
- Modify: `src/app/spherical_formation_display.rs`
- Modify: `src/engine/cache.rs`
- Modify: `src/generators/natural/global_circulation/forcing.rs`
- Modify: `src/generators/natural/global_circulation/generation.rs`
- Modify: `src/generators/natural/quality/surface_formation.rs`
- Modify: `src/generators/natural/surface_formation/coast.rs`
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/surface_formation/hillslope.rs`
- Modify: `src/generators/natural/surface_formation/hydrology.rs`
- Modify: `src/generators/natural/surface_formation/isostasy.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/generators/natural/surface_formation/sediment.rs`
- Modify: `src/generators/natural/surface_formation/stream_power.rs`
- Modify: `src/generators/natural/terrain_amplification.rs`
- Modify: `src/ui/field/localization.rs`
- Modify: `src/world/natural/fields.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/world/natural/surface_formation.rs`
- Test: `tests/formation_coast_isostasy.rs`
- Test: `tests/formation_hillslope.rs`
- Test: `tests/formation_hydrology.rs`
- Test: `tests/formation_sediment.rs`
- Test: `tests/formation_stream_power.rs`
- Test: `tests/surface_formation_atlas.rs`
- Test: `tests/surface_formation_contracts.rs`
- Test: `tests/surface_formation_evidence.rs`
- Test: `tests/surface_formation_generation.rs`
- Test: `tests/surface_formation_performance.rs`
- Test: `tests/surface_formation_quality.rs`
- Test: `tests/surface_formation_stage.rs`
- Test: `tests/terrain_audit_probe.rs`

**Interfaces:**
- Consumes: 进入本计划时记录的 tracked dirty snapshot
  `32e0ca43a8055bf4a3c39e8c62cbb441de106734` 与上列唯一文件清单。
- Produces: 一个从提交树本身即可编译和复现测试的继承基线；不改变 Tasks 1–13
  冻结的新架构，不吸收 `debug.log` 或任何执行期间新出现的范围外修改。

- [ ] **Step 1: 核对继承字节与路径边界**

运行 `git status --short`、逐文件 `git diff --check` 和上列路径的完整 diff；对照
dirty snapshot 确认没有丢失既有 hunk。若执行期间出现不在清单中的新用户改动，
保留并排除，不扩大 Task 0。不得 reset、checkout、stash 或把文件恢复成 HEAD。

- [ ] **Step 2: 复现继承 RED，不把脏工作树的可编译性当提交证据**

Run:
`cargo test --release --lib generators::natural::global_circulation::forcing::formation_tests::formation_terrain_reuses_exact_p4_forcing_and_changes_checkpoint_causally`

Expected: 复现当前 forcing/checkpoint 因果断言失败。

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: 复现当前 P5 scientific files 中的既有 diagnostics；记录精确路径与数量。

- [ ] **Step 3: 只修继承实现自己的失败**

按 `2026-08-24-p5-publish-transaction.md` 已记录的 RED/GREEN 和当前 R4 设计事实，
修复 forcing identity 测试或实现的真实不一致，并清理 clippy；不得借机实现
timeline、authoritative view、九项新 retained state、有限时间报告或 bundle。
若失败暴露机制错误，先用现有测试定位因果；不得 clamp、改门禁或改快照来迁就
测试。纯格式/警告修复不改变数值语义。

同时在继承的 `2026-08-24-transient-climate-geomorphology-design.md` §14.5 增加
2026-08-25 显式修订指针：本规格 §0.1(9) 已替代“固定 `100 ka` 时域本身被否决”的
旧结论；九组旧整步/两半步数据只证明旧外层重复求解与互调过重，不否定
`SURFACE_FORMATION_HORIZON_YEARS`，也不构成新的发布阈值。§14.7 同步澄清：
`FORMATION_SEDIMENT_CAPACITY_KG_M3` 已不再存在于生产 P5；
`FORMATION_FLOODPLAIN_ACCOMMODATION_M` 只由既有 T1 呈现链按其冻结规格消费，
不参与 P5 科学路由，恢复 horizon 不重新证明其数值。该 T1 数值的直接出处仍是
所属 T1 规格的独立开放问题，不在 Task 0 中偷换机制、补拍系数或扩大范围。

- [ ] **Step 4: 从完整候选和提交树复跑 GREEN**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo check --target wasm32-unknown-unknown --all-features --lib`

Run: `cargo test --release --lib natural_app_tests`

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Run: `cargo test --release --test formation_coast_isostasy --test formation_hillslope --test formation_hydrology --test formation_sediment --test formation_stream_power`

Run: `cargo test --workspace --all-targets --all-features`

Expected: 全部 PASS；最后一条按项目说明预留完整调试回归时间。若只在未暂存
工作树通过，Task 0 不得提交。

- [ ] **Step 5: 整文件暂存继承基线并提交**

```bash
git add docs/superpowers/specs/2026-08-08-spherical-presentation-design.md docs/superpowers/specs/2026-08-24-transient-climate-geomorphology-design.md docs/superpowers/plans/2026-08-24-p5-publish-transaction.md src/app.rs src/app/spherical_formation_display.rs src/engine/cache.rs src/generators/natural/global_circulation/forcing.rs src/generators/natural/global_circulation/generation.rs src/generators/natural/quality/surface_formation.rs src/generators/natural/surface_formation/coast.rs src/generators/natural/surface_formation/generation.rs src/generators/natural/surface_formation/hillslope.rs src/generators/natural/surface_formation/hydrology.rs src/generators/natural/surface_formation/isostasy.rs src/generators/natural/surface_formation/mod.rs src/generators/natural/surface_formation/sediment.rs src/generators/natural/surface_formation/stream_power.rs src/generators/natural/terrain_amplification.rs src/ui/field/localization.rs src/world/natural/fields.rs src/world/natural/mod.rs src/world/natural/surface_formation.rs tests/formation_coast_isostasy.rs tests/formation_hillslope.rs tests/formation_hydrology.rs tests/formation_sediment.rs tests/formation_stream_power.rs tests/surface_formation_atlas.rs tests/surface_formation_contracts.rs tests/surface_formation_evidence.rs tests/surface_formation_generation.rs tests/surface_formation_performance.rs tests/surface_formation_quality.rs tests/surface_formation_stage.rs tests/terrain_audit_probe.rs
git diff --cached --name-only
git diff --cached --check
git commit -m "Land the inherited P5 publication baseline" -m "Make the existing R4 scientific and UI transaction work reproducible from its own commit before the contract-restoration tasks build on it."
```

Expected: staged name list 与本任务清单完全一致；提交后相关路径干净，未跟踪
`debug.log` 原字节仍在且未进入提交。

---

### Task 1: 把形成时间线提升为 resolved 输入事实源

**Files:**
- Modify: `src/world/natural/formation.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Modify: `src/generators/natural/spherical_tectonics/publication.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/mod.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/spreading.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/subduction.rs`
- Modify: `src/generators/natural/spherical_tectonics/forcing.rs`
- Modify: `src/generators/natural/evolved_tectonics.rs`
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
    assert_eq!(timeline.step_count(), SEKAI_REFERENCE_FORMATION_STEP_COUNT);
    assert_eq!(timeline.step_duration_kyr(), CORTIAL_FORMATION_STEP_DURATION_KYR);
    assert_eq!(
        timeline.total_duration_myr().to_bits(),
        (f64::from(SEKAI_REFERENCE_FORMATION_STEP_COUNT)
            * f64::from(CORTIAL_FORMATION_STEP_DURATION_KYR)
            / 1_000.0).to_bits(),
    );

    let encoded = serde_json::to_value(&formation).unwrap();
    assert_eq!(
        encoded["timeline"]["step_count"],
        SEKAI_REFERENCE_FORMATION_STEP_COUNT,
    );
    assert_eq!(
        encoded["timeline"]["step_duration_kyr"],
        CORTIAL_FORMATION_STEP_DURATION_KYR,
    );
}

#[test]
fn resolved_formation_rejects_a_forged_timeline() {
    let mut encoded = serde_json::to_value(
        resolve(42, WorldFormationPreset::Continents).formation(),
    )
    .unwrap();
    encoded["timeline"]["step_count"] =
        serde_json::json!(SEKAI_REFERENCE_FORMATION_STEP_COUNT - 1);
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

严格 unknown-field 拒绝由 private `ResolvedFormationTimelineWire: Deserialize` 与
`ResolvedWorldFormation` 的手写反序列化承担；不在只实现 `Serialize` 的生产类型上
放无效 serde 属性。测试只引用生产常量/访问器，不复制 `128`、`2_000` 或总时长
第二份事实。

把 `timeline` 嵌入 `ResolvedWorldFormation` 及 wire，`new` 固定写入 `sekai_reference()`，反序列化后同时校验。删除 runner 私有 `EVOLUTION_STEP_COUNT`/`EVOLUTION_DELTA_MYR`，并删除第三份时间真相
`spherical_tectonics::processes::constants::DEFAULT_DELTA_MYR`；两个 V4/V5 循环及
所有 process 调用都从本次 `formation.timeline()` 派生/显式传入 `delta_myr`。
`generate_evolved_spherical` 和 runner 的 formation 参数改为
`&ResolvedWorldFormation`，只在 recipe 选择处调用 `.resolved()`。
`forcing.rs` 的 metres-per-step→mm/year 换算显式接收同一
`timeline.step_duration_myr()`；`subduction.rs` 不再把每步 uplift 固化为含
`DEFAULT_DELTA_MYR` 的常量，而是保留有单位 rate 并在调用时乘本步时长；
`spreading.rs` 的测试/辅助调用也显式传入 resolved/test timeline 的时长。删除常量
后 `rg "DEFAULT_DELTA_MYR" src tests` 必须零命中。

- [ ] **Step 4: 运行 GREEN 与 P2 等价回归**

Run: `cargo test --test world_formation_spec`

Run: `cargo test --release --test evolved_tectonic_generation`

Run: `cargo test --lib spherical_tectonics::processes::`

Run: `cargo test --lib spherical_tectonics::forcing::`

Expected: 全部 PASS；固定 seed 的 P2 物理数组与预算不变。由于 timeline 新进入
resolved input 序列化身份，任何包含该输入或其 lineage 的 artifact/stage
fingerprint 必须按现有身份规则确定性刷新；不得要求旧指纹伪装不变。

- [ ] **Step 5: 提交**

```bash
git add src/world/natural/formation.rs src/world/natural/mod.rs src/generators/natural/spherical_tectonics/runner.rs src/generators/natural/spherical_tectonics/publication.rs src/generators/natural/spherical_tectonics/processes/mod.rs src/generators/natural/spherical_tectonics/processes/spreading.rs src/generators/natural/spherical_tectonics/processes/subduction.rs src/generators/natural/spherical_tectonics/forcing.rs src/generators/natural/evolved_tectonics.rs tests/world_formation_spec.rs tests/evolved_tectonic_generation.rs
git commit -m "Move formation timing into resolved world state" -m "Keep Sekai's authored horizon and Cortial's sourced step duration distinct inside validated input identity."
```

---

### Task 2: 建立不暴露兼容高程的借用型权威构造视图

**Files:**
- Modify: `src/world/natural/evolved_tectonics.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/geologic_substrate.rs`
- Test: `tests/geologic_substrate_generation.rs`

**Interfaces:**
- Consumes: `EvolvedTectonicSnapshot::{material,forcing,compatibility}`。
- Produces: crate-private
  `EvolvedTectonicSnapshot::authoritative_view() -> AuthoritativeTectonicView<'_>`；
  view 只提供 plates、plate ids、crust kind/thickness/age/lineation/orogeny、
  boundaries、material 和 forcing 的借用访问器，不提供
  `tectonic_elevation_m` 或完整 compatibility snapshot，不进入公共 API。

- [ ] **Step 1: 写 crate 内借用与不可见性 RED**

在 `evolved_tectonics.rs` 的所属模块单元测试验证零复制；测试与生产消费者都只能
经 `pub(crate)` 接缝访问，不为 compile-fail doctest 放宽产品可见性：

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

Run: `cargo test --lib authoritative_view_borrows_only_causal_fields`

Expected: 单元测试编译失败，指出 `authoritative_view` 尚不存在。

- [ ] **Step 3: 实现最小借用 view 并迁移 substrate**

核心类型固定为：

```rust
#[derive(Debug, Clone, Copy)]
pub(crate) struct AuthoritativeTectonicView<'a> {
    snapshot: &'a EvolvedTectonicSnapshot,
}

impl EvolvedTectonicSnapshot {
    pub(crate) const fn authoritative_view(&self) -> AuthoritativeTectonicView<'_> {
        AuthoritativeTectonicView { snapshot: self }
    }
}

impl<'a> AuthoritativeTectonicView<'a> {
    pub(crate) const fn surface_ref(self) -> SurfaceRef { self.snapshot.compatibility.surface_ref() }
    pub(crate) fn plates(self) -> &'a [SphericalPlate] { self.snapshot.compatibility.plates() }
    pub(crate) const fn cell_plates(self) -> &'a PlateIdField {
        self.snapshot.compatibility.cell_plates()
    }
    pub(crate) const fn material(self) -> &'a SphericalCrustMaterialState { &self.snapshot.material }
    pub(crate) const fn forcing(self) -> &'a SphericalTectonicForcingState { &self.snapshot.forcing }
    pub(crate) fn crust_kinds(self) -> &'a CrustKindField { self.snapshot.compatibility.crust_kinds() }
    pub(crate) fn crust_thickness_km(self) -> &'a [f32] {
        self.snapshot.compatibility.crust_thickness_km()
    }
    pub(crate) fn crust_age_myr(self) -> &'a [f32] { self.snapshot.compatibility.crust_age_myr() }
    pub(crate) fn lineation_east(self) -> &'a [f32] { self.snapshot.compatibility.lineation_east() }
    pub(crate) fn lineation_north(self) -> &'a [f32] { self.snapshot.compatibility.lineation_north() }
    pub(crate) fn orogeny_kind(self) -> &'a [SphericalOrogenyKind] {
        self.snapshot.compatibility.orogeny_kind()
    }
    pub(crate) fn orogeny_age_myr(self) -> &'a [f32] {
        self.snapshot.compatibility.orogeny_age_myr()
    }
    pub(crate) fn boundaries(self) -> &'a [BoundaryRecord] { self.snapshot.compatibility.boundaries() }
    pub(crate) fn boundary_segments(self) -> &'a [SphericalBoundarySegment] {
        self.snapshot.compatibility.boundary_segments()
    }
}
```

以上就是本任务允许的完整访问器集合；禁止返回 `&SphericalTectonicSnapshot`、`compatibility()` 或 `tectonic_elevation_m()`。`GeologicSubstrateGenerator::generate` 先取得 `let tectonic = evolved.authoritative_view();`，材料、forcing 和 crust 字段全部经该 view 读取。

- [ ] **Step 4: 运行 GREEN**

Run: `cargo test --release --lib authoritative_view_borrows_only_causal_fields`

Run: `cargo test --release --test geologic_substrate_generation`

Expected: 全部 PASS；crate-private 类型的完整访问器清单没有兼容高程入口，借用测试
证明没有新 snapshot/数组复制，外部 crate 无法获得该 view。

- [ ] **Step 5: 提交**

```bash
git add src/world/natural/evolved_tectonics.rs src/world/natural/mod.rs src/generators/natural/geologic_substrate.rs tests/geologic_substrate_generation.rs
git commit -m "Isolate authoritative tectonic inputs" -m "Give P3 a borrowed cause-only view that cannot expose compatibility elevation."
```

---

### Task 3: 让 P3 成为无历史权威投影并删除事后高程修形

**Files:**
- Modify: `src/generators/natural/primary_relief.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/spherical_island_relief.rs`
- Modify: `src/generators/natural/spherical_relief.rs`
- Modify: `src/generators/natural/spherical_relief/directed_noise.rs`
- Modify: `src/generators/natural/spherical_mantle.rs`
- Modify: `src/generators/natural/geologic_substrate.rs`
- Modify: `src/generators/natural/quality/primary_relief.rs`
- Modify: `src/world/natural/primary_relief.rs`
- Test: `tests/geologic_pipeline_contracts.rs`
- Test: `tests/primary_relief_atlas.rs`
- Test: `tests/primary_relief_evidence.rs`
- Test: `tests/primary_relief_generation.rs`
- Test: `tests/primary_relief_quality.rs`
- Test: `tests/terrain_audit_probe.rs`

**Interfaces:**
- Consumes: `AuthoritativeTectonicView<'_>`、`GeologicSubstrateSnapshot`、`ReliefSpec`。
- Produces: `PrimaryReliefGenerator::generate` 的签名保持不变；新增 crate-private
  `GeologicSubstrateGenerator::generate_from_streams` 与
  `PrimaryReliefGenerator::generate_working_from_streams` 供协调器复用同一随机身份；
  后者返回私有 `PrimaryReliefWorkingState`（各组成、完整高程与水面几何均为
  `f64`）及最终 snapshot 投影。内部所有构造读取改走 view；删除
  `dynamic_tectonic_response_m` 与 `DYNAMIC_RATE_RESPONSE_M_PER_MM_PER_YEAR`。

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

#[test]
fn p3_projection_does_not_integrate_current_tectonic_rates() {
    let fixture = authoritative_p3_fixture(RootSeed::new(42));
    let changed = fixture.with_only_current_forcing_rates_changed();
    let original = generate_p3_from_evolved(&fixture, &fixture.evolved);
    let projected = generate_p3_from_evolved(&fixture, &changed);
    assert_eq!(
        projected.exact.primary_elevation_m(),
        original.exact.primary_elevation_m(),
    );
    assert!(projected
        .exact
        .dynamic_tectonic_offset_m()
        .iter()
        .all(|value| value.to_bits() == 0.0_f64.to_bits()));
}
```

`authoritative_p3_fixture` 与 `generate_p3_from_evolved` 写在同一测试文件，直接调用 `GeologicSubstrateGenerator`、`PrimaryReliefGenerator` 和固定 `derive_stage_seed`；不得复制 P3 公式。
第二个 fixture 只变异 uplift/subsidence rate，保持 material、crust age、orogeny、
lineation、boundary kind 与所有随机标签不变。P3 的 passive-margin 分类改读上述
固体几何事实，不再把当前 forcing rate 当作地形高度或分类替身；因此完整 working
elevation 对只改当前率保持不变。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --test geologic_pipeline_contracts compatibility_elevation_alone_`

Expected: FAIL，当前大陆 compatibility elevation 和无出处的 current-rate 增益会
改变 `dynamic_tectonic_offset_m`/最终 relief。

- [ ] **Step 3: 删除兼容/当前率位移并建立 `f64` P3 working state**

删除 `DYNAMIC_ACCUMULATED_RESPONSE_WEIGHT`、
`DYNAMIC_RATE_RESPONSE_M_PER_MM_PER_YEAR`、`causal_accumulated_response_m` 与
`dynamic_tectonic_response_m`。P3 不再把 compatibility elevation 或当前 forcing
rate 变成 additive elevation；既有 `dynamic_tectonic_offset_m` wire 在本轮表示
“P3 没有独立动态位移”的精确零场，不能成为未来偷偷恢复经验项的接缝。
`quality/primary_relief.rs` 中
`subduction-negative-dynamic-fraction` 与
`convergent-positive-dynamic-fraction` 的被测对象因此按设计消失：删除这两个
metric、`EXPECTED_METRIC_NAMES` 条目及对应测试，不把它们重定向到 P5，也不以
零场继续执行原 `0.80` 硬门。其余 P3 质量门若失败，按 Step 4 报告真实的
物质/过程缺口，不调系数、不恢复旧项。

把 hotspot、passive-margin、conditioned-detail helper 参数改为
`AuthoritativeTectonicView<'_>` 或其最窄字段，并把会进入完整 P3 高程的中间数组
与返回值提升为 `f64`。删除 `HEIGHT_QUANTUM_M` 的科学量化、
`reconcile_primary_safety`、`constrain_regional_pair`、`adjust_component`、所有
结果 `.clamp(...)` 和 clamp diagnostic；每个具名组成及完整和越出其已有支持域
都返回包含原始 `f64` 值的 typed failure。只在 `PrimaryReliefWorkingState` 完成
校验后由唯一 `to_snapshot()` 做一次 `f32` wire 投影。

完整和只在一个 helper 中形成：

```rust
fn compose_primary_elevation(
    isostatic: &[f64],
    dynamic: &[f64],
    volcanic: &[f64],
    passive: &[f64],
    detail: &[f64],
) -> Result<Vec<f64>, PrimaryReliefGenerationError> {
    let mut elevation = Vec::with_capacity(isostatic.len());
    for index in 0..isostatic.len() {
        let exact = isostatic[index]
            + dynamic[index]
            + volcanic[index]
            + passive[index]
            + detail[index];
        if !exact.is_finite()
            || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&exact)
        {
            return Err(PrimaryReliefGenerationError::ElevationOutOfRange {
                cell: CellId::from_raw(index as u32),
                found: exact,
            });
        }
        elevation.push(exact);
    }
    Ok(elevation)
}
```

该 helper 是 P3 内部组成投影，不是第二份最终 P5 高程恒等式：P5 仍只通过
`formation_elevation_from_components` 把 `primary_elevation_m` 与另外八项相加。
`PrimaryReliefWorkingState` 是一次构建的私有中间态，不进入 serde/artifact/UI；
公开 `generate` 用它生成 snapshot，协调器/P5 则直接消费其 `f64` 值，绝不从
snapshot 的 `f32` wire 反读。

现有各物理分量自身有出处的输入域限制保持；任何生成结果超出分量或总高程 artifact 域都 typed fail，不对结果 clamp。另把 `MantleGenerator::generate_spherical_from_streams`、`GeologicSubstrateGenerator::generate_from_streams` 与 `PrimaryReliefGenerator::generate_working_from_streams` 定为 crate-private；现有 public `generate` 只负责 capture 一次 `LabeledSubstreams` 后转调，协调器则在最终 P3 投影时复用同一组标签身份。
删除对应旧 re-export 时同步修改 `src/generators/natural/mod.rs`。现有
`ReliefSpecArtifact` 仍留在 `relief_spec.rs`；本任务不移动 artifact 定义。

- [ ] **Step 4: 运行 GREEN 与 P3 质量否决门**

Run: `cargo test --release --test geologic_pipeline_contracts compatibility_elevation_alone_`

Run: `cargo test --release --test primary_relief_generation --test primary_relief_quality --test primary_relief_evidence --test primary_relief_atlas --test terrain_audit_probe`

Expected: 消融 PASS，两个失去物理被测对象的 dynamic-sign metric 已删除，其余 P3
质量/证据全 PASS。若其余 morphology envelope 失败，本任务停在 RED，输出失败指标
并按规格登记“物质/过程缺口”；禁止恢复兼容高程、恢复修形或调系数。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/primary_relief.rs src/generators/natural/spherical_island_relief.rs src/generators/natural/spherical_relief.rs src/generators/natural/spherical_relief/directed_noise.rs src/generators/natural/spherical_mantle.rs src/generators/natural/geologic_substrate.rs src/generators/natural/quality/primary_relief.rs src/generators/natural/mod.rs src/world/natural/primary_relief.rs tests/geologic_pipeline_contracts.rs tests/primary_relief_atlas.rs tests/primary_relief_evidence.rs tests/primary_relief_generation.rs tests/primary_relief_quality.rs tests/terrain_audit_probe.rs
git commit -m "Restore P3 as an authoritative projection" -m "Remove compatibility and unsourced rate-to-height inheritance, retain exact f64 working elevation, and reject unsupported values without post-hoc reshaping."
```

---

### Task 4: 拆分 P2 固体年龄推进与 legacy 表面响应

**Files:**
- Modify: `src/generators/natural/spherical_tectonics/processes/relaxation.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/mod.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Modify: `src/generators/natural/spherical_tectonics/forcing.rs`
- Modify: `src/world/natural/evolved_tectonics.rs`（仅把 forcing 越界诊断保留为舍入前 `f64`）
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
            && found.to_bits() == 500.25_f64.to_bits()
            && max.to_bits() == f64::from(MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR).to_bits()
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
    if !exact.is_finite()
        || !(0.0..=f64::from(MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR)).contains(&exact)
    {
        return Err(EvolvedTectonicValidationError::ForcingRateOutOfRange {
            field,
            cell,
            found: exact,
            max: f64::from(MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR),
        }
        .into());
    }
    Ok(exact as f32)
}

let rate = checked_forcing_rate(
    sample.anchor,
    "uplift_rate_mm_per_year",
    f64::from(uplift_step_m) * METRES_PER_STEP_TO_MM_PER_YEAR,
)?;
```

subduction subsidence、subduction uplift、collision shortening 与 collision uplift 四个写入点共用该 helper；`ForcingRateOutOfRange` 的 `found/max` 随之使用
`f64`，保证 exact 科学值在 wire 舍入前可诊断。这里的乘数 `1` 表示删除无权威
依据的兼容高程 modifier，不新增常量、新阈值或新方程。上面的越界测试锁定 typed
failure，且错误中的值没有被改成支持域上界。`EvolvedTectonicSnapshot` 既有
validator 构造该错误时同步使用 `f64::from(found)` 与
`f64::from(MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR)`；只扩展诊断精度，不改变
artifact 支持域或 wire forcing 类型。

- [ ] **Step 4: 运行 GREEN 与 V4/V5 回归**

Run: `cargo test --lib spherical_tectonics::`

Run: `cargo test --release --test evolved_tectonic_forcing --test evolved_tectonic_material --test evolved_tectonic_quality --test evolved_tectonic_evidence --test spherical_tectonic_generation`

Expected: 全部 PASS；V5 material budget 保持闭合，V4 compatibility 测试保持冻结。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/spherical_tectonics/processes/relaxation.rs src/generators/natural/spherical_tectonics/processes/mod.rs src/generators/natural/spherical_tectonics/runner.rs src/generators/natural/spherical_tectonics/forcing.rs src/world/natural/evolved_tectonics.rs tests/evolved_tectonic_forcing.rs tests/evolved_tectonic_material.rs tests/evolved_tectonic_quality.rs tests/evolved_tectonic_evidence.rs
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
- Produces: 生产仍只通过既有 one-shot P2 入口返回最终 `EvolvedTectonicSnapshot`；另有一个 `#[cfg(test)]`、crate-private 的 accepted-step observer 只供 Task 10 高成本耦合顺序探针使用，不返回历史集合。

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
借给 Task 10 reference closure。observer 不取得 workspace/ledger 可变引用，只在
accepted snapshot 的借用期内即时消费，不保存 snapshot/history 集合，不改变随机
流，也不进入 serde/artifact/UI。

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

### Task 6: 在 P5 核心迁移前归因完整 formation horizon 成本

**Files:**
- Modify: `src/generators/natural/surface_formation/generation.rs`（仅
  `#[cfg(test)]` ignored probe、生产构造 fixture 与即时 observer）
- Modify: `src/world/natural/surface_formation.rs`（恢复已冻结 horizon 的唯一常量）
- Modify: `src/world/natural/mod.rs`（恢复该常量的唯一 world re-export）
- Modify: `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`（只追加实测记录）

**Interfaces:**
- Consumes: 当前生产 `advance_geomorphic_window` 及 Draft/seed `42` 的真实
  P2/P3/start-P4 输入；`ClimateModelProfile` 固定为现有
  `ClimateModelProfile::C2LayeredV1`，不是新配置。
- Produces: world 层唯一 `SURFACE_FORMATION_HORIZON_YEARS = 100_000.0`（恢复
  HEAD/上位 P5 规格已有事实，不新钉数值）；以及一个 test-private、ignored 的
  one-advance observer，只即时记录每个
  accepted stable window 的物理时长/耗时，不保留 snapshot；输出
  `target/natural-quality/p5/pre-migration-one-advance.json` 与 blake3。

- [ ] **Step 1: 写证据文件契约 RED**

在 `generation.rs` 的 unit-test module 中新增 ignored probe，使用现有 production
constructors 组装 Draft/seed `42` 输入；测试 fixture 可复用构造流程但不得复制
科学公式。测试总是先运行一个短前缀并持久化成本估算；只有执行者根据该次机器、
编译档与可用研发资源确认可承受时，才从相同起点和 forcing 运行完整
`SURFACE_FORMATION_HORIZON_YEARS`。断言 JSON
明确包含 `requested_duration_years`、`accepted_duration_years`、accepted/rejected
window 数、wall time、逐窗口成本摘要、surface/profile/forcing fingerprints 和
完整/前缀成本估算；未执行完整 probe 时还必须包含前缀长度、预计资源和未完成
原因。文件不含 terrain/history 数组。预先将 retained state 或 horizon 替换为
f32/短时域都应让该契约失败。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --lib pre_migration_one_advance -- --ignored --nocapture`

Expected: 编译失败，test-private one-advance observer 尚不存在。

- [ ] **Step 3: 用生产算子实现最小测量外壳**

observer 只循环调用现有完整
hydrology→stream-power→hillslope→coast→sediment→Airy→water production window；
它可以收缩/拒绝候选并即时累计 accepted duration，但不得复制任一物理公式、
改变状态更新或把测量 API 编入非测试 library。先用相同稳定步策略跑短前缀并
记录投影成本；完整 `100,000 yr` 只在该次资源估算可承受时另行实际运行。
恢复常量时同时把它加回 `src/world/natural/mod.rs` 既有
`pub use surface_formation::{...}` 块；不得在 probe 或 generator 内再定义第二份
数值。
短前缀估算不能冒充完整结果，但资源不足也不能成为缩短 horizon 或改 profile 的
理由。
若完整运行因既有 typed scientific failure 结束，JSON 如实记录失败类型、已接受
时长和耗时，不得 clamp 或把失败伪成性能结果。

- [ ] **Step 4: 运行 probe 并在冻结 Tasks 7–9 策略前裁定**

Run: `cargo test --release --lib pre_migration_one_advance -- --ignored --nocapture`

Expected: 必定生成前缀估算证据；资源允许时另生成完整证据，并追加到规格实测
修订（回指规格 §0.1(9) 对 R3 §14.5 的显式替代）。分别报告单次 P5 advance、当前外层
重复 climate/PTC 和 upstream setup 的成本；不得把三者混写成“kernel 成本”。

- 若完整 one-advance 已在既有 Draft 预算内，Tasks 7–9 采用最小顺序实现。
- 若超预算，先按证据定位 kernel；只可在原方程、`100,000 yr`、守恒、九项组成
  与最终身份不变的前提下，选择该 kernel 已有出处支持的隐式下游栈、近似线性
  求解或多分辨率工作域，并在规格追加显式数值策略修订。多分辨率若没有直接
  对口出处必须标为类比/开放问题，不能仅凭性能落地。
- 不在本任务钉新误差阈值，不实现 predictor-corrector，也不缩短 horizon。

- [ ] **Step 5: 提交**

Task 0 已把 R4 生产前提落成可编译基线；本任务整文件暂存具名路径，不依赖工作树
中未提交的前置 hunk。

```bash
git add src/generators/natural/surface_formation/generation.rs src/world/natural/surface_formation.rs src/world/natural/mod.rs docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md
git diff --cached --check
git commit -m "Measure one P5 formation advance" -m "Attribute the full-horizon production cost before choosing the retained finite-time solver strategy."
```

---

### Task 7: 建立 P5 `f64` 九项因果地形组成状态

**Files:**
- Create: `src/generators/natural/surface_formation/state.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/surface_formation/hydrology.rs`
- Modify: `src/generators/natural/surface_formation/stream_power.rs`
- Modify: `src/generators/natural/surface_formation/sediment.rs`
- Modify: `src/generators/natural/surface_formation/hillslope.rs`
- Modify: `src/generators/natural/surface_formation/coast.rs`
- Modify: `src/generators/natural/surface_formation/isostasy.rs`
- Modify: `src/generators/natural/quality/surface_formation.rs`
- Modify: `src/generators/natural/global_circulation/forcing.rs`（同步九项测试 fixture）
- Modify: `src/app/spherical_formation_display.rs`（只迁移九项 payload，不提前切 bundle）
- Modify: `src/world/natural/surface_formation.rs`
- Modify: `src/world/natural/fields.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/ui/field/localization.rs`
- Test: `src/generators/natural/surface_formation/generation.rs`
- Test: `tests/formation_hydrology.rs`
- Test: `tests/formation_stream_power.rs`
- Test: `tests/formation_sediment.rs`
- Test: `tests/formation_hillslope.rs`
- Test: `tests/formation_coast_isostasy.rs`
- Test: `tests/surface_formation_generation.rs`
- Test: `tests/surface_formation_atlas.rs`
- Test: `tests/surface_formation_contracts.rs`
- Test: `tests/surface_formation_quality.rs`
- Test: `tests/surface_formation_evidence.rs`
- Test: `tests/surface_formation_stage.rs`
- Test: `tests/terrain_audit_probe.rs`
- Test: `tests/natural_field_registry_spherical.rs`
- Test: `tests/field_display_integration.rs`

**Interfaces:**
- Consumes: Task 3 的私有 `PrimaryReliefWorkingState`、现有两项 aggregate
  `FormationElevationComponents`（本任务原子迁移为九项）、
  P5 hydrology/process kernels 与 `LocalAiryIsostasy::response_from_validated_surface`。
- Produces: `FormationState::from_primary_working(&PrimaryReliefWorkingState) -> Result<Self, FormationStateError>`、九个具名 `apply_*_f64`/只读组成访问器、
  `current_elevation_exact_m(&self) -> &[f64]` 和
  `wire_components(&self) -> Result<FormationElevationComponents, FormationStateError>`；
  `pub(super) from_legacy_primary_wire_for_migration(&PrimaryReliefSnapshot)` 只有
  Task 7–10 的旧独立 P5 wrapper 一个消费者，并与该 wrapper 在 Task 11 同时删除；
  `#[cfg(test)] pub(super) from_primary_values(Vec<f64>)` 只供解析测试，
  `#[cfg(test)] replace_primary_for_offline_reference(...)` 只服务 Task 10 的单一高成本
  参考。所有 retained state 与所有会反馈后续状态的 kernel 输入/输出均为 `f64`；
  不提供 `current_elevation_f32` 科学入口或 f32 位移写入口。

- [ ] **Step 1: 写亚 ULP、test-only 参考差量和真实越界 RED**

在 `state.rs` 单元测试固定三个解析契约：

```rust
#[test]
fn sub_ulp_surface_changes_accumulate_without_f32_feedback() {
    let mut state = FormationState::from_primary_values(vec![9_000.0]).unwrap();
    state.apply_fluvial_erosion_f64(&[0.0003]).unwrap();
    state.apply_fluvial_erosion_f64(&[0.0003]).unwrap();
    assert_eq!(state.fluvial_erosion_m()[0].to_bits(), 0.0006_f64.to_bits());
    assert!(state.current_elevation_exact_m()[0] < 9_000.0);
}

#[test]
fn offline_reference_primary_replacement_preserves_accumulated_components() {
    let mut state = FormationState::from_primary_values(vec![100.0]).unwrap();
    state.apply_routed_sediment_deposition_f64(&[12.0]).unwrap();
    state.replace_primary_for_offline_reference(&[100.0], &[130.0]).unwrap();
    assert_eq!(state.primary_elevation_m(), &[130.0]);
    assert_eq!(state.routed_sediment_deposition_m(), &[12.0]);
    assert_eq!(state.current_elevation_exact_m(), &[142.0]);
}

#[test]
fn exact_f64_state_rejects_a_true_overflow_before_wire_rounding() {
    let mut state = FormationState::from_primary_values(vec![f64::from(ELEVATION_MAX_M)]).unwrap();
    assert!(matches!(
        state.apply_tectonic_displacement_f64(&[0.000_01]),
        Err(FormationStateError::ElevationOutOfRange { found, .. }) if found > 9_000.0
    ));
}

#[test]
fn every_scientific_kernel_reads_exact_f64_elevation() {
    let mut state = FormationState::from_primary_values(vec![9_000.0]).unwrap();
    state.apply_fluvial_erosion_f64(&[0.0003]).unwrap();
    let observed = probe_all_surface_kernel_elevation_inputs(&state).unwrap();
    assert!(observed.into_iter().all(|value| value.to_bits()
        == state.current_elevation_exact_m()[0].to_bits()));
}
```

`replace_primary_for_offline_reference` 必须受 `#[cfg(test)]` 约束，并只校验/应用
新旧 P3 primary 的 `f64` 差量以保留参考路径的 P5 累计状态；生产 Lie-style 路径由
协调器以最终 P3 working state 恰好调用一次 `from_primary_working`，不得暴露或调用该替换入口；最终
协调器生产入口不接受 `PrimaryReliefSnapshot`，从类型上阻止 `f32` wire 回流。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --lib surface_formation::state::tests`

Expected: 编译失败，`FormationState` 尚不存在。

- [ ] **Step 3: 抽取现有 `ComponentState` 并锁定唯一 wire 边界**

```rust
pub(in crate::generators::natural) struct FormationState {
    primary_elevation_m: Vec<f64>,
    tectonic_displacement_m: Vec<f64>,
    fluvial_erosion_m: Vec<f64>,
    hillslope_erosion_m: Vec<f64>,
    hillslope_deposition_m: Vec<f64>,
    routed_sediment_deposition_m: Vec<f64>,
    coastal_erosion_m: Vec<f64>,
    coastal_deposition_m: Vec<f64>,
    isostatic_response_m: Vec<f64>,
    current_elevation_m: Vec<f64>,
}

impl FormationState {
    fn rebuild_and_validate(&mut self) -> Result<(), FormationStateError> {
        for index in 0..self.current_elevation_m.len() {
            let exact = formation_elevation_from_components(
                self.primary_elevation_m[index],
                self.tectonic_displacement_m[index],
                self.fluvial_erosion_m[index],
                self.hillslope_erosion_m[index],
                self.hillslope_deposition_m[index],
                self.routed_sediment_deposition_m[index],
                self.coastal_erosion_m[index],
                self.coastal_deposition_m[index],
                self.isostatic_response_m[index],
            );
            if !exact.is_finite()
                || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&exact)
            {
                return Err(FormationStateError::ElevationOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: exact,
                });
            }
            self.current_elevation_m[index] = exact;
        }
        Ok(())
    }
}
```

`wire_components` 是唯一 `f64→f32` 组成转换，并复用
`formation_elevation_from_components` 验证发布恒等式。删除 `generation.rs` 私有
两项 `ComponentState`；hydrology、stream power、sediment、hillslope、coast 与
isostasy 的 elevation/displacement 输入和影响 retained state 的输出一起迁移为
`f64`。不得保留给“现有 kernel”读取的 `elevation_f32` scratch；需要 GPU/wire 的
量化只发生在最终候选全部接受之后。九项 field/accessor 名沿用现有 UI/field
registry，不创建 aggregate 替代品。所有现存 `primary_relief_m`/
`equilibrium_adjustment_m` 消费者在本任务原子迁移：质量/atlas/probe 改为九项
因果账本，field registry 和 localization 恢复九项累计 field id/label，display
payload 绑定九项 wire slice。Task 11 只把 payload 来源原子切到 bundle，不再
改变 field schema 或文案。

`formation_elevation_from_components` 本任务提升为九个 `f64` 参数并返回 `f64`；
其唯一生产文档注释固定符号约定：`tectonic_displacement_m` 与
`isostatic_response_m` 是有符号位移并相加；fluvial/hillslope/coastal erosion
保存非负侵蚀深度并相减；hillslope/routed-sediment/coastal deposition 保存非负
沉积厚度并相加。测试和 UI 只引用该 helper/字段注册表，不另写第二份公式。
最终 wire validator 把每个 `f32` 以 `f64::from` 扩展后仍调用这一实现，不新增
第二份求和公式。`PrimaryReliefSnapshot` 的 `f32` elevation 在最终协调器路径只用于
已接受 snapshot/呈现，不得传入 `FormationState`。

为让 Task 7–10 的旧独立 P5 stage 在原子 bundle 切换前保持逐提交可编译，
`from_legacy_primary_wire_for_migration` 只把该旧 stage 已有的 primary wire 一次扩展为
`f64`，随后所有 P5 retained state 与 kernel 仍全程 `f64`。这是对进入本轮前精度
基线的窄迁移桥，不是最终科学入口，不进入 artifact/serde/cache，不允许协调器调用，
也不授权任何 P5 子步从 wire 回读；Task 11 必须与旧 stage/wrapper 一起删除它。
`FormationState` 及协调器所需方法使用
`pub(in crate::generators::natural)`，并由 `surface_formation/mod.rs` 以相同最小
可见性 re-export，避免跨模块接口暴露更私有类型；migration 构造器仍保持
`pub(super)`，不能被 sibling 协调器取得。

两项→九项属于 wire schema 变化：把
`FORMATION_TERRAIN_FIELDS_SCHEMA_V3` 升为下一版本，并同步手写 wire、validator、
snapshot/model fingerprint、world re-export 与所有 serde/unknown-field 测试。field
registry 只保留规格冻结的 `primary_elevation_m` id；删除当前 aggregate 时代的
`primary_relief_m` id 而不留双写 alias，其余八项沿用历史稳定 id，所有本地化由
注册表 SSOT 驱动。

- [ ] **Step 4: 运行 GREEN 与原失败回归**

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Run: `cargo test --release --test formation_hydrology --test formation_stream_power --test formation_sediment --test formation_hillslope --test formation_coast_isostasy`

Run: `cargo test --release --test surface_formation_generation --test surface_formation_atlas --test surface_formation_contracts --test surface_formation_quality --test surface_formation_evidence --test surface_formation_stage --test terrain_audit_probe --test natural_field_registry_spherical --test field_display_integration`

Expected: 全部 PASS；`9000.000260834617` 一类 f32 身份误报不再出现，真实越界仍返回未裁剪 `f64`。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/surface_formation/state.rs src/generators/natural/surface_formation/mod.rs src/generators/natural/surface_formation/generation.rs src/generators/natural/surface_formation/hydrology.rs src/generators/natural/surface_formation/stream_power.rs src/generators/natural/surface_formation/sediment.rs src/generators/natural/surface_formation/hillslope.rs src/generators/natural/surface_formation/coast.rs src/generators/natural/surface_formation/isostasy.rs src/generators/natural/quality/surface_formation.rs src/generators/natural/global_circulation/forcing.rs src/app/spherical_formation_display.rs src/world/natural/surface_formation.rs src/world/natural/fields.rs src/world/natural/mod.rs src/ui/field/localization.rs tests/formation_hydrology.rs tests/formation_stream_power.rs tests/formation_sediment.rs tests/formation_hillslope.rs tests/formation_coast_isostasy.rs tests/surface_formation_generation.rs tests/surface_formation_atlas.rs tests/surface_formation_contracts.rs tests/surface_formation_quality.rs tests/surface_formation_evidence.rs tests/surface_formation_stage.rs tests/terrain_audit_probe.rs tests/natural_field_registry_spherical.rs tests/field_display_integration.rs
git diff --cached --check
git commit -m "Retain causal formation components in f64" -m "Preserve all nine final-state causes and prevent wire precision from feeding any scientific kernel."
```

---

### Task 8: 把沉积库存改为 `f64` 质量事实源

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
    pub(super) fn apply_transfer(
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
git add src/generators/natural/surface_formation/state.rs src/generators/natural/surface_formation/sediment.rs src/generators/natural/surface_formation/hillslope.rs src/generators/natural/surface_formation/coast.rs src/generators/natural/surface_formation/generation.rs tests/formation_sediment.rs tests/formation_hillslope.rs tests/formation_coast_isostasy.rs
git diff --cached --check
git commit -m "Retain sediment inventory as f64 mass" -m "Make five-source solid stock the cross-step truth and keep thickness and fractions as validated wire projections."
```

---

### Task 9: 提取有限物理时间 P5 步进并退役外层绝对稳态求根

**Files:**
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/surface_formation/stream_power.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/generators/natural/quality/surface_formation.rs`
- Modify: `src/world/natural/surface_formation.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `src/generators/natural/surface_formation/generation.rs`
- Test: `tests/formation_stream_power.rs`
- Test: `tests/surface_formation_contracts.rs`
- Test: `tests/surface_formation_generation.rs`
- Test: `tests/surface_formation_evidence.rs`
- Test: `tests/surface_formation_quality.rs`
- Test: `tests/surface_formation_performance.rs`
- Test: `tests/surface_formation_stage.rs`

**Interfaces:**
- Consumes: `FormationState`、最终 `EvolvedTectonicSnapshot`、当前 P4、P5
  hydrology/stream-power/hillslope/coast/sediment/Airy kernel。
- Consumes: Task 6 从 HEAD 恢复的 world 层
  `SURFACE_FORMATION_HORIZON_YEARS`（P5 coarse-grained horizon，不从 P2
  timeline 派生）。
- Produces: `advance_surface_processes(state, inputs, duration_years, cancellation) -> SurfaceAdvanceReport` 恰好消费请求物理时长，并把最终 P2 构造率在该时长内积分一次；`recompute_surface_diagnostics(state, endpoint_inputs, cancellation) -> TerminalSurfaceDiagnostics` 在零时间推进下重算终点水文/过程率；`finalize_surface_formation(..., upstream, ...) -> NaturalSurfaceFormationSnapshot` 是唯一纯 P5 wire 发布入口。Task 9 先把 checkpoint 改为最终 climate 身份；现有嵌套 climate 只作为编译期过渡保留到 Task 11 的 bundle/UI/T1 原子迁移，不新增第二份写入路径。

- [ ] **Step 1: 写有限时间与无双计数 RED**

在 `generation.rs` 的 `#[cfg(test)]` 模块写测试，以便直接调用最小可见性的 state/advance functions；integration test 不放宽生产可见性：

```rust
#[test]
fn surface_step_consumes_the_complete_requested_duration() {
    let fixture = surface_formation_fixture();
    let mut state = FormationState::from_primary_working(fixture.primary_working()).unwrap();
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
fn zero_tectonic_rate_does_not_reapply_final_p3_displacement() {
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
fn final_p2_rate_is_integrated_once_over_the_p5_horizon() {
    let uplift_rate = 0.25_f32;
    let subsidence_rate = 0.05_f32;
    let fixture = constant_tectonic_rate_fixture(uplift_rate, subsidence_rate);
    let mut state = FormationState::from_primary_values(vec![125.0]).unwrap();
    advance_surface_processes(
        &mut state,
        fixture.inputs(),
        10_000.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    let expected_m = (f64::from(uplift_rate) - f64::from(subsidence_rate))
        / 1_000.0
        * 10_000.0_f64;
    assert_eq!(
        state.tectonic_displacement_m()[0].to_bits(),
        expected_m.to_bits(),
    );
    assert_eq!(
        state.current_elevation_exact_m()[0].to_bits(),
        (125.0 + expected_m).to_bits(),
    );
}

#[test]
fn transitional_surface_snapshot_binds_the_endpoint_climate() {
    let (snapshot, endpoint_climate) = finalized_surface_fixture();
    assert_eq!(
        snapshot.checkpoint().upstream().formation_climate_checkpoint_fingerprint(),
        endpoint_climate.checkpoint().fingerprint(),
    );
    assert_eq!(
        snapshot.formation_climate().checkpoint().fingerprint(),
        endpoint_climate.checkpoint().fingerprint(),
    );
}
```

同一 test module 增加 `zero_surface_process_fixture()` 与
`constant_tectonic_rate_fixture(...)`，复用已有 production fixture 的
surface/tectonics/substrate/climate/spec，只通过现有 snapshot constructors 构造
零降水、零 active surface-water 和零 erodibility 条件；不复制 stream-power
或构造位移方程。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Expected: 编译失败；现有入口只有 PTC absolute fixed-point solve。

- [ ] **Step 3: 建立只含表面过程的完整时长推进**

固定接口：

```rust
pub(in crate::generators::natural) struct SurfaceProcessInputs<'a> {
    pub surface: &'a SphericalSurfaceSnapshot,
    pub tectonics: &'a EvolvedTectonicSnapshot,
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
    endpoint_climate: GlobalCirculationSnapshot,
    upstream: SurfaceFormationUpstreamFingerprints,
    terminal_diagnostics: TerminalSurfaceDiagnostics,
    evolution_report: FormationEvolutionReport,
    cancellation: &BuildCancellation,
) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError>;
```

`endpoint_climate` 是 Task 9 的显式过渡所有权参数，checkpoint 必须来自同一个
对象；Task 11 创建 sibling bundle 时删除该参数和嵌套字段，不能在两处独立重建。

实现以 `remaining_years` 循环；每次取
`min(remaining_years, maximum_stable_step_years, maximum_elevation_domain_step_years)`，
在 clone 候选上跑完整 hydrology→stream power→hillslope→coast→sediment→Airy→water
验证，成功才扣减 remaining。`ImplicitStreamPowerSolver` 把最终 P2 uplift/subsidence
率先以 `f64::from` 扩展为该调用窗口的常量 forcing。每次调用 advance 时记录
入口 `tectonic_displacement_m` 为 base；每个被接受候选按累计
`accepted_duration_years` 直接形成
`target = base + (uplift - subsidence) / 1000 × accepted_duration_years`，以 target
替换候选的构造组成，而不是逐子步执行 `+= rate × dt`。这样稳定子步仍看到当前
累计强迫，拒绝候选不污染 base，最终值又与子步划分无关；它是分段常量 forcing
在该调用窗口内的解析积分，不新增容差。构造组成与 fluvial displacement 分别写入
`FormationState`。该积分不得读取
`compatibility.tectonic_elevation_m`，也不得重新加入 final P3 elevation；构造
位移累计恰好一次。终点 current rate 继续报告相同 P2 forcing，但零时间诊断不
增加 `tectonic_displacement_m`。

`accepted_duration_years` 的最终报告值以及上述解析构造位移使用
`requested_duration_years - remaining_years`；完整成功时显式归一为原请求时长，
不通过逐子步浮点求和另造时间身份。这样 Step 1 的位精确时长契约不依赖稳定子步
是否能用二进制精确表示。

`recompute_surface_diagnostics` 只在终点 P4 下重建 hydrology 和瞬时过程率，
不得调用任何会改变 elevation component、sediment inventory、water reservoir
或累计时长的推进算子。增加前后完整状态字节/指纹相等测试，防止“诊断重算”
偷偷形成第三个 P5 半步。

删除 `solve_geomorphic`、`generate_with_climate_solve_limit`、
`EquilibriumOutsideElevationDomain` 和 `NotConverged` 生产路径。同步删除
`SURFACE_FORMATION_MAX_CLIMATE_SOLVES`、
`SURFACE_FORMATION_CONTINUATION_STEPS_PER_CLIMATE_SOLVE`、
`SURFACE_FORMATION_MAX_EQUILIBRIUM_ITERATIONS`、
`SURFACE_FORMATION_CONTINUATION_GROWTH_FACTOR`、
`FORMATION_EQUILIBRIUM_RELATIVE_RESIDUAL_MAX`，以及
`PriorityFloodFastscapeDavyLagueHillslopeCoastIsostasyEquilibriumV3`/pseudo-transient
model tag 与 fingerprint 输入。把 `FormationSolveReport` 替换为：

```rust
pub struct FormationEvolutionReport {
    accepted_surface_substeps: u32,
    integrated_duration_years: f64,
    current_rates: FormationResiduals,
    dense_state_bytes: u64,
}
```

`NaturalSurfaceFormationSnapshot` 中原 `solve_report` 字段同步改名为
`evolution_report: FormationEvolutionReport`，并只读公开 `evolution_report()`；
为保证本提交独立编译，`formation_climate` 字段及现有访问器只过渡保留到
Task 11，且必须与上述最终 checkpoint 逐指纹相等。Task 11 在 sibling P4 可供所有
消费者原子替换时删除该字段，不允许这个过渡进入最终 bundle schema。
现有 `SurfaceFormationGenerator::generate` 同样只过渡保留到 Task 11，作为旧独立
stage 的唯一编译桥：它必须把输入 `initial_climate` 当作 start P4，执行一次完整
有限时间 advance，再从最终 terrain/`SurfaceWaterGeometry` 调用既有 forcing builder
和 P4 solver 得到 endpoint climate，零时间重算诊断后才 finalize 上述过渡 snapshot；
不得把 start climate 冒充 endpoint。Task 11 在 causal coordinator/bundle 成为唯一
生产入口时连同旧 stage 删除该 wrapper，不能长期保留第二条 P4/P5 编排路径。
`SurfaceFormationUpstreamFingerprints` 的
`initial_climate_checkpoint_fingerprint` 改名为
`formation_climate_checkpoint_fingerprint`。P2 accepted step 与 P4 solve
count 只由协调器/性能证据统计，不进入 P5 report；不在协调器 output 或 bundle
顶层复制 `FormationEvolutionReport`。`current_rates` 是诊断；删除
`FormationResiduals::normalized_max()`，不再除以退役的
`FORMATION_EQUILIBRIUM_RELATIVE_RESIDUAL_MAX`。需要保留的证据分别读取既有
具名原始量/无阈值比值，不再构造一个新的无出处 max aggregate；schema 和
`surface_formation_model_fingerprint`
同步升版。内部 P4 仍可使用既有快平衡求解，不把 PTC 扩回外层地貌。
`surface_formation_state_fingerprint` 同步删除 climate 参数，只散列实际保留
的 P5 terrain/process/hydrology；P4 绑定只由 checkpoint 的 upstream 指纹承担，
不得在 state fingerprint 内暗中保留第二份气候所有权。

`quality/surface_formation.rs` 同步删除
`equilibrium-current-flux-residual` 的 `max <= 1` release gate。若当前 UI/研发证据
仍消费当前过程信息，则以各具名 `m/year`、`kg/year` 或现有无阈值 flux ratio
分别记录 observation；若没有真实消费者则删除 metric，不创建兼容别名或新的
max aggregate。所有旧
`solve_report()`/`EquilibriumV3` 调用点、序列化字段、错误码、测试名与 UI label
一并迁移，避免“有限时间实现、稳态质量门”双重语义。

- [ ] **Step 4: 运行 GREEN 与数值稳定族**

Run: `cargo test --release --lib generators::natural::surface_formation::generation::tests`

Run: `cargo test --release --test formation_stream_power --test surface_formation_contracts --test surface_formation_generation --test surface_formation_evidence --test surface_formation_quality --test surface_formation_performance --test surface_formation_stage`

Expected: 全部 PASS；有限时间测试允许非零 `dh/dt`，只要求归因、守恒、时长完整和数值稳定；终点诊断不改变状态。过渡 P5 snapshot 的 climate 与 checkpoint
严格一致；真正去嵌套由 Task 11 与 sibling 消费者一起原子完成。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/surface_formation/generation.rs src/generators/natural/surface_formation/stream_power.rs src/generators/natural/surface_formation/mod.rs src/generators/natural/quality/surface_formation.rs src/world/natural/surface_formation.rs src/world/natural/mod.rs tests/formation_stream_power.rs tests/surface_formation_contracts.rs tests/surface_formation_generation.rs tests/surface_formation_evidence.rs tests/surface_formation_quality.rs tests/surface_formation_performance.rs tests/surface_formation_stage.rs
git diff --cached --check
git commit -m "Advance P5 over finite physical time" -m "Retire the global absolute-landscape root while integrating held tectonic forcing once with attributed surface processes."
```

---

### Task 10: 实现领域因果协调器

**Files:**
- Create: `src/generators/natural/causal_formation.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/global_circulation/forcing.rs`
- Modify: `src/world/natural/formation.rs`（只增加 `#[cfg(test)]` schedule constructor）
- Test: `src/generators/natural/causal_formation.rs`

**Interfaces:**
- Consumes: Tasks 1–9 的 timeline、P2 one-shot generator、P3 投影、P4 solver、`FormationState`、`advance_surface_processes` 和 `recompute_surface_diagnostics`；test-only reference 另消费 Task 5 observer。
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
    let components = output.surface.terrain_fields().elevation_components();
    assert!(components.tectonic_displacement_m().iter().any(|value| *value != 0.0));
    assert_eq!(
        components.primary_elevation_m(),
        output.primary_relief.elevation_m(),
    );
    assert!(components.validate_identity().is_ok());
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
2. 从最终 `AuthoritativeTectonicView` 生成一次 substrate/P3 私有
   `PrimaryReliefWorkingState`，从其 `f64` 完整高程初始化 `FormationState`，并
   单独生成只供最终发布的 P3 snapshot；不得从 snapshot 反读 `f32`；
3. 从该完整 terrain/`SurfaceWaterGeometry` 重建 forcing 并求 start P4；
4. `advance_surface_processes` 以现有稳定子步恰好消费
   `SURFACE_FORMATION_HORIZON_YEARS`，显式借用 final
   `EvolvedTectonicSnapshot` 并把其当前 uplift/subsidence forcing 积分一次；该值
   属于 P5，不从 P2 timeline 派生；
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
start/endpoint P4 均固定调用现有 `ClimateModelProfile::C2LayeredV1`；它是模型
身份而不是本任务新旋钮。
同步删除 `global_circulation/forcing.rs` 中全部三处已失真的
`#[allow(dead_code)] // consumed by the P5 compositor added in Task 7`；这些入口已有
当前生产消费者，不保留过期 suppressor/任务号注释。

- [ ] **Step 4: 运行 GREEN 的短前缀解析/确定性测试和离线参考探针**

Run: `cargo test --release --lib generators::natural::causal_formation::tests`

Expected: 全部 PASS；两步 fixture 的 P2 accepted 顺序、单次 P3/P5、两次外层
P4、终点闭合和拒绝原子性成立。

同一模块增加 `#[cfg(test)]` 且 ignored 的耦合顺序敏感性探针
`compare_production_split_with_high_cost_reference`。它固定使用生产
`Standard`/seed `42`、相同 resolved inputs 和标签子流，分别运行：

- 生产路径：完整 P2 → 最终 P3 → start P4 → 一次完整 P5 → endpoint P4；
- 高成本耦合顺序对照：每个 P2 宏步生成对应 P3 组成变化，把
  `SURFACE_FORMATION_HORIZON_YEARS / timeline.step_count()` 的 P5 时长分配给
  该窗口，并执行 start P4 → half P5 → midpoint P4 → half P5 → endpoint P4。
  所有窗口的 P5 时长之和必须与生产路径相同；不得让 P5 跟随 P2 累计成
  `256 Myr`。

参考路径的构造账本与生产路径采用相同所有权：P3 primary replacement 只把该
accepted solid state 的物质/厚度/年龄/造山/具名 P3 投影更新到
`primary_elevation_m`，绝不写 `tectonic_displacement_m`；每个参考窗口的 P5
只以该 accepted P2 snapshot 的 uplift/subsidence rate 对分配到的 P5 时长做零阶
保持积分，并从调用入口 base 解析形成构造组成。两项分别属于“当前固体几何投影”
与“随后 P5 有限时域的外部强迫”，不得把同一 P3 差量同时记进构造组成。由于
P2/P5 物理时域仍不同，该限制只消除实现中的双计数/表示混淆，不把对照升级为
预测性轨迹参考。

高成本对照 observer 在每个 accepted P2 snapshot 的借用期内立即完成该窗口的 P3/P4/P5
消费，不克隆或保留 snapshot/history 集合。参考 helper 和 trace 全部保持
test-private，不进入 library API、serde、artifact 或 cache。两条路径只输出最终
九项 exact elevation components、
`SurfaceWaterGeometry`、五来源 sediment mass、water reservoirs、climate fields、
既有 quality metrics、守恒残差、wall time、peak RSS 与相关
checkpoint/forcing/bundle fingerprints 到
`target/natural-quality/causal-formation/offline-reference-standard-seed-42.json`
及 blake3。不得生成 `2/1/0.5 Myr` 路径、差值比或新通过阈值。
由于 P2/P5 时域不同，这条对照只测耦合顺序敏感性，不称为同一 `Δt` 下的高精度
参考解，也不据此声明全轨迹收敛。

先以相同 `2 Myr` 步长运行 `Standard`/seed `42` 的短前缀，记录每窗口耗时、峰值
内存并估算完整 `128` 窗口资源；该前缀只估成本，不替代代表性完整参考。估算在
当前机器可承受时再执行完整 ignored probe；若不可承受，仍保留同一代表性 probe
代码并在 JSON 记录前缀长度、估算方法、预计资源和未完成原因，不换成低 profile
或不同 seed 冒充参考。

Run: `cargo test --release --lib estimate_high_cost_reference_from_short_prefix -- --ignored --nocapture`

Run when the recorded estimate is feasible:
`cargo test --release --lib compare_production_split_with_high_cost_reference -- --ignored --nocapture`

Expected: 前缀估算必定生成非空 JSON；资源允许时完整 probe 另生成完整比较。
两条路径分别消费相同的 P2 timeline 和
相同的 P5 horizon，并共享输入/随机身份，各自满足硬不变式；文件记录原始最终态
差值与成本且不进入 artifact。该
命令只在耦合策略改变或明确的离线研发复核时运行，不属于常规构建/CI 门。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/causal_formation.rs src/generators/natural/mod.rs src/generators/natural/global_circulation/forcing.rs src/world/natural/formation.rs
git commit -m "Coordinate causal natural formation" -m "Run one production split over the resolved timeline and close the published climate against final terrain."
```

---

### Task 11: 原子建立 bundle、去嵌套 P4 并迁移生产/UI/T1 消费者

**Files:**
- Create: `src/world/natural/formation_bundle.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/world/natural/surface_formation.rs`
- Modify: `src/world/natural/fields.rs`
- Create: `src/generators/natural/causal_formation_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/surface_formation/state.rs`
- Modify: `src/generators/natural/surface_formation/mod.rs`
- Modify: `src/generators/natural/surface_formation/generation.rs`
- Delete: `src/generators/natural/surface_formation_stage.rs`
- Modify: `src/generators/natural/terrain_amplification.rs`
- Modify: `src/generators/natural/hierarchical_derivation.rs`
- Modify: `src/generators/natural/quality/global_circulation.rs`
- Modify: `src/generators/natural/quality/mod.rs`
- Modify: `src/app/spherical_presentation.rs`
- Modify: `src/app/spherical_formation_display.rs`
- Modify: `src/app.rs`
- Modify: `src/engine/cache.rs`
- Modify: `src/ui/field/localization.rs`
- Create: `tests/support/causal_formation.rs`
- Modify: `tests/support/mod.rs`
- Test: `tests/geologic_pipeline_contracts.rs`
- Test: `tests/causal_formation_generation.rs`
- Test: `tests/causal_formation_performance.rs`
- Test: `tests/natural_field_registry_spherical.rs`
- Test: `tests/field_display_integration.rs`
- Test: `tests/spherical_presentation_integration.rs`
- Test: `tests/surface_formation_atlas.rs`
- Test: `tests/surface_formation_evidence.rs`
- Test: `tests/surface_formation_generation.rs`
- Test: `tests/surface_formation_performance.rs`
- Test: `tests/surface_formation_quality.rs`
- Delete: `tests/surface_formation_stage.rs`
- Test: `src/app.rs`（`natural_app_tests`）
- Modify: `docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md`（只追加实测修订）

**Interfaces:**
- Consumes: `CausalFormationOutput`、现有 P2/P3/P4/P5 `NaturalQualityReport`
  evaluator、Task 9 的过渡 snapshot 以及现有 field registry/localization。
- Produces: `NaturalFormationBundle`、`NaturalFormationBundleArtifact`、
  `CausalNaturalFormationStage`、`causal_natural_formation_graph()`；
  `SphericalFormationFieldDocument`、T1 与 app 只从同一 bundle/sibling 读取，最终
  `NaturalSurfaceFormationSnapshot` 不含 climate。

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
    fn assert_no_solver_history(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, child) in fields {
                    assert!(
                        !["history", "checkpoints", "pseudo_time", "rejected_steps"]
                            .contains(&key.as_str()),
                        "forbidden solver-history key at any nesting depth: {key}",
                    );
                    assert_no_solver_history(child);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    assert_no_solver_history(item);
                }
            }
            _ => {}
        }
    }

    let json = serde_json::to_value(bundle).unwrap();
    assert_no_solver_history(&json);
    assert!(json["surface_formation"].get("formation_climate").is_none());
}

#[test]
fn formation_document_reads_siblings_from_the_same_bundle() {
    let outcome = causal_formation_outcome(RootSeed::new(42));
    let document = SphericalFormationFieldDocument::from_build_outcome(&outcome).unwrap();
    let bundle = outcome.artifacts.get::<NaturalFormationBundleArtifact>().unwrap();
    assert_eq!(document.formation_snapshot(), bundle.bundle().surface_formation());
    assert_eq!(document.substrate(), bundle.bundle().substrate());
    assert_eq!(document.formation_climate(), bundle.bundle().climate());
    assert_eq!(document.evolved_compatibility(), bundle.bundle().tectonics().compatibility());
}
```

app 单元测试另断言 build outcome 缺少旧 `NaturalSurfaceFormationArtifact` 仍可成功
安装；缺少 bundle 必须失败且不提交 world/cache/GPU。旧独立 P5 stage/test 在同一
RED 中被删除，避免无 sibling P4 的 snapshot 继续成为可调用生产入口。把旧
`surface_formation_stage` 中 T1 测试迁入 `geologic_pipeline_contracts`，直接用
同一 bundle 的 `surface_formation()` 与 `climate()` 构造 production
`HierarchicalEvaluator`；不为测试新增 lineage accessor。

- [ ] **Step 2: 运行 RED**

Run: `cargo test --release --test geologic_pipeline_contracts --test field_display_integration`

Expected: 编译失败，bundle/stage/graph 尚不存在，document/T1 仍从分散 artifact 或
P5 内嵌 climate 读取。

- [ ] **Step 3: 实现 world bundle 与原子 artifact**

```rust
pub const NATURAL_FORMATION_BUNDLE_SCHEMA_V1: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
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

另定义字段完全相同、带 `#[serde(deny_unknown_fields)]` 的 private
`NaturalFormationBundleWire: Deserialize`，手写
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

`final_climate_forcing` 必须是 Task 10 从 final formation terrain/
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

`CausalNaturalFormationStageInputs` 的 dependencies 固定为 profile、resolved tectonic/world formation/geologic/climate/hydro、relief spec、surface、climate work domain；stage version 从 1 开始，id 为 `natural.causal-formation`。`NaturalQualityProfileArtifact` 与
`ReliefSpecArtifact` 保持各自现有定义文件和 artifact key；新 stage 只依赖它们，
不为“集中”移动事实源。通用 `Stage` 不修改。

stage error code 固定为：`causal-formation.invalid-input`、`causal-formation.numerical-stability`、`causal-formation.solid-budget`、`causal-formation.sediment-budget`、`causal-formation.water-budget`、`causal-formation.climate-not-converged`、`causal-formation.endpoint-forcing-mismatch`、`causal-formation.elevation-out-of-range`、`causal-formation.resource-limit` 与既有 `engine.cancelled`。错误只携带诊断值，不携带可发布的中间 bundle。

- [ ] **Step 4: 在 sibling 可用的同一提交中去嵌套并迁移所有消费者**

`NaturalSurfaceFormationSnapshot`/wire/validator/fingerprint/accessor 删除 Task 9
过渡保留的 `formation_climate`；`finalize_surface_formation` 删除
`endpoint_climate` 所有权参数，只接收已经由同一个 endpoint P4 生成的 checkpoint
fingerprint。bundle factory 同时持有 sibling climate 并校验两者逐指纹相等，因而
不存在任何一个提交同时发布两份 P4 payload。

同一步删除 Task 9 仅为旧独立 stage 保留的
`SurfaceFormationGenerator::generate` 过渡 wrapper 及其导出；若 facade 类型因此失去
全部消费者，则按 YAGNI 一并删除。协调器继续直接组合
`advance_surface_processes`、`recompute_surface_diagnostics` 与
`finalize_surface_formation`，不得保留第二条可调用的 P4/P5 编排路径。同步删除
Task 7 的 `from_legacy_primary_wire_for_migration`；最终树不允许任何
`PrimaryReliefSnapshot` wire 构造科学 `FormationState`。

`TerrainAmplifier::from_formation_product` 与
`HierarchicalEvaluator::from_formation_product` 改为分别显式借用
`NaturalSurfaceFormationSnapshot` 和 sibling `GlobalCirculationSnapshot`；算法只
读取最窄快照，不依赖 app/artifact。`src/app.rs` 从同一个 bundle 把两者传给 T1，
不得在 T1 cache 里复制 climate。删除旧 `surface_formation_stage.rs` 及其测试；
`surface_formation_atlas/evidence/generation/performance/quality` 中全部
`NaturalSurfaceFormationArtifact`/`surface_formation_graph()` 直接消费者在同一
提交改用 causal bundle fixture 或纯 evaluator，不把编译修复推迟到 Task 12。

`build_spherical_formation_candidate_with_lineage` 改用
`causal_natural_formation_graph()`。`SphericalFormationFieldDocument::build` 固定为：

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

field id 与本地化文案已在 Task 7 冻结；本任务对 `fields.rs`/
`localization.rs` 只做 bundle payload 绑定所需调整，不重定义 schema、label、range
或 palette。保留 current elevation、primary elevation、tectonic displacement、
fluvial erosion、hillslope erosion/deposition、routed sediment deposition、coastal
erosion/deposition、isostatic response、P2/P5 current rates、hydrology、sediment
和最终 P4 字段。不得用 aggregate 替代九项事实。timeline 不进入面板，也不增加
年龄、分布或 cadence 旋钮。

`SphericalFormationFieldDocument::initial_circulation` 无消费者且 bundle 不发布
初始 P4，按 YAGNI 删除；若保留 `formation_climate()`，它只能借用
`formation.bundle().climate()`，不得从 surface snapshot 取值或缓存第二份。

- [ ] **Step 5: 运行 schema、serde、原子失败、T1 与 UI GREEN**

Run: `cargo test --release --test geologic_pipeline_contracts --test causal_formation_generation --test surface_formation_generation --test surface_formation_atlas --test surface_formation_evidence --test surface_formation_performance --test surface_formation_quality`

Run: `cargo test --release --test natural_field_registry_spherical --test field_display_integration --test spherical_presentation_integration`

Run: `cargo test --release --lib natural_app_tests`

Run: `cargo test --release --lib terrain_amplification`

Run: `cargo test --release --lib hierarchical_derivation`

Expected: 全部 PASS；serde 中 P5 无 `formation_climate`，T1/UI 只借用同一 bundle
sibling，成功只发布一个 bundle，取消、数值越界、预算或安装失败均没有部分
artifact/cache/GPU 提交。

- [ ] **Step 6: 运行完整 locked timeline 实测，不先写新 cadence/阈值**

`tests/causal_formation_performance.rs` 使用既有 P5 门禁事实：Draft/Standard/High
分别 `15/90/300 s`，High retained dense state `1 GiB`，取消 `250 ms`；测试记录
完整 P2 timeline、最终 P3、start P4、一次 P5 advance、endpoint P4、终点诊断、
bundle/T1 装配各自 wall time，P5 stable substeps、forcing/checkpoint fingerprints、
peak RSS、取消延迟和最终 bundle fingerprint。不得为满足预算跳过 endpoint P4、
缩短 P2 timeline/P5 horizon、把二者混成一个时域或新写 cadence。

Run: `cargo test --release --test causal_formation_performance -- --ignored --nocapture`

Expected: 生成 `target/natural-quality/causal-formation/performance.json`，生产外层
调用数为两次 P4/一次 P5，且无部分 artifact。Task 10 的离线敏感性 JSON 单独
保留，不由本性能测试重复运行。

- [ ] **Step 7: 验证既有最终态/性能门并提交原子迁移**

把机器、编译档、seed、profile、逐阶段耗时、峰值内存、取消延迟、最终
forcing/checkpoint 身份和文件 blake3 写入设计规格“实测修订”。Task 10 的
`Standard`/seed `42` 对照另记录两条路径的原始最终态差值、既有质量、守恒和
成本；它不产生新 envelope。迁移门同时要求：

1. endpoint P4 forcing identity 与 sibling checkpoint identity 通过；
2. solid/sediment/water/component、有限性、支持域和现有数值稳定性门禁通过；
3. 既有 P2/P3/P4/P5 最终态质量包络通过；
4. 既有 profile 时间/内存/取消门禁通过；
5. 代表性离线对照文件或资源不足记录及 blake3 已产生，且中间轨迹未进入 artifact。

- 五项全部通过：提交本任务并继续 Task 12；离线原始差值本身不构成失败。
- 1–3 因耦合离散误差失败：提交可复核证据后停止执行 Tasks 12–13，判断是否需
  显式修订为一次固定 predictor-corrector。仅第 4 项性能失败时，先定位 P2/P4/
  P5 成本，再按规格允许的近似线性解、多分辨率工作域或内部有界求解另作有出处
  修订；predictor-corrector 会增加成本，不是性能修复。不得预先实现双路径、
  减少 P2/P5 horizon、跳过 endpoint P4、放宽门禁或发布未完成状态。
- 仅离线对照因明确资源限制无法完成：记录机器与失败点，不把生产路径伪称为
  已与对照一致；该研发缺口不在常规构建中反复支付，也不放宽 1–4。

```bash
git add src/world/natural/formation_bundle.rs src/world/natural/mod.rs src/world/natural/surface_formation.rs src/world/natural/fields.rs src/generators/natural/causal_formation_stage.rs src/generators/natural/mod.rs src/generators/natural/surface_formation/state.rs src/generators/natural/surface_formation/mod.rs src/generators/natural/surface_formation/generation.rs src/generators/natural/surface_formation_stage.rs src/generators/natural/terrain_amplification.rs src/generators/natural/hierarchical_derivation.rs src/generators/natural/quality/global_circulation.rs src/generators/natural/quality/mod.rs src/app/spherical_presentation.rs src/app/spherical_formation_display.rs src/app.rs src/engine/cache.rs src/ui/field/localization.rs tests/support/causal_formation.rs tests/support/mod.rs tests/geologic_pipeline_contracts.rs tests/causal_formation_generation.rs tests/causal_formation_performance.rs tests/natural_field_registry_spherical.rs tests/field_display_integration.rs tests/spherical_presentation_integration.rs tests/surface_formation_atlas.rs tests/surface_formation_evidence.rs tests/surface_formation_generation.rs tests/surface_formation_performance.rs tests/surface_formation_quality.rs tests/surface_formation_stage.rs docs/superpowers/specs/2026-08-24-geologic-pipeline-contract-restoration-design.md
git diff --cached --check
git commit -m "Publish and render one formation bundle" -m "Atomically make final tectonics, terrain, sibling climate, surface state, T1 and UI share one current-state authority without a nested P4 copy."
```

---

### Task 12: 删除失去生产消费者的旧 stage、稳态 API 与兼容适配器

**Files:**
- Delete: `src/generators/natural/evolved_tectonic_stage.rs`
- Delete: `src/generators/natural/primary_relief_stage.rs`
- Modify: `src/generators/natural/global_circulation_stage.rs`（只保留 `ClimateWorkDomainArtifact`/`ClimateWorkDomainStage`）
- Modify: `src/generators/natural/mod.rs`
- Delete: `tests/evolved_tectonic_stage.rs`
- Delete: `tests/primary_relief_stage.rs`
- Delete: `tests/global_circulation_stage.rs`
- Modify: `tests/evolved_tectonic_evidence.rs`
- Modify: `tests/evolved_tectonic_performance.rs`
- Modify: `tests/evolved_tectonic_quality.rs`
- Modify: `tests/global_circulation_atlas.rs`
- Modify: `tests/global_circulation_evidence.rs`
- Modify: `tests/global_circulation_performance.rs`
- Modify: `tests/global_circulation_quality.rs`
- Modify: `tests/primary_relief_evidence.rs`
- Modify: `tests/primary_relief_performance.rs`
- Test: `tests/geologic_pipeline_contracts.rs`
- Test: `tests/causal_formation_generation.rs`
- Test: `tests/diagnostics_and_provenance.rs`
- Test: `tests/engine_execution.rs`
- Test: `tests/stage_graph.rs`

**Interfaces:**
- Consumes: Task 11 已无旧 stage/artifact 生产消费者且已删除独立 P5 stage 的事实。
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

Expected: FAIL，旧 evolved/primary/global-circulation stage assertions 仍在测试和
exports 中；独立 `surface_formation_graph()` 已由 Task 11 删除。

- [ ] **Step 2: 审计真实消费者并形成删除清单**

Run: `rg -n "(SurfaceFormationStage|NaturalSurfaceFormationArtifact|EvolvedTectonicStage|PrimaryReliefStage|GlobalCirculationStage|surface_formation_graph|global_circulation_graph)" src tests -g '*.rs'`

Expected: 输出只来自本任务具名的两个待删除完整 stage、
`global_circulation_stage.rs` 中待删的 P4 artifact/stage 部分、module re-export 及其
具名待迁移测试；不得再出现任何 P5 stage/artifact/graph 名称，因为它们及全部直接
消费者已由 Task 11 原子迁移。legacy foundation 使用不同代际类型；测试引用不视为
保留 production adapter 的理由。

- [ ] **Step 3: 删除无消费者代码并刷新缓存身份测试**

删除两份完整旧 stage 文件和 `global_circulation_stage.rs` 中的
`GlobalCirculationArtifact`/`GlobalCirculationStage`/`global_circulation_graph`，
只保留 work-domain artifact/stage。`EvolvedTectonicArtifact`、
`GeologicSubstrateArtifact`、`PrimaryReliefArtifact`、`GlobalCirculationArtifact`
随所属旧包装删除；`NaturalSurfaceFormationArtifact` 已在 Task 11 随独立 P5 stage
删除。Task 11 的 bundle factory 已直接运行对应 evaluator；纯
snapshot/generator/quality evaluator 保留。

删除三个仍只验证已退役 stage 的集成测试；其余上列 evolved/primary/climate
evidence/quality/performance 测试改为直接调用 generator/evaluator 或
`causal_natural_formation_graph()`，不得为测试保留 production adapter。P5
对应消费者已在 Task 11 随独立 P5 artifact 原子迁移。

更新 graph cache 测试：同 seed、同全部 resolved inputs 第二次必须命中新 bundle；timeline/schema/stage version 任一变化必须 miss，不双写新旧科学 artifact。

- [ ] **Step 4: 运行 GREEN 与 engine 不变式**

Run: `cargo test --release --test causal_formation_generation --test stage_graph --test engine_execution --test diagnostics_and_provenance`

Run: `git diff -- src/engine/stage.rs src/engine/graph.rs`

Expected: 测试 PASS；最后一条无输出，证明未修改通用 Stage/graph 接口。

- [ ] **Step 5: 提交**

```bash
git add src/generators/natural/evolved_tectonic_stage.rs src/generators/natural/primary_relief_stage.rs src/generators/natural/global_circulation_stage.rs src/generators/natural/mod.rs tests/evolved_tectonic_stage.rs tests/primary_relief_stage.rs tests/global_circulation_stage.rs tests/evolved_tectonic_evidence.rs tests/evolved_tectonic_performance.rs tests/evolved_tectonic_quality.rs tests/global_circulation_atlas.rs tests/global_circulation_evidence.rs tests/global_circulation_performance.rs tests/global_circulation_quality.rs tests/primary_relief_evidence.rs tests/primary_relief_performance.rs tests/geologic_pipeline_contracts.rs tests/causal_formation_generation.rs tests/stage_graph.rs tests/engine_execution.rs tests/diagnostics_and_provenance.rs
git diff --cached --check
git commit -m "Retire split formation publication stages" -m "Delete superseded adapters after the causal bundle becomes the only production science artifact."
```

提交前检查 staged name list 与完整 diff；不得用 `git add -A` 吸收范围外修改。

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
fn compatibility_elevation_ablation_preserves_p3_p4_p5_and_t1_authority() {
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
        assert_eq!(mutated.t1_probe_hash(), original.t1_probe_hash(), "seed {seed}");
    }
}
```

该测试放在 `causal_formation.rs` 的 private test module；
`generate_with_test_compatibility_mutation` 在最终 P2 snapshot 生成后、任何
P3/P4/P5 消费前，只改 compatibility elevation 数组。fixture 用 production
`HierarchicalEvaluator` 在固定 probes 上计算 test-private hash，证明 T1 也不
回读兼容高程；不为测试增加 production accessor。helper 与 mutation enum 均受
`#[cfg(test)]` 约束，不增加 production 注入点。

补全 seed `3/7/42` Draft，现有 atlas/evidence corpus，至少一个 Standard release build；断言非零当前 `dh/dt` 合法、所有位移可归因、solid/sediment/water/component budget 闭合。

- [ ] **Step 2: 运行 Release 科学/呈现套件**

Run: `cargo test --release --test geologic_pipeline_contracts --test causal_formation_generation --test surface_formation_contracts --test surface_formation_generation --test surface_formation_quality --test surface_formation_evidence --test surface_formation_atlas`

Run: `cargo test --release --lib compatibility_elevation_ablation_preserves_p3_p4_p5_and_t1_authority`

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
3. 在字段面板依次查看本地化 SSOT 中的“当前地表高程”“初级地形高程”
   “构造位移量”“河流侵蚀深度”“坡面侵蚀量”“坡面堆积量”“河道输沙沉积”
   “海岸侵蚀量”“海岸沉积量”“均衡响应”，再查看构造位移率、河流侵蚀率、
   沉积厚度和最终气候降水；预期九项累计组成可分别归因且由同一次 bundle build
   提供，切换字段不重新求解科学状态。若 Task 7 的 registry SSOT 在实现期经显式
   修订改名，完成记录必须引用实际 `localization.rs` 文案，不得保留这份旧清单。
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
- P5 `SURFACE_FORMATION_HORIZON_YEARS`：从 HEAD 恢复、并由规格 §0.1(9)
  显式替代 R3 §14.5 的相反结论；数值仍沿用
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
- 在 P5 horizon 内零阶保持最终 P2 uplift/subsidence rate，是上述低频顺序耦合的
  工程类比；对被保持的常量 forcing，`displacement = rate × accepted duration`
  是精确积分。Paik & Kim、Shen et al. 与 Santos et al. 不直接给出 Sekai 的保持
  时长或误差，故适用性由 Task 10 代表性参考实测，明确保留为开放问题。
- P3 删除当前率经验增益不引入替代公式：原 `250 m/(mm yr⁻¹)` 量纲等效于
  无出处的 `250 kyr` 保持积分，而 P5 已拥有唯一明确物理时长的 rate integration。
  Cortial et al. (2019) 支持 P2 程序构造与有单位 forcing，不支持 P3 的这个经验
  响应时间；因此 Task 3 删除该机制，并以质量门实测是否暴露新的物质/过程缺口。
- 一次固定 predictor-corrector 和高成本耦合顺序对照的数值类比：Schüller et al.
  (2025), DOI `10.5194/gmd-18-9167-2025`，比较非迭代/迭代 Earth-system
  coupling 并记录非光滑参数化的非收敛风险；Strang (1968), DOI
  `10.1137/0705041`，只支持离线对照的对称排序背景。本计划不预实现校正路径，
  不把不同时域的对照称为高精度金标准，也不声称全系统二阶。
- 后期空间参数分布的数学入口：Lang & Schwab (2015), DOI
  `10.1214/14-AAP1067`（球面相关随机场与频谱截断）；Lindgren, Rue &
  Lindström (2011), DOI `10.1111/j.1467-9868.2011.00777.x`（流形/三角网格上的
  Matérn/SPDE 场）；Lagae et al. (2009), *Procedural Noise using Sparse Gabor
  Convolution*, DOI `10.1145/1531326.1531360`（方向/频谱控制）；Goff & Jordan
  (1988), DOI
  `10.1029/JB093iB11p13589`（各向异性海床协方差的地学类比）。这些来源不为
  任何具体 P2/P3/P5 参数给出通用分布；边际/联合分布、尺度、条件关系和值仍须
  逐参数找直接出处，否则明确列为开放问题。
- 参数采样的确定性/正交性沿用现有 `LabeledSubstreams` 工业实现：固定 32-byte
  根材料经长度分帧的 BLAKE3 标签派生 `rand_chacha::ChaCha8Rng`；实现位于
  `src/generators/natural/random.rs`，并由既有跨标签不干扰测试守门。该机制只
  负责可重放与模块隔离，不替代物理分布出处。
- 河流侵蚀的隐式下游栈：Braun & Willett (2013), DOI `10.1016/j.geomorph.2012.10.008`；抬升—侵蚀响应背景：Whipple & Tucker (1999), DOI `10.1029/1999JB900120`。
- 河流侵蚀—输运—沉积连续方程与解析递推：Davy & Lague (2009), DOI `10.1029/2008JF001146`；Barnhart et al. (2019), DOI `10.5194/gmd-12-1267-2019`；pyBadlands/SPACE 的守恒库存实践见 Salles (2018) 与 Shobe et al. (2017)。
- 九项最终高程组成不是新算法：primary 来自 P3；tectonic displacement 来自 P2
  有单位 forcing；fluvial/routed sediment、hillslope、coast 与 Airy 分别沿用本节
  已列 Davy & Lague/Barnhart/Shobe/Salles、Eymard/Landlab、Paola & Voller、
  Turcotte & Schubert 的机制。Task 7 只保留它们各自的最终累计账本和唯一求和
  恒等式，禁止折叠成无归因 aggregate。
- 非线性坡面稳定子步与离散最大值：Eymard, Gallouët & Herbin (2000), DOI `10.1016/S1570-8659(00)07005-8`；Landlab `TaylorNonLinearDiffuser` commit `8f59a66279cefa288b146735a939d95e9a6730c2`。
- 海岸交换、沉积库存和 generalized Exner 质量账本：Paola & Voller (2005), DOI `10.1029/2004JF000274`；具体项目适用边界按上位规格保留。
- 局部 Airy 加载/卸载：Turcotte & Schubert (2014), *Geodynamics*, 3rd ed., ch. 5；响应直接记入完整 `f64` 状态，不按 artifact 边界裁剪。
- PTC 只用于有根的内部快平衡，不再作为全地貌绝对稳态：Kelley & Keyes (1998), DOI `10.1137/S0036142996304796`；PETSc 只作数值实现类比，不提供 Sekai 科学容差。
- 浮点身份、先定义精确状态再做发布舍入：Goldberg (1991), DOI `10.1145/103162.103163`；稳定误差解释参考 Higham (2002), *Accuracy and Stability of Numerical Algorithms*, 2nd ed.
- Task 6 的前置成本归因沿用项目既有 release wall-clock/peak-RSS/取消测量工业
  实践，只测量生产 operator，不创造科学阈值。若测量要求更换内部算法，隐式
  下游栈先采用 Braun & Willett (2013) 的直接依据；近似线性解或多分辨率工作域
  必须在落地前补充直接出处，否则按规格标为类比和开放问题。
- 原子候选/提交、确定性重放与 artifact 内容寻址沿用本项目现有 engine 工业实践；不修改通用 Stage，不引入第二份科学事实源。
