# G4 大陆几何物理化实施计划（2026-10-08 规格，计划起草于 2026-10-26）

上位：`AGENTS.md`；设计真相
`docs/superpowers/specs/2026-10-08-g4-continental-geometry-physics-design.md`（R1）。
本计划只把规格 §11 的阶段拆成“一任务一提交”的任务；任何偏离规格的地方以规格修订条目
记录，不在本计划里另立决定。阶段 0 与 1a 写到可执行的细度，1b、2、3 只列大纲，进入前
再细化。

基线：`main` 51a9b36。分支 `g4-stage0-1a`（worktree `.worktrees/g4-stage0-1a`）。

## 共同纪律

- 每个任务一个提交；提交前跑 `cargo fmt --all -- --check`、
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`、
  `cargo check --target wasm32-unknown-unknown --all-features --lib`。
- 测试范围按 `AGENTS.md` 影响面规则：碰到构造求解器与身份时跑 Release 的构造、形成链、
  层级产品与 GPU 金样套件；不跑调试档全量，除非怀疑调试断言或溢出（规格 §11 末段）。
- 计时纪律（A8）：Release、同机、CPU 空闲、新旧交替成对、取最小值、任何时刻只跑一个
  cargo。计时前检查有无其他 cargo / rustc 进程（其他 worktree 可能在跑），不空闲时记录。
- 门失败时不调参、不放宽门（规格 Q10、`AGENTS.md` 过程成因）：归因、记录、停下报告。
- 规格歧义或未覆盖的决定：停在该点，写成开放问题交用户裁定。

## 阶段 0：测（只提交测试与夹具，不改产物）

### Task 0.0 —— 前置核对：“P2 重采样厚度/边界噪声修复”是否已落地（规格 §7.4、§11）

核对结论（2026-10-26，`main` 51a9b36）：**未落地，阶段 0 的测量不得开始。**

- 2090696（G1e R4）只改了重采样的**起步厚度**：`resample.rs` 的
  `conservative_material_resample_v5` 仍以覆盖赢家（最近样本，NGP）决定每格的
  `kinds`，插值厚度只在 `provisional.kind == winner.kind` 时使用。陆壳/洋壳**种类边界**
  的分配没有变化。
- 规格 §1.3 H3 所说的草稿档噪声是**掩膜边界**的：“每次重采样扰动约 1 个控制格”、
  周长/√面积翻倍、充填晕、内部孔洞、刚体残差。这正是 M1、M4 读取的量；2090696 按构造
  不触及它。
- 规格 R1 在 2090696 之后一分钟入库（2bdaacb），§8 已引用 2090696 占用 G1e R4，却仍把
  该修复列为未完成的前置；同日的 P5 湖泊规格 §1.3 同样引用 2090696，§9.0 仍把“厚度/
  边界噪声修复”列为未落地的前置。2090696 之后 `foundation/tectonics/` 没有其他提交，
  其他 worktree（a10-*、lod1-*）也没有相关提交。
- G1e R4 自己的“未做（开放）”(a)（张性拉伸只作用于代表样本、逐边法向阶梯翻号）仍在
  等用户裁定。

处置（初稿）：本计划提交后停下交用户裁定。

**用户裁定（2026-10-26，规格 R2）**：批准阶段顺序偏离，不等边界噪声修复（另立规格），
先做阶段 0 测量与阶段 1a。阶段 0 / 1a 的 M1–M4 一律标“边界噪声修复前的临时值”，只记录；
§7.3 的界推迟到修复落地、重测之后再推导。地球参照与零模型照常测。

### Task 0.1 —— 测试侧形状助手 `tests/support/continent_shape.rs`（规格 §7.1）

- 新建 `tests/support/continent_shape.rs`，消费者以 `#[path = "support/continent_shape.rs"]`
  单独引入（不把其余 support 模块编进每个测试）。不新增生产 API、
  schema 或算法分支。
- 输入：权威面 `SphericalSurfaceSnapshot`、`NaturalTopologyIndex`（或同等的权威拓扑
  邻接）与逐格布尔掩膜。连通性一律用权威拓扑。
- M1 `six_cap_iou`：K = 6 等面积球冠（单冠面积 = 掩膜面积 / 6），中心用球面 Lloyd：
  掩膜格的最远点采样初始化（复用 `farthest_point_seeds_from_candidates` 若可见，否则
  在测试侧以同一定义实现并注明），迭代到分配不变；IoU 以格面积计。
- M2 `major_component_area_cv`：面积 ≥ 1 % 球面的连通陆块的面积变异系数。
- M3 `max_major_elongation`：把 `tests/spherical_morphology_quality.rs:255` 的
  `tangent_covariance_aspect_ratio` **挪进**助手（SSOT：原文件改为引用助手，不留第二份），
  对每个主要陆块取 \(\sqrt{\lambda_1/\lambda_2}\) 的最大值。
