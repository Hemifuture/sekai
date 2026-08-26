# Sekai Agent Instructions

本文件是本项目对 AI 代理与所有协作者的**唯一指令事实源**。`CLAUDE.md`
只指向此文件，不承载内容。规则冲突时以本文件为准；修改规则只改这里。

## 工程原则（用户指令，2026-08-19）

### 1. 极简哲学
- 只构建解决当前问题的最小方案；禁止投机性通用化（YAGNI）。
- 每个旋钮、抽象层、trait 必须有真实消费者；无人使用的一律不加、已失去
  使用者的主动删除。修复的第一候选是"删代码"，其次才是"加代码"。
- 新增第三方依赖必须在提交说明中给出理由。

### 2. DRY 与唯一事实源（SSOT）
- 每个事实——常量、公式、schema、色板、阈值、图例——在代码中只允许有
  一处定义，其余位置引用它。发现重复即视为缺陷：先合并事实源，再继续
  功能开发。
- 项目内的事实源分工：物理常量与领域模型归 `src/world/`；色板只在
  `src/view/palette.rs`；高程恒等式只有
  `formation_elevation_from_components` 一个实现；设计真相在
  `docs/superpowers/specs/`（冻结后只能以显式"修订"条目变更）；运行时
  身份真相是 artifact 指纹。
- 测试必须复用生产侧助手而不是重新实现算法（先例：
  `elevation_display_radius_m` 被测试直接引用）。
- 文档与注释不复述代码中的数值，引用常量名；面板/图例文案由字段注册表
  与本地化表驱动，不得在调用点硬编码第二份。

### 3. 高内聚低耦合
- 模块边界即领域边界：`world`（语义与数据）、`generators`（算法）、
  `engine`（编排、缓存、指纹）、`app`（装配）、`view`（呈现）、
  `gpu`（上传与绘制）。算法不进 `app`，呈现不进 `generators`。
- 跨层只通过已验证的快照 / Artifact 通信；禁止越层读取内部字段。
- 可见性从最小开始（private → `pub(super)` → `pub(crate)` → `pub`），
  每次放宽都必须有具体消费者。

### 4. 功能模块正交
- 特性之间组合而不互相知晓：改色板不得触碰求解器；新增一个展示字段 =
  注册表条目 + payload 绑定，渲染器与求解器零改动（`FieldDocument`
  边界就是范式）。
- 禁止让一个开关改变无关模块的行为；配置项的影响面必须与其所属模块
  一致。
- 新能力优先做成平行的正交单元，经枚举 / trait 接入点挂载，而不是在
  既有路径里加分支（先例：`SphericalWorldFieldDocument`）。

## 出处纪律：决策必须有最佳实践背书（用户指令，2026-08-22）

- 任何算法、公式、常量、阈值、验收包络与门禁边界——**尤其是地形与自然
  管线**——都必须有学术界或工业界的既有最佳实践背书，并在设计规格中写明
  出处（作者与年份、数据集及其版本，或可复核的工业实现）。禁止凭感觉
  自创机制、自拟系数。
- 每份计划以"每项承重技术的出处"一节收尾，逐项列出该里程碑依赖的技术与
  来源；规格里的每个杠杆、每条恒等式同样逐项标注（先例：T0、T0b）。
- 数值常量归 `src/world/`，文档注释必须写明来源与取值过程（先例：
  `CRUST1_PLATFORM_THICKNESS_QUANTILES_KM` 取自 CRUST1.0 稳定台地分位表；
  `EARTH_OCEANIC_SEDIMENT_MEAN_THICKNESS_M`、`EARTH_OCEAN_CRUST_MEAN_AGE_MYR`
  同理）。自行从公开数据集算出的值，要记录数据版本、处理方法与校验和。
- **先测后钉**：需要新常量时，先用生产算子测量现状（探针、语料实测），
  把数字写进规格，再据实测与文献钉值；不得先拍一个数字再倒补理由。
- **过程成因，不做事后修形**：一切位移必须由过程或有出处的常量产生。
  禁止后处理直方图重映射、为凑指标而设的经验曲线与魔法系数——指标不达标
  时要找成因，不是改结果。
- 找不到直接对口出处时：给出最接近的一手依据加上明确的类比论证，并把该
  项作为**开放问题**写进规格交用户裁定，不得以"看起来合理"落地（先例：
  T0 §11.4 陆地占比、T0b §8 常量表）。
