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
of branching existing paths). Acceptance: algorithm work is delivered only
when it reaches the UI and the user personally verifies it; agent
self-checks are necessary but never sufficient. Workflow: task-per-commit
plans in docs/superpowers/plans, frozen specs with explicit amendments,
fmt/clippy/wasm gates, release-mode iteration for the slow P5 suites with
a full debug regression at the end.