- M4 `boundary_box_dimension`：边界格（掩膜内且有掩膜外邻格）上，图距离盒半径
  r ∈ {4, 8, 16} 格的贪心盒覆盖数，取 log N 对 log r 的最小二乘斜率的相反数。
- 纯函数测试（无种子，规格 §12 最小充分）：6 个互不相交的等面积球冠掩膜上 M1 接近 1、
  M2 = 0、M3 接近 1；单一球冠的 M4 接近 1；结果与格遍历顺序无关。离散网格上“接近”
  的量值先测后写，不预拍容差。

### Task 0.2 —— 地球参照夹具探针 `probe_earth_reference_masks`（规格 §7.2）

- ignored、Release、离线：读环境变量 `SEKAI_ETOPO1_PATH` 指向的 ETOPO1 冰面版
  （Amante & Eakins 2009）10′ 采样；数据不入库。
- 按格平均到草稿档与标准档权威网格（与诊断口径一致：格内所有 10′ 点的面积加权均值），
  输出逐格位掩膜 `tests/fixtures/earth/{draft,standard}-{threshold}.bin`：
  - 主掩膜 `> −200 m`（Q1）；
  - 记录项 `0 m`（只给 P3 陆地探针）；
  - 记录项 Cogley (1984) 地壳块：需要一份可复核的栅格化来源（见开放问题 O3），无来源
    则不生成，在修订里如实记“未测”。
- 清单 `tests/fixtures/earth/MANIFEST.md`（或同名 JSON）：数据版本、源文件 SHA-256、
  处理方法、网格 `SurfaceRef` 哈希、每张掩膜的校验和。
- 夹具读取函数放在 `tests/support/continent_shape.rs`：载入时校验 `SurfaceRef`，不符即
  失败。代码中不出现任何地球常数（G1 §2 本意）。

### Task 0.3 —— 阶段 0 测量探针与时延基线（规格 §7.4、§10）

- ignored Release 探针（建议放进新的 `tests/continent_shape_probe.rs`，以后 A5 门
  同文件）：`formation_seed_sweep.rs` 冻结的 8 个种子（`DEFAULT_SEEDS`）× 草稿/标准 ×
  Continents，12 板、推荐陆壳分数；活动度见开放问题 O2。对 P2 发布到权威面的地壳类型
  掩膜算 M1–M4，输出逐种子值与中位数。
- 同一助手算地球参照（> −200 m 主掩膜；0 m、Cogley 记录）与零模型：§7.3 的解析值
  （1、0、1、1）是门的端点；另在同网格上量“6 个等面积球冠”掩膜的 M1–M4 作记录，用来
  看离散化让零模型偏离解析值多少（只记录，不改 §7.3 规则）。
- 由地球值按 §7.3 中点规则算出两档的界，标出现状是否通过（只记录：阶段 0 不设门）。
- 时延基线（§10 三项）：
  1. P2 阶段时间，草稿 / 标准，种子 42（`evolved_tectonic_performance` 或阶段探针的 P2
     行）；
  2. 草稿档 `ui_scope`，种子 42，2 次。A8b §9.4.1 的 `ui_scope` 取自不入库的
     `a8_meas_full_graph` 计时装置补丁（见开放问题 O4）；
  3. `formation_seed_sweep.rs` 8 种子冷启动，草稿与标准各一次，记录均值与范围。

### Task 0.4 —— 规格修订 R3：写入阶段 0 实测（规格 §7.3、§17）

- §17 新增 R3：M1–M4 两档逐种子与中位；地球参照；零模型（解析与
  离散）；§7.3 两档的界；时延基线三项与机器空闲情况。
- §7.3 的界按 R2 推迟推导；R3 只给出“若按临时值与地球值套中点规则”的参考数，明确不作定值。

## 阶段 1a：裂谷调度按物理率评估（规格 §4.0、§8 1a 行、§9）

进度（2026-10-27，规格 R4）：1a.1、1a.2 完成（另加 bb77b2a 率求值一次扫描）；门禁中
`spherical_morphology_quality` 三项与 §10 第 1 条（P2 时延）未过，已归因、停下待裁定；
1a.3 UI 字段未做。合并时 causal 抬升改为 6 → 7（P2 边界噪声修复先占 5 → 6）。

### Task 1a.1 —— `SEKAI_V5_TRACE` 裂谷调度计数（只加诊断，不改产物）

- `processes/rifting.rs::maybe_rift_plates` 返回的 `ProcessStats`（或一个小的私有统计
  结构）增加本步评估数与被跳过数；`runner.rs` V5 循环在 `SEKAI_V5_TRACE` 下打印每步一行
  `[v5-trace] step N rift: evaluated=… skipped=… events=…`，以及每次裂谷 / 机械碎裂事件
  一行（步数、子板数、是否切到陆壳：子板间割边中陆壳边是否 > 0）。规格 §8 runner 行的
  “割长份额”属 1b，不在此做。