- 世界设定可以偏离地球（水量、陆地占比等由用户决定），但机制必须守
  物理：偏离的是参数取值，不是方程（用户裁定：过程守物理，结果归玩家，
  系统只给建议值）。

## 产品边界：最终态与科学近似（用户指令，2026-08-25）

- Sekai 是**地图生成器**，不是生态、气候或地球动力学预测模拟器。产品是一次
  生成结束时原子发布的最终当前态；时间轨迹、耦合中间态、迭代序列、拒绝步和
  高精度参考解只属于私有求解过程，不是最终产物，不得仅为“模拟完整”而进入
  artifact、持久缓存契约或 UI。
- 科学性首先约束**机制**：方程与过程来源、单位、因果归属、质量/水量守恒、
  数值域、跨层身份和最终态自洽必须正确。不得用无成因修形、魔法系数、
  事后重映射或科学状态 clamp 掩盖机制或账本错误。
- 效率允许约束**求解策略**：可以采用有数值分析或工业实践依据的算子分裂、
  显式/隐式方法、预测—校正、固定或有界迭代、近似线性解、较低频率耦合和
  多分辨率工作域。类似为同一数学问题选择牛顿法、拟牛顿法或其他迭代器；
  近似可以改变求解成本和误差，不得偷换被求解的物理机制。
- 默认选择满足最终态质量与性能目标的最小求解方案。除非最终地图有已证明的
  真实需求，不得默认要求单次生成的完整时间轨迹收敛、保存历史，或把多级
  步长加密/高精度参考求解设为每次构建的发布门禁。
- 新增或改变近似策略时，先在代表性 seed/profile 语料上用生产算子与更高成本
  的参考路径做离线对照，记录最终态误差、守恒、质量指标、耗时和适用范围。
  参考路径只提供研发证据，不进入产品 schema；验收以最终态不变式、既有质量
  包络、性能门禁和 UI 结果为准。若需要新的误差阈值，仍须遵循“先测后钉”和
  出处纪律。
- 近似导致最终态不守恒、身份不一致、非有限或越出科学支持域时，必须调整
  求解器、步长或耦合策略；不得以地图生成器为理由放宽硬不变式或裁剪结果。

## 测试范围纪律（用户指令，2026-08-26）

- 测试遵循“最小充分证据”：每个新增或扩大的测试必须对应一个明确契约、风险或
  已复现回归，并能说明更小测试层级为何不足；不得以“更保险”为由盲目增加
  seed、profile、分辨率、迭代时域、模块或断言数量。
- 优先选择能捕获该失败的最低成本层级：纯函数/单元测试 → 窄集成测试 →
  代表性语料 → 全链/UI。局部行为不得默认通过多 seed 全管线生成来证明；已有
  下游契约覆盖的事实不得在上游重复测试。
- 只有性质本身跨 seed/profile、具有统计性，或涉及跨层原子身份时，才扩大语料；
  范围必须取满足证据需求的最小代表集，并在测试或计划中写明消费者与选择理由。
  新增语料规模、性能门禁或阈值仍须遵循“先测后钉”和出处纪律。
- 高成本参考、敏感性分析与大语料探针默认使用 `#[ignore]`、Release 档和离线
  运行，不进入日常单元测试或普通构建门禁。测试不得保存无人消费的历史状态，
  也不得为便于测试新增生产抽象、公共 API、schema 或算法分支。
- 测试复用生产侧事实源、构造器和校验器，不复制科学公式。若测试耗时显著增加，
  先定位重复求解并缩窄 fixture/调用次数；不能用扩大超时掩盖不合理范围。
- 本节约束测试设计与迭代期命令，不取消工作流程中明确要求的最终完整回归、
  性能门禁和用户 UI 验收。

## 验收纪律：算法必须与 UI 同步交付（用户指令，2026-08-19）

- 算法与生成管线的验收一定要与 UI 同步：任何一条生成链路、任何算法改动
  或质量修复，只有当它在应用界面上能被看到、被操作时才算交付。只有后台
  实现的算法是没有用的。
- 最终验收由用户本人在 UI 上完成。代理的自行验证（单元/集成测试、探针、
  离线渲染、无障碍驱动截图等）只能解决一部分问题，必要但永远不充分，
  不能替代用户上手验证。
- 因此每个算法类任务的计划必须包含"接入 UI"的任务项；未接入 UI 之前
  不得把算法任务标记为完成或宣称"已交付"。
