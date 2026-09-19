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
- 精确冻结身份（表面指纹、矩阵哈希、全图金样、终态契约语料）只在审计浮点
  平台上有效：超越函数舍入随数学库与指令派发不同，同一种子在别的平台构出
  不同的世界。相关断言经 `world::spatial::audited_float_platform()` 金丝雀
  自检，不匹配时带说明跳过（与 GPU 金样钉审计适配器同一策略）；科学界限与
  一致性检查不受此限，任何平台都必须通过（2026-09-01，Docker ubuntu 与
  windows-latest 实测三平台三指纹后确立）。
- 本节约束测试设计与迭代期命令，不取消用户 UI 验收。测试范围由代理按改动
  的影响面自行决定并说明理由（用户指令，2026-08-28）：不再硬性要求任务收尾
  跑完整调试回归或固定的性能门禁；改动只碰一个模块就跑该模块与直接消费者
  的套件，碰到共享基础设施（求解器、重采样、schema、身份）再扩到 Release
  全量；调试档全量只在有具体理由（例如怀疑 debug 断言或溢出检查）时跑。

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

## Agent Guidelines for Rust Code Quality

This document provides guidelines for maintaining high-quality Rust code. These rules MUST be
followed by all AI coding agents and contributors.

Where this section conflicts with an earlier project-specific rule, the earlier rule remains
authoritative and the conflict is reconciled explicitly below; otherwise, every requirement in
this section applies unchanged.

### Your Core Principles

All code you write MUST be fully optimized.

"Fully optimized" includes:

- maximizing algorithmic big-O efficiency for memory and runtime
- using parallelization and SIMD where appropriate
- following proper style conventions for Rust (e.g. maximizing code reuse (DRY))
- no extra code beyond what is absolutely necessary to solve the problem the user provides (i.e.
  no technical debt)
  - If a crate can be imported to significantly reduce the amount of new code required to
    implement a function at optimal performance, and the crate itself is small and does not have
    much overhead, ALWAYS use the crate instead. The dependency still requires a concrete current
    consumer and the commit message MUST explain why it was added.

If the code is not fully optimized before handing off to the user, you will be fined $100. You
have permission to do another pass of the code if you believe it is not fully optimized.

“Fully optimized” does not override YAGNI, provenance, or measure-before-pinning: select the best
algorithmic complexity for the current requirement, and add parallelism, SIMD, caching, or a new
dependency only when a real consumer and measurement demonstrate the benefit. Do not add
speculative optimization machinery.

### Preferred Tools

- Use `cargo` for project management, building, and dependency management.
- Use `indicatif` to track long-running operations with progress bars. The message should be
  contextually sensitive.
- Use `serde` with `serde_json` for JSON serialization/deserialization.
- Use `ratatui` and `crossterm` for terminal applications/TUIs.
  - Include logical and intuitive mouse controls for all TUIs.
  - **ALWAYS** account for interface scrolling offsets when calculating click locations
- Use `axum` for creating any web servers or HTTP APIs.
  - Keep request handlers async, returning `Result<Response, AppError>` to centralize error
    handling.
  - Use layered extractors and shared state structs instead of global mutable data.
  - Add `tower` middleware (timeouts, tracing, compression) for observability and resilience.
  - Offload CPU-bound work to `tokio::task::spawn_blocking` or background services to avoid
    blocking the reactor.
- When reporting errors to the console, use `tracing::error!` or `log::error!` instead of
  `println!`.
- If the project involves the creation of images (e.g. PNG/WEBP), you have permission to use the
  Read tool to verify the rendered images fit the user and application requirements.
- If designing applications with a web-based front end interface, e.g. compiling to WASM or using
  `dioxus`:
  - All deep computation **MUST** occur within Rust processes (i.e. the WASM binary or the `dioxus`
    app Rust process). **NEVER** use JavaScript for deep computation.
  - The front-end **MUST** use Pico CSS and vanilla JavaScript. **NEVER** use jQuery or any
    component-based frameworks such as React.
  - The front-end should prioritize speed and common HID guidelines.
  - The app should use adaptive light/dark themes by default, with a toggle to switch the themes.
  - The typography/theming of the application **MUST** be modern and unique, similar to that of
    popular single-page web/mobile. **ALWAYS** add an appropriate font for headers and body text.
    You may reference fonts from Google Fonts.
  - **NEVER** use the Pico CSS defaults as-is: a separate CSS/SCSS file is encouraged. The design
    **MUST** logically complement the semantics of the application use case.
  - **ALWAYS** rebuild the WASM binary if any underlying Rust code that affects it is touched.
- For data processing:
  - **ALWAYS** use `polars` instead of other data frame libraries for tabular data manipulation.
  - If a `polars` dataframe will be printed, **NEVER** simultaneously print the number of entries
    in the dataframe nor the schema as it is redundant.
  - **NEVER** ingest more than 10 rows of a data frame at a time. Only analyze subsets of data to
    avoid overloading your memory context.