- 在 HEAD（未修调度）上以该计数重测语料：跳过率、每世界裂谷数与首裂时刻、面积帽碎裂
  次数（规格 §1.3 H2、H4 的现状，作为 1a 的“之前”）。语料取阶段 0 的 8 种子 × 草稿 /
  标准 × Continents；若要与 A0 直接对照，另跑 A0 的 4 种子 × 草稿三预设 + 标准 Continents。
- 打印受环境变量门控，不改产物，不抬身份。

### Task 1a.2 —— 把 `maybe_rift_plates` 移到提交之后（§4.0 推荐方案）并抬身份、重钉

计划核实（2026-10-26）：推荐方案可行，不需要备选的“推迟”语义。

- 依据：`TectonicState` 只有 `samples / plates / initiation / next_lineage_raw`；工作区
  `swap_current_next` 之后的 `workspace.current` 是完整、自洽的已提交状态，`step_parts`
  在下一步由 `advance_samples` 重写 `next`；`resample_due` 只看步计数。机械碎裂
  `mechanically_fragment_oversized_plates_v5` 已在已提交状态上直接改 owner、换板、
  `initiation.remap_inherited_lineage`、检查 `FragmentationChangedMaterial`，是现成先例。
  动作批与重采样都不依赖“裂谷在批内”。
- 改法：
  - `maybe_rift_plates(step, delta_myr, surface, state: &mut TectonicState, recipe,
    streams, scratch)`：在已提交状态上逐板求率（`rift_rate_q32_with_scratch`，率与
    抽签键 `[step, lineage]`、子板数 `[step, lineage, 1]`、首种子 `[step, lineage, 2]`、
    逐子板噪声 `[step, lineage, 3+child]` 全部不变）；`split_plate` 改为直接写
    `state.samples[i].owner`、换板、`initiation.replace_lineage`，前后比较
    `material_totals()`，不等即 `FragmentationChangedMaterial`。scratch 继续复用
    `ProcessActions::rift_scratch`（提交后动作已清空，借用不冲突）。
  - 删除 `ProcessActions::lineage_has_pending_changes`（唯一调用者即该门，§4.8）。
  - V5 循环（`runner.rs` `evolve_control_state_v5_with_resample_observer`）：调用移到
    `commit_process_actions_v5` 与 `swap_current_next` 之后、`resample_due` 之前，作用于
    `workspace.current`。V4 兼容循环（`evolve_current_state`，
    `natural.spherical-tectonics` 阶段）同步移到 `commit_process_actions` 与
    `swap_current_next` 之后（§8、§9：V3 兼容路径共用该函数，同样会变）。
  - 裂谷子板的谱系账：`EvolutionLineageLedger::budget` 由 `next_lineage_raw` 与存活集合
    推出，不需要新的记账项（与今天相同）。
- 测试（§12 调度行，无种子）：
  - 改写 `rifting.rs` 的 `rifting_does_not_delete_a_lineage_with_pending_process_actions`：
    该测试保护的是旧门的语义，提交后评估使它失去对象。新测试在现有夹具上构造一步：
    某板同时有俯冲移除（或扩张新生）且抽签命中，经批处理、提交后调用
    `maybe_rift_plates`，断言裂谷照常发生（旧代码会跳过）、物质总量逐位不变、所有样本
    的 owner 都是存活板、谱系不复用。
  - 其余 `rifting.rs` 现有测试按新签名改调用方式，断言不变。
- 身份（§9，同一提交，不留红色哈希）：
  - `natural.spherical-tectonics` 9 → 10：`foundation/graph.rs`（`SphericalTectonicStage::
    version`），以及 grep `"natural.spherical-tectonics", 9` 的全部测试
    （`foundation/tectonics.rs:289`、`tests/evolved_tectonic_forcing.rs:49`、
    `tests/spherical_morphology_quality.rs:98`、`tests/spherical_natural_graph_performance.rs:251`
    等）。
  - `natural.causal-formation` 5 → 6：`formation/graph.rs` 与 `formation/causal.rs` 的
    派生身份（251、352、969、1425 行附近）。
  - `natural.evolved-tectonics@5` 只在测试里构造 `StageIdentity`（`publication.rs:813`、
    `runner.rs:870/1063`、`primary_relief.rs:641`），按 G1 R2 先例维持不动；实现时再确认
    它不是活阶段。
  - 冻结物重钉（审计浮点平台金丝雀自检）：`tests/spherical_natural_stage_graph.rs::
    EXPECTED_GRAPH_HASHES`、`app/spherical_natural_display.rs::EXPECTED_FIELD_HASH`、
    `tests/hierarchical_product_gates.rs::EXPECTED_PROBE_FINGERPRINT`、
    `tests/golden/natural-foundation/*.png`、G1e R2 提到的字形采样表；P0 V4 字节基线若仍有
    消费者同步重钉。换版本即换阶段种子：语料钉值只按新语料重测，门的定义不改。