- 每次交付都要附上用户验证步骤：如何启动、在哪个面板/视图看、预期看到
  什么。

## 工作流程

- 较大特性走 superpowers 纪律：`docs/superpowers/plans/` 里的任务化
  计划、一任务一提交；锁定设计入 `docs/superpowers/specs/`，实现期偏离
  以修订条目显式记录。
- 项目文档（plans / specs / 完成记录）用中文撰写；引文、代码标识符与
  常量名保留原文。已完结的历史文档不回头翻译。（用户指令，2026-08-19）
- 门禁（提交前）：`cargo fmt --all -- --check`；
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`；
  `cargo check --target wasm32-unknown-unknown --all-features --lib`；
  受影响测试即时跑，任务收尾跑完整调试回归。
- P5 全链集成套件在调试档极慢（全量约 40 分钟）：迭代期用 `--release`
  跑目标套件，最终门禁再跑完整调试回归。
- 提交信息：一行祈使句主题 + 说明动机的正文。

---

English mirror: this file is the single source of truth for agent
instructions (CLAUDE.md only points here). Engineering principles mandated
by the user: minimalism (build the least that solves the problem, prefer
deletion, no speculative generality); DRY / single source of truth (every
fact defined once — constants in `world`, palettes in `view/palette.rs`,
design truth in specs, identity truth in fingerprints; tests reuse
production helpers); high cohesion & low coupling (module = domain, layers
talk only through validated snapshots/artifacts, minimal visibility);
orthogonal features (compose without mutual knowledge; new capabilities
mount through enum/trait seams like `SphericalWorldFieldDocument` instead
of branching existing paths). Provenance: every algorithm, formula,
constant, threshold, acceptance envelope and gate bound — terrain and the
natural pipeline above all — must rest on established academic or industry
practice, cited in the spec (author and year, dataset and version, or a
checkable industrial implementation); plans close with a "sources for each
load-bearing technique" section, constants in `src/world/` document where
their value came from, and a constant is measured with production operators
before it is pinned, never picked first and justified afterwards. Every
displacement must come from a process or a sourced constant: no post-hoc
histogram remapping, no fitted curves or magic coefficients to hit a
number — a missed metric is a cause to find, not a result to edit. Where no
source fits, record it as an open question for the user instead of inventing
one. A world may leave Earth's parameter values (water inventory, land
fraction); its mechanisms may not leave physics. Product boundary: Sekai is a
map generator, not a predictive ecosystem, climate, or geodynamics simulator.
Only the atomically published final current state is a product; trajectories,
iterations, rejected steps, and high-accuracy references remain private solver
work. Physical mechanisms, units, causal ownership, conservation, numeric
domains, artifact identity, and final-state consistency are hard constraints.
Numerical strategy may use sourced and measured approximations—operator
splitting, explicit/implicit or predictor-corrector schemes, bounded iteration,
approximate linear solves, reduced coupling cadence, and multiresolution work
domains—just as one chooses an iterative solver for the same mathematical
problem. Choose the least expensive method that meets final-state quality and
performance; do not require trajectory convergence, history publication, or a
high-accuracy reference solve on every build without a demonstrated final-map
need. Validate new approximations offline on representative seeds against a
costlier reference path, while keeping conservation and final-state invariants
mandatory and following measure-before-pinning for any new tolerance.
Testing follows minimal sufficient evidence: every new or enlarged test must
name the contract, risk, or reproduced regression it covers and explain why a
smaller layer is insufficient. Prefer the cheapest effective layer; do not
blindly multiply seeds, profiles, resolutions, simulated duration, modules, or
assertions. Expand corpora only for genuinely statistical, cross-profile, or
cross-layer atomic properties, using the smallest representative set. Keep
high-cost references and sensitivity probes ignored, Release-only, and
offline; never add production APIs, schemas, history, or algorithm branches
just for tests. Reuse production formulas and validators, and narrow repeated
solves before increasing timeouts. This scope discipline does not remove the
explicit final regression, performance, or user UI gates below.
Acceptance: algorithm work is delivered only when it reaches the UI and the
user personally verifies it; agent self-checks are necessary but never
sufficient. Workflow: task-per-commit
plans in docs/superpowers/plans, frozen specs with explicit amendments,
fmt/clippy/wasm gates, release-mode iteration for the slow P5 suites with
a full debug regression at the end.