- If using Python to implement Rust code using PyO3/`maturin`:
  - Rebuild the Python package with `maturin` after finishing all Rust code changes.
  - **ALWAYS** use `uv` for Python package management and to create a `.venv` if it is not present.
    **NEVER** use the base system Python installation.
  - Ensure `.venv` is added to `.gitignore`.
  - Ensure `ipykernel` and `ipywidgets` is installed in `.venv` for Jupyter Notebook compatability.
    This should not be in package requirements.
  - **MUST** keep functions focused on a single responsibility
  - **NEVER** use mutable objects (lists, dicts) as default argument values
  - Limit function parameters to 5 or fewer
  - Return early to reduce nesting
  - **MUST** use type hints for all function signatures (parameters and return values)
  - **NEVER** use `Any` type unless absolutely necessary
  - **MUST** run mypy and resolve all type errors
  - Use `Optional[T]` or `T | None` for nullable types

### Code Style and Formatting

- **MUST** use meaningful, descriptive variable and function names
- **MUST** follow Rust API Guidelines and idiomatic Rust conventions
- **MUST** use 4 spaces for indentation (never tabs)
- **NEVER** use emoji, or unicode that emulates emoji (e.g. ✓, ✗). The only exception is when
  writing tests and testing the impact of multibyte characters.
- Use snake_case for functions/variables/modules, PascalCase for types/traits,
  SCREAMING_SNAKE_CASE for constants
- Limit line length to 100 characters (rustfmt default)
- Assume the user is a Python expert, but a Rust novice. Include additional code comments around
  Rust-specific nuances that a Python developer may not recognize.
- **MUST** avoid including redundant comments which are tautological or self-demonstating (e.g.
  cases where it is easily parsable what the code does at a glance or its function name giving
  sufficient information as to what the code does, so the comment does nothing other than waste
  user time)
- **MUST** avoid including comments which leak what this file contains, or leak the original user
  prompt, ESPECIALLY if it's irrelevant to the output code.

### Documentation

- **MUST** include doc comments for all public functions, structs, enums, and methods
- **MUST** document function parameters, return values, and errors
- Keep comments up-to-date with code changes
- Include examples in doc comments for complex functions

Example doc comment:

````rust
/// Calculate the total cost of items including tax.
///
/// # Arguments
///
/// * `items` - Slice of item structs with price fields
/// * `tax_rate` - Tax rate as decimal (e.g., 0.08 for 8%)
///
/// # Returns
///
/// Total cost including tax
///
/// # Errors
///
/// Returns `CalculationError::EmptyItems` if items is empty
/// Returns `CalculationError::InvalidTaxRate` if tax_rate is negative
///
/// # Examples
///
/// ```
/// let items = vec![Item { price: 10.0 }, Item { price: 20.0 }];
/// let total = calculate_total(&items, 0.08)?;
/// assert_eq!(total, 32.40);
/// ```
pub fn calculate_total(items: &[Item], tax_rate: f64) -> Result<f64, CalculationError> {
````

### Type System

- **MUST** leverage Rust's type system to prevent bugs at compile time
- **NEVER** use `.unwrap()` in library code; use `.expect()` only for invariant violations with a
  descriptive message
- **MUST** use meaningful custom error types with `thiserror`
- Use newtypes to distinguish semantically different values of the same underlying type
- Prefer `Option<T>` over sentinel values

### Error Handling

- **NEVER** use `.unwrap()` in production code paths
- **MUST** use `Result<T, E>` for fallible operations
- **MUST** use `thiserror` for defining error types and `anyhow` for application-level errors
- **MUST** propagate errors with `?` operator where appropriate
- Provide meaningful error messages with context using `.context()` from `anyhow`

### Function Design

- **MUST** keep functions focused on a single responsibility
- **MUST** prefer borrowing (`&T`, `&mut T`) over ownership when possible
- Limit function parameters to 5 or fewer; use a config struct for more
- Return early to reduce nesting
- Use iterators and combinators over explicit loops where clearer

### Struct and Enum Design

- **MUST** keep types focused on a single responsibility
- **MUST** derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate
- Use `#[derive(Default)]` when a sensible default exists
- Prefer composition over inheritance-like patterns
- Use builder pattern for complex struct construction
- Make fields private by default; provide accessor methods when needed

### Testing

- **MUST** write unit tests for all new functions and types that introduce an independently
  testable contract, risk, or reproduced regression. Do not add a redundant per-item test when the
  same fact is already covered by the smallest sufficient downstream contract.
- **MUST** mock external dependencies (APIs, databases, file systems), unless doing so would require
  a production abstraction or public API created only for testing; in that case, use the cheapest
  real fixture that proves the contract.
- **MUST** use the built-in `#[test]` attribute and `cargo test`
- Follow the Arrange-Act-Assert pattern
- Do not commit commented-out tests
- Use `#[cfg(test)]` modules for test code

### Imports and Dependencies

- **MUST** avoid wildcard imports (`use module::*`) except for preludes, test modules
  (`use super::*`), and prelude re-exports
- **MUST** document dependencies in `Cargo.toml` with version constraints
- Use `cargo` for dependency management
- Organize imports: standard library, external crates, local modules
- Use `rustfmt` to automate import formatting

### Rust Best Practices

- **NEVER** use `unsafe` unless absolutely necessary; document safety invariants when used
- **MUST** call `.clone()` explicitly on non-`Copy` types; avoid hidden clones in closures and
  iterators
- **MUST** use pattern matching exhaustively; avoid catch-all `_` patterns when possible
- **MUST** use `format!` macro for string formatting
- Use iterators and iterator adapters over manual loops
- Use `enumerate()` instead of manual counter variables
- Prefer `if let` and `while let` for single-pattern matching

### Memory and Performance

- **MUST** avoid unnecessary allocations; prefer `&str` over `String` when possible
- **MUST** use `Cow<'_, str>` when ownership is conditionally needed
- Use `Vec::with_capacity()` when the size is known
- Prefer stack allocation over heap when appropriate
- Use `Arc` and `Rc` judiciously; prefer borrowing

### Benchmarking and Optimization

- **NEVER** run benchmarks in parallel, as the benchmarks will compete for resources and the
  results will be invalid
- **NEVER** game the benchmarks. Do not manipulate the benchmarks themselves to satisfy any
  required performance constraints
- **NEVER** run benchmarks with `target-cpu=native` or any other `RUSTFLAGS`
- If benchmarking against another crate or library, ensure the benchmarks are apples-to-apples
  comparisons
- Ensure benchmark tests are independent. If the tests are dependent due to a feature (e.g.
  caching), ensure the feature is disabled

### Concurrency

- **MUST** use `Send` and `Sync` bounds appropriately
- **MUST** prefer `tokio` for async runtime in async applications
- **MUST** use `rayon` for CPU-bound parallelism
- Avoid `Mutex` when `RwLock` or lock-free alternatives are appropriate
- Use channels (`mpsc`, `crossbeam`) for message passing

### Security

- **NEVER** store secrets, API keys, or passwords in code. Only store them in `.env`
  - Ensure `.env` is declared in `.gitignore`
- **MUST** use environment variables for sensitive configuration via `dotenvy` or `std::env`
- **NEVER** log sensitive information (passwords, tokens, PII)
- Use `secrecy` crate for sensitive data types

### Version Control

- **MUST** write clear, descriptive commit messages
- **NEVER** commit commented-out code; delete it
- **NEVER** commit debug `println!` statements or `dbg!` macros
- **NEVER** commit credentials or sensitive data

### Tools

- **MUST** use `rustfmt` for code formatting
- **MUST** use `clippy` for linting and follow its suggestions
- **MUST** ensure code compiles with no warnings (use `-D warnings` flag in CI, not
  `#![deny(warnings)]` in source)
- Use `cargo` for building, testing, and dependency management
- Use `cargo test` for running tests
- Use `cargo doc` for generating documentation
- For projects which build a Python package, **NEVER** build with
  `cargo build --features python`: this will always fail. Instead, **ALWAYS** use `maturin`.
- **NEVER** uses the `Explore` tool for `Cargo.lock`: it is large and irrelevant. Read
  `Cargo.lock` **ONLY** if it's extremely relevant.

### Before Committing

- [ ] All tests pass (`cargo test`; for this project, follow the targeted Release/full debug
      regression split below)
- [ ] No compiler warnings (`cargo build`)
- [ ] Clippy passes
      (`cargo clippy --workspace --all-targets --all-features -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --all -- --check`)
- [ ] If the project creates a Python package and Rust code is touched, rebuild the Python package
      (`source .venv/bin/activate && maturin develop --release --features python`)
- [ ] If the project creates a WASM package and Rust code is touched, rebuild the WASM package
      (`wasm-pack build --target web --out-dir web/pkg`)
- [ ] All public items have doc comments
- [ ] No commented-out code or debug statements
- [ ] No hardcoded credentials

---

**Remember:** Prioritize clarity and maintainability over cleverness. This is your core directive.

## 工作流程

- 较大特性走 superpowers 纪律：`docs/superpowers/plans/` 里的任务化
  计划、一任务一提交；锁定设计入 `docs/superpowers/specs/`，实现期偏离
  以修订条目显式记录。
- 项目文档（plans / specs / 完成记录）用中文撰写；引文、代码标识符与
  常量名保留原文。已完结的历史文档不回头翻译。（用户指令，2026-08-19）
- 门禁（提交前）：`cargo fmt --all -- --check`；
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`；
  `cargo check --target wasm32-unknown-unknown --all-features --lib`；
  测试范围按上文“测试范围纪律”由代理自行决定并在交付说明里写明跑了什么。
- P5 全链集成套件在调试档极慢（全量约 40 分钟）：用 `--release` 跑目标
  套件；应用占着 `target/release/sekai.exe` 时用 `CARGO_TARGET_DIR=target/probe`。
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
fmt/clippy/wasm gates, and a test scope the agent chooses by blast radius and
states in the hand-off (no mandatory full debug regression or fixed
performance gate; user instruction 2026-08-28).