### Task 1a.3 —— UI 显示字段“造山与缝合带年龄”（§8 fields 行，接入 UI）

- `src/world/natural/fields.rs` 新增字段 id（参照 `ocean_age_myr_field_id`，54 行与注册表
  937/1004 行）；`src/view/palette.rs` 色标；`src/ui/field/localization.rs` 文案
  “造山与缝合带年龄”；`app/spherical_formation_display.rs` 绑定
  `compatibility.orogeny_age_myr()`（参照 556、763 行对洋龄的绑定）。
- 无造山的哨兵值按无数据显示，沿用洋龄字段对陆壳哨兵的处理先例。求解器零改动。
- 字段注册表或显示文档哈希若因此改变，在本提交内重钉。

### Task 1a.4 —— 阶段 1a 门禁与记录，规格修订 R4（§11 1a 行）

- 门：
  1. 跳过率为 0（Task 1a.1 的计数，`MAXIMUM_RIFTED_LINEAGES` 上限之外每块存活板每步
     恰好一次评估）；
  2. V5 §11 的 17 种子证据门（`tests/evolved_tectonic_evidence.rs`、
     `tests/evolved_tectonic_quality.rs`）与账本闭合门全绿；
  3. `tests/g1d_endstate_crust.rs` 关系门全绿；
  4. 逐位确定性：同种子重复构建字节相同；
  5. §10 时延门三项（与阶段 0 基线成对比较）。
- 记录：每世界裂谷数与首裂时刻（对照 A0 无门期望 1.3–2.3/世界，不作门）、裂谷与面积帽
  碎裂各自切开大陆的次数、M1–M4 两档（与阶段 0 对照，即“裂谷真的发生”的独立贡献）、
  A8b `stage_timing_probe` 冻结摘要（P4 循环、步数、快子步、残差）按新身份的实测。
- 规格 §17 新增 R4 写入以上结果（M1–M4 标临时值）。任何门失败：归因、记录、停下报告，不调参。
- 用户 UI 验证步骤（§13 第 2 条阶段 1a 部分）随交付说明给出。

## 阶段 1b、2、3（大纲，进入前细化）

- **1b A2 切线**：`effective_elastic_thickness_m`（`foundation/crust_physics.rs`）；
  `marker_spanning_forest_partition`（`natural/topology.rs`）；`split_plate` 与
  `assign_fragment_domains` 换成同一划分；§4.7 常量先测后钉（Audet & Bürgmann 2011 的
  陆 Te 两类中位需要数据处理）；删除 §4.8 各项；trace 增加割长份额；身份 10 → 11、
  causal 6 → 7；§12 纯函数与窄集成测试；用户 UI 验收“裂谷沿造山带走”。
- **2 A1 拼合开局**：§5 全部（Continents 与 Supercontinent）；
  `merge_supercontinent_plates` 返回被并入谱系；有符号距离前缀；`stamp_opening_sutures`；
  `OPENING_SUTURE_AGE_MYR`；迁移 `CONTINENTAL_COLLISION_ZONE_WIDTH_M`；谓词改名；删除
  `hemispheric_cap_nuclei`；身份 11 → 12、causal 7 → 8；A5 门两档通过；G1、G1e 修订条目；
  用户 UI 验收（§13）。
- **3 条件升级**：只在阶段 2 的 A5 门失败时启动，先归因再交裁定。

## 开放问题（2026-10-26 裁定，规格 R2）

- O1：边界噪声修复另立规格，由他人处理；本计划先行阶段 0 / 1a（临时值）。
- O2：A5 语料活动度取 Moderate（`TectonicSpec::default()`）。
- O3：Cogley (1984) 无可复核栅格来源即记“未测”。
- O4：`ui_scope` 在临时 worktree 上打 A8 §2 计时补丁成对计时，补丁不入库。
- O5：ETOPO1 冰面版先在本机查找，没有则从 NOAA NCEI 官方源下载到
  `D:/hemifuture-probes/etopo1/`（不入库），版本、URL、文件名、SHA-256 记入清单与修订。
- 计时协调：计时前写 `D:/hemifuture-probes/.cargo-timing.lock`（分支名 + 开始时间），确认无其他
  cargo / rustc 进程，计时结束立即删除。

## 每项承重技术的出处

见规格 §16。本计划额外依赖的只有测量纪律（`AGENTS.md` 先测后钉、A8 计时纪律）与
G1 R2、G1e R2 的身份抬升与重钉先例。
