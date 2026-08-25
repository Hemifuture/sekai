# P5/Formation 应用发布事务实施计划

> **状态：部分回溯。** 本文件 2026-08-24 补写。§A 的 M1／M2／M3／L3 四项**实现
> 已经先于本计划发生**（同一未提交工作树，先改代码与规格、后补计划），本文件
> 对那部分只做**如实回溯记录**，抄录当时真实留下的 RED 命令与失败断言，
> **不伪称是事前计划**。§B 是本轮（复审后续修复）的前瞻计划，按"先 RED 后
> GREEN"执行，其证据在各任务内实测记录。
>
> 补写动机：独立只读复审的 M-1 条——AGENTS.md《工作流程》要求较大特性走
> superpowers 纪律（`docs/superpowers/plans/` 里的任务化计划、一任务一提交），
> 而 M1-M3 那轮没有任何计划／RED 载体，"新测试确实先红过"无法被第三方核验。
>
> §C 是**第二轮独立复审**的后续修复（本文件与规格同轮更新），同样按"先 RED
> 后 GREEN"执行，构造不出 RED 的明说无 RED 并附变异探针。

**Goal:** 让 `src/app.rs` 世界构建装配路径上的发布事务在**取消、失败、成功**
三类结局下都原子：工作 `MemoryStageCache` 与 amplified 显示包只在发布成功后
提交，取消在指针按下即线性化，且**结算所在的那个 pass 也必须先把取消输入交给
按钮**。

**Architecture:** 事务边界全部在应用装配层，不进入 `generators`／`world`：
`prepare_pending_world_build` 分叉工作 cache 并保留发布快照，
`settle_world_build_stage_cache` 依发布结果二选一；`PendingWorldBuild.completion`
记录入栈 pass 号做同-pass 防重；`TemplateApp::update` 的顺序
（画状态行 → `poll_world_build` → actions → canvas）本身就是不变量的一部分。
设计事实源是 `docs/superpowers/specs/2026-08-08-spherical-presentation-design.md`
§17 R1。

**Tech Stack:** Rust 2024、egui/eframe 0.31.1、既有 `engine::cache`／
`engine::cancellation`、cargo。无新增依赖。

---

## 执行纪律

- §B／§C 每项严格 RED → GREEN；无法构造 RED 的（生产行为本就正确、只补守门）
  **明说无 RED**，改用变异探针证明测试有鉴别力，不把绿测试包装成 RED→GREEN。
- 迭代期用 `--release` 跑目标套件（AGENTS：P5 全链调试档约 40 分钟）。
- 测试复用生产侧助手（`GeodesicVoronoiBuilder`、`HierarchicalEvaluator`、
  `build_spherical_presentation_candidate_for_view`、`BuildReport::cache_hits()`），
  不重新实现算法。
- 本轮任务约束为**不提交**：全部改动留在工作树。"一任务一提交"的拆分建议
  写在文末，由用户决定何时执行。
- 不触碰工作树中范围外的 P5 科学／地貌改动，不删除 `debug.log` 等用户文件。

---

## §A 已发生的实现（回溯记录）

四条不变量与其守门测试已经落在 `src/app.rs` 与规格 R1 中。下列 RED 证据抄自
当时的实现报告，命令与失败断言为原文；**原始记录未附退出码**，本轮同类失败
实测 `cargo test` 退出码为 101。

### A1（M1）初始安装非原子：amplified 状态先于发布提交

- 生产改动：`install_initial_spherical_candidate` 把 `store_amplified_bundle` /
  `upload_amplified_display` 从 `PublishedSphericalPresentation::try_new` **之前**
  移到其成功**之后**，与 replacement 路径同构。
- RED（改生产代码前）：

```
$ cargo test --release --lib natural_app_tests::failed_initial_formation_install_commits_no_amplified_display_state
test app::natural_app_tests::failed_initial_formation_install_commits_no_amplified_display_state ... FAILED
thread '...' panicked at src\app.rs:3665:9:
a failed install must leave no amplified mesh
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 555 filtered out
```

- 触发用的是**真实生产守卫**：replacement 血缘候选走 initial 安装路径，
  `candidate.lineage.validate_initial()` 拒绝，无 test-only 注入。
- GREEN：`failed_initial_formation_install_...` 与反向守门
  `successful_initial_install_commits_the_amplified_display_state` 同时通过。

### A2（M2）按住取消被抬起-pass 语义吞掉

- 生产改动：`show_pending_world_build_status` 由 `cancel.clicked()` 改为
  `cancel.clicked() || cancel.is_pointer_button_down_on()`。
- RED：

```
$ cargo test --release --lib -- natural_app_tests::held_cancel_button_precedes_a_same_pass_completion_commit
test app::natural_app_tests::held_cancel_button_precedes_a_same_pass_completion_commit ... FAILED
thread '...' panicked at src\app.rs:3858:9:
holding 取消 must linearize the cancellation before publication
test result: FAILED. 0 passed; 1 failed
```

- GREEN 时的连带失败（已如实记录）：
  `same_frame_cancel_precedes_successful_world_and_cache_commit` 一度失败于
  `assertion failed: !cancellation.is_cancelled()`（`src\app.rs:4081`）——该断言
  编码的正是旧的"只有抬起才取消"语义，按已批准设计改为
  `assert!(cancellation.is_cancelled())`。
- 反向守门：`keyboard_activated_cancel_still_cancels_the_pending_build`
  全程无指针事件（Tab + Space），守 `clicked()` 分支。

### A3（M3）同一 pass 内第二次 poll 立即结算

- 生产改动：`PendingWorldBuild.completion` 改为
  `Option<(u64, WorldBuildCompletion)>` 记录入栈 `cumulative_pass_nr`，同 pass
  拒绝结算。
- RED：

```
$ cargo test --release --lib -- natural_app_tests::a_second_poll_in_the_same_pass_stages_without_settling
test app::natural_app_tests::a_second_poll_in_the_same_pass_stages_without_settling ... FAILED
thread '...' panicked at src\app.rs:3811:9:
a second poll inside the staging pass must not settle it
test result: FAILED. 0 passed; 1 failed
```

### A4（L3）取消判定排在 render state guard 之后

- 生产改动：`settle_staged_world_build` 把 `cancellation.is_cancelled()` 上移到
  `render_state` 可用性检查之前。
- RED：

```
$ cargo test --release --lib -- natural_app_tests::cancellation_is_decided_before_the_render_state_guard
test app::natural_app_tests::cancellation_is_decided_before_the_render_state_guard ... FAILED
thread '...' panicked at src\app.rs:3893:9:
assertion `left == right` failed
  left: Some("渲染状态不可用，无法发布新世界")
 right: Some("已取消本次世界构建")
test result: FAILED. 0 passed; 1 failed
```

### A5 没有 RED 的那一批（如实说明）

cache 回滚族（`formation_cache_fork_is_shared_...`、
`legacy_world_build_keeps_moving_...`、`formation_channel_disconnect_...`、
`formation_initial_and_replacement_install_failures_...`）直接调用**当时新增的**
`prepare_pending_world_build` / `settle_world_build_stage_cache`。这些符号在修复
前根本不存在，测试**编译不过**，因此它们的"RED"含义是"不存在"而不是"失败"。
本条按复审 M-1 的要求写明，不冒充 RED→GREEN。

---

## §B 本轮任务（复审后续修复）

### Task 1（复审 L-1）：结算所在 pass 的取消输入必须先被按钮消费

**Files:** Modify `src/app.rs`

**因果**：`update` 首行 `poll_world_build`，而取消按钮要到同一 pass 的
`SidePanel` 才绘制。一次"物理按下发生在 pass N 快照之后、pass N+1 快照之前"的
取消，其事件已在 pass N+1 的 `RawInput` 里，却在结算之后才被 widget 消费——
这一下按下被吞掉，世界照常发布。A3 的同-pass 计数器不覆盖这个窗口（它只保证
接收与结算不同 pass），A2 的 pointer-down 只覆盖"用户已经按住"的场景。

#### Step 1：先写 RED

新增 `pointer_down_in_the_settling_pass_precedes_the_world_and_cache_commit`：
pass 1 只定位按钮（无任何指针事件），pass 2 无输入地 stage completion，
pass 3 才第一次按下取消——正是被允许结算的那个 pass。

```
$ cargo test --release --lib -- natural_app_tests::pointer_down_in_the_settling_pass_precedes_the_world_and_cache_commit
test app::natural_app_tests::pointer_down_in_the_settling_pass_precedes_the_world_and_cache_commit ... FAILED
thread '...' panicked at src\app.rs:4149:9:
the settling pass must draw 取消 and observe its input before committing
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 561 filtered out
退出码 = 101
```

#### Step 2：最小实现

- `TemplateApp::update`：把 `self.poll_world_build(ctx)` 从函数首行移到左侧控制
  面板 `SidePanel::show` 结束之后、`for action in field_actions` 之前。顺序变为
  **画状态行 →（本 pass 取消输入已被消费）→ poll/结算 → actions → canvas**，
  本 pass 的画布因此画的是结算后的世界。
- 结算之后 `ctx.request_repaint()`：本 pass 的面板摘要是按结算前的世界画的，
  需要下一 pass 追上。
- `poll_amplified_detail` 保持在首行不动；顺序变化的副作用是本 pass 里
  world-build 错误**最后**写入 `spherical_runtime_error`，后台细节错误不再可能
  覆盖它，与 R1.2 同向。
- 测试侧 `run_world_build_frame` 同步改成"先画按钮、后 poll"，注释仍如实标注
  它复刻的是生产顺序。
- 不加时间宽限、不加魔法帧数、不改 pass 计数逻辑。

#### Step 3：GREEN 与连带修正

```
$ cargo test --release --lib -- natural_app_tests
test result: ok. 47 passed; 0 failed; 0 ignored; 0 measured; 516 filtered out
```

中间态如实记录：改顺序后
`same_frame_cancel_precedes_successful_world_and_cache_commit` 先失败于
`assertion failed: run_world_build_frame(...).is_none()`（`src\app.rs:4411`，
退出码 101）。该断言编码的是旧顺序下"结算 pass 不再画按钮"的事实；新顺序里
结算 pass **必须**先画按钮（这正是修复本身），故改为断言该 pass 仍返回按钮矩形、
其后 `world_build` 已为 `None`，再多跑一个 pass 才返回 `None`。

### Task 2（复审 L-3）：初始路径成功时提交工作 cache 的端到端守门

**Files:** Modify `src/app.rs`

**因果**：R1.1 的成功侧矩阵里"initial（`replacement = false`）+ 成功 → 工作副本
成为新发布副本"没有任何测试。`a_second_poll_in_the_same_pass_...` 覆盖的是
`replacement = true`；`successful_initial_install_commits_the_amplified_display_state`
直接调用 `install_initial_spherical_candidate`，绕过 `settle_staged_world_build`，
完全不检查 cache 提交。

**无 RED（如实说明）**：生产行为本就正确，构造不出真实 RED。改用**变异探针**
证明新测试有鉴别力：把 `settle_world_build_stage_cache(..., install.is_ok())`
临时改成 `false` 后

```
thread '...' panicked at src\app.rs:4077:9:
assertion `left == right` failed: a published initial world commits its working fork, not the retained snapshot
  left: 17
 right: 32
test result: FAILED. 0 passed; 1 failed
退出码 = 101
```

变异已即刻还原（`install.is_ok()` 复原，工作树无残留）。

**实现**：新增 `successful_initial_settlement_commits_the_working_stage_cache`。
fixture 按生产真相构造——生产只有在**尚无发布**时才走 initial 路径
（`request_spherical_world_build` 以 `replacement_token()` 是否存在判定），
所以 app 不发布任何世界，改用一个无关种子（99）预热已发布 cache，再把
`RootSeed::new(app.world_seed)` 的真实候选建进工作副本，经
`settle_queued_world_build` 完整结算后断言 `stage_cache.len() == working_len`。
按 R1.1 的两档规则，这是非关键路径，用严格长度差异即可判别（实测 17 → 32）。

```
$ cargo test --release --lib -- natural_app_tests::successful_initial_settlement_commits_the_working_stage_cache
test ... ok
```

### Task 3（复审 L-4）：内容探针"确实命中"的前提集中守住

**Files:** Modify `src/app.rs`

**因果**：`assert!(identity_before.0 > 0)` 只写在一个调用点。若 fixture 漂移到
零命中，`assert_eq!(published_cache_hits(...), identity_before)` 会退化成
`(0, n) == (0, n)` 的自证恒等式，静默失去鉴别力。

**实现**：把该前提移进 `published_cache_hits` 自身，删除调用点上的重复断言，
并在 helper 文档里写明理由。这是 SSOT：前提只有一处定义。无 RED（不改变生产
行为）。

本轮结束时受保护的调用点是**四个**测试：
`failed_initial_formation_install_commits_no_amplified_display_state`、
`cancellation_is_decided_before_the_render_state_guard`、
`held_cancel_button_precedes_a_same_pass_completion_commit`、
`pointer_down_in_the_settling_pass_precedes_the_world_and_cache_commit`
（§C2 把 `same_frame_cancel_precedes_successful_world_and_cache_commit` 也接上，
之后为五个）。

### Task 4（复审 L-5）：`test_amplified_bundle` 注释如实化

**Files:** Modify `src/app.rs`

原注释称该 bundle "stores exactly what a formation worker hands it"，但只有
`detail`（`HierarchicalEvaluator`）与 `river_radius_m` 来自生产算子，`mesh` 是
手搓单三角形、`rivers` 为空、`initial_hash` 为 0。改为如实描述：detail context
来自生产 T1 v2；mesh／rivers 是最小占位，因为被测行为（存入与清空）不依赖其
内容。AGENTS.md §2 要求文档不复述错误事实。无 RED（纯注释）。

### Task 5（复审 M-2／L-2／L-6）：规格 R1 修订

**Files:** Modify `docs/superpowers/specs/2026-08-08-spherical-presentation-design.md`

R1 是本工作树内尚未提交的新增条目，因此**就地改正其措辞**而不是再加一条与之
矛盾的 R2（SSOT：一个事实只有一处定义）。四处：

1. **R1.1 断言方式**（M-2）：原文把 3/8 的做法写成全体做法。改为两档——关键
   回滚路径（初始安装失败、取消、render state 缺失）用 `cache_hits()`/
   `cache_misses()` 内容探针；其余路径先断言工作副本严格长于发布副本，条目数
   即可判别。并写明探针有效性前提集中在 `published_cache_hits` 内部（Task 3），
   补上成功侧（初始与替换都须经完整 settle 证明提交，Task 2）。
2. **R1.1 豁免**（L-6）：`formation_surface` 不在事务内——`FormationSurfaceCacheEntry`
   按 `(profile, radius_m)` 内容寻址（取用前过 `formation_surface_key_is_stale`），
   是纯派生缓存，不参与发布血缘、不进 artifact 指纹，因此取消／`Err`／安装失败
   三条路径都无条件回填，省一次昂贵的测地面重建。刻意的不对称，不是遗漏。
3. **R1.3 语义补全**（L-2）：pointer-down 使取消**不可撤销**——按下的那一 pass
   已调用 `BuildCancellation::cancel()`；其不可逆 `store(true)` 已将取消线性化，
   所以后续拖出再释放也不改变结果。GUI 常见的
   "滑开撤销"逃生口在此路径不适用，是为消除 release-pass 竞态接受的取舍。
4. **R1.4 强化**（Task 1）：由"同一 pass 内 stage 后禁止 settle"扩为"接收与结算
   之间必须隔着一个**取消按钮能处理输入**的 pass"，明确它由两个互补部件共同
   成立：同-pass 计数器 + `update` 内"先画状态行、后 poll"的顺序。并保留
   `cumulative_pass_nr` 只是同-pass 防重、不是墙钟／时间保证的原有澄清。

### Task 6（复审 M-1）：补本计划文件

**Files:** Create `docs/superpowers/plans/2026-08-24-p5-publish-transaction.md`

即本文件。§A 如实回溯、§B 前瞻执行，见开头状态声明。

---

## §C 第二轮独立复审的后续修复

### Task C1（复审 1）：worker 线程意外终止后显式请求重绘

**Files:** Modify `src/app.rs`

**因果**：`stage_world_build_completion` 的 `TryRecvError::Disconnected` 分支写入
"世界构建线程意外终止"时，本 pass 的状态行早已画完（Task 1 的顺序），而这一分支
同时取走 `world_build`——`Empty` 分支的 150 ms `request_repaint_after` 与状态行的
`Spinner` 一起消失，没有任何部件会再要求下一帧。反应式集成因此可能睡到下一次
外部输入才显示这条错误。该分支的可见性不应依赖 `Spinner` 动画这个无关部件。

#### Step 1：先写 RED（含一次无鉴别力的初版，如实记录）

初版探针直接在新建的 `egui::Context` 上跑一个 pass 就断言重绘，**绿了**：

```
$ cargo test --release --lib -- natural_app_tests::formation_channel_disconnect_restores_published_stage_cache
test app::natural_app_tests::formation_channel_disconnect_restores_published_stage_cache ... ok
```

实测原因（探针打印）：`Context` 的**第一个** pass 永远请求一次立即重绘
（`delay=0ns`），其后的空 pass 才回到 `Duration::MAX`。加一个预热 pass 后 RED 立刻
出现：

```
$ cargo test --release --lib -- natural_app_tests::formation_channel_disconnect_restores_published_stage_cache
thread '...' panicked at src\app.rs:4562:9:
assertion `left == right` failed: a dead worker must wake the UI to show its error
  left: 18446744073709551615.999999999s
 right: 0ns
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 562 filtered out
退出码 = 101
```

观测量是 egui 的**集成契约**而不是内部状态：`Context::run` 返回的
`FullOutput.viewport_output[ViewportId::ROOT].repaint_delay`，其文档规定"时长为零
即立即重绘"（`egui-0.31.1/src/viewport.rs:1141`）。测试让 `poll_world_build` 单独
跑在预热过的 context 里，正是为了让探针只测这一分支——真实状态行的 `Spinner`
每 pass 都请求重绘，会把分支自身的请求掩盖掉。

守门测试没有新建：复用既有的
`formation_channel_disconnect_restores_published_stage_cache` 及其 fixture（DRY），
它本就是这条路径的唯一守门。

#### Step 2：最小实现

`Disconnected` 分支在写入错误消息之后加一行 `ctx.request_repaint()`；不加计时器、
不动其它分支、不改 `update` 顺序。

#### Step 3：GREEN

```
$ cargo test --release --lib -- natural_app_tests::formation_channel_disconnect_restores_published_stage_cache
test ... ok. 1 passed; 0 failed; 0 ignored; 0 measured; 562 filtered out
```

设计真相同步落在规格 R1.4 的重绘句里（SSOT：重绘规则只有一处定义）。

### Task C2（复审 2）：取消路径的内容探针

**Files:** Modify `src/app.rs`

**因果**：R1.1 把"取消"列为**关键回滚路径**、要求用内容探针，但
`same_frame_cancel_precedes_successful_world_and_cache_commit` 只断言
`stage_cache.len() == published_len`——测试与规格不一致。

**无 RED（如实说明）**：生产行为本就正确。改用**变异探针**证明新断言有鉴别力：
把 `prepare_pending_world_build` 的 retained 快照临时改成
`MemoryStageCache::default()`，并临时注释掉本测试的长度断言，使新断言成为唯一
把关者：

```
thread '...' panicked at src\app.rs:3654:9:
the probed cache must actually serve the published seed
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 562 filtered out
退出码 = 101
```

两处变异已即刻还原，工作树无残留。

**两档断言是并列而不是替代**（补充 R1.1 的读法）：工作副本是发布副本的**超集**，
用发布种子探它得到的命中／未命中与发布副本**相同**——"错误地提交了工作副本"只有
长度断言能判别；反过来"同样长度的新壳"只有内容探针能判别。故该测试同时保留
两者。受 `published_cache_hits` 前提保护的调用点因此从四个变为五个（见 §B Task 3）。

### Task C3（复审 3）：改正 §B Task 3 的调用点计数

**Files:** Modify `docs/superpowers/plans/2026-08-24-p5-publish-transaction.md`（本文件）

原文写"三个调用点"，实测当时是四个测试在用 `published_cache_hits`。已按实测
逐个列名改正，并注明 §C2 之后为五个。无 RED（文档）。

### Task C4（复审 4）：规格 R1.3 删除不精确的理由

**Files:** Modify `docs/superpowers/specs/2026-08-08-spherical-presentation-design.md`

原文用"`is_pointer_button_down_on()` 在拖出期间仍锁定该 id"来解释"拖出再释放
也已取消"。这个理由既依赖 egui 内部的 id 归属细节，也不是真正的原因：**按下的
那一 pass 就已经调用了不可逆的 `BuildCancellation::cancel()`**，此后按钮是否还
认为自己被按住都改变不了结果。已就地改成后者。无 RED（文档）。

### Task C5（复审 5）：规格 R1.2 的"无后台错误源"限定到初始路径／本 pass

**Files:** Modify `docs/superpowers/specs/2026-08-08-spherical-presentation-design.md`

原文把"不存在能覆盖真正安装错误的后台错误源"写成无条件结论。实测生产：
**初始路径**成立（此前没有发布、没有细节引擎，失败后也没有新引擎装上）；
**替换路径**不成立——上一份发布的细节引擎仍在运行，`poll_amplified_detail`
（`update` 首行）在**后续 pass** 里可以用它的失败覆盖同一个
`spherical_runtime_error`。**本 pass 内**不会，因为 world-build／安装错误在
`update` 顺序上写在最后。已按这三档就地改写。无 RED（文档）。

---

## 门禁

```powershell
cargo fmt --all -- --check
cargo test --release --lib -- natural_app_tests
cargo test --release --lib engine::cache
cargo test --release --test build_cancellation
cargo test --release --test engine_execution
git diff --check
```

§C 收尾实测（全部退出码 0）：`cargo fmt --all -- --check`；
`natural_app_tests` 47 passed / 0 failed；`engine::cache` 2 passed；
`build_cancellation` 7 passed；`engine_execution` 25 passed；
`git diff --check` 无空白错误。

提交前仍须按 AGENTS.md 补齐（本轮未跑，任务未要求）：
`cargo clippy --workspace --all-targets --all-features -- -D warnings`、
`cargo check --target wasm32-unknown-unknown --all-features --lib`、完整调试回归。
其中 clippy 当前被**范围外**的既有 P5 科学算法告警阻断（8 条，全在
`surface_formation/*` 与 `world/natural/surface_formation.rs`，`src/app.rs` 零命中）。

## 提交拆分建议（本轮不执行）

本轮任务约束为不提交。若日后按"一任务一提交"拆分，建议顺序与主题：

1. `Settle world builds after the cancel button sees the pass`（Task 1）
2. `Guard the initial publication's cache commit`（Task 2）
3. `Centralize the published-cache probe precondition`（Task 3 + Task 4）
4. `Amend R1 for probe tiers, cancel finality and formation surface`（Task 5）
5. `Record the P5 publish transaction plan`（Task 6）
6. `Repaint after a dead world-build worker`（Task C1）
7. `Probe the cancelled rollback's cache content`（Task C2）
8. `Correct R1.2, R1.3 and the plan's probe count`（Task C3 + C4 + C5）

## 范围外与开放问题

- **L-7（未做）**：`MemoryStageCache` 的 `Clone` 建议收成语义化的
  `fork_for_publication()`。要改 `src/engine/cache.rs`，本轮范围只含
  `src/app.rs` + 规格 + 本计划，交用户裁定。
- **L-8（未做）**：`formation_initial_and_replacement_install_failures_...` 只断言
  `spherical_runtime_error.is_some()`，未分别断言两条分支的失败种类。精度问题，
  非有效性问题（安装成功会把该字段置 `None`，测试不会假阳性）。
- **L-9（不处理）**：仓库根未跟踪的 `debug.log` 来自 Orca 宿主的 crashpad，
  非本任务产物；用户文件不删。
- 工作树内范围外的 P5 地貌／气候改动（`surface_formation/*`、`world/natural/*`、
  `tests/formation_*`）未触碰；`cargo test --release --lib` 全量里
  `global_circulation::forcing::formation_tests::formation_terrain_reuses_exact_p4_forcing_and_changes_checkpoint_causally`
  的既有失败属于那一侧，不在本事务范围内。

## 每项承重技术的出处

本计划不引入任何算法、公式、常量或阈值——它是应用装配层的**事务顺序**修复，
承重的是既有框架语义与项目内既有事实源：

- **两阶段提交／写入只在提交点生效**——数据库事务的标准原子性论述
  （Gray & Reuter, *Transaction Processing: Concepts and Techniques*, 1993，
  原子性与 shadow-copy 回滚）。工作副本 + 保留快照 + 单点提交即 shadow paging
  的最小形态。
- **输入在一帧内的线性化点**——egui 0.31.1 的 immediate-mode 输入契约：
  `Response::clicked()` 只在抬起 pass 触发（`response.rs:154`，键盘激活走
  `FAKE_PRIMARY_CLICKED`），`is_pointer_button_down_on()` 在按下那一 pass 即为真；
  项目侧 `BuildCancellation::cancel()` 随即执行不可逆 `store(true)`。
  `cumulative_pass_nr` 在 `end_pass` 自增。顺序结论直接来自这些语义，不是自拟规则。
- **"先绘制、后提交"的顺序**——immediate-mode GUI 的既有实践：一帧的输入只有
  在消费它的 widget 绘制之后才算已处理（Muratori 的 IMGUI 输入模型，2005 起；
  egui 的 `Context::run` 即该模型的实现）。
- **取消的不可逆性**——项目内既有事实源 `crate::engine::BuildCancellation`
  （`store(true)`，幂等），不新增语义。
- **cache 身份用命中率而非条目数判别**——项目内既有生产事实源
  `BuildReport::cache_hits()` / `cache_misses()`；测试复用生产候选构建链路
  （AGENTS.md §2「测试必须复用生产侧助手」）。
- **`(profile, radius)` 内容寻址的派生缓存可无条件回填**——项目内既有
  `formation_surface_key_is_stale`，与 artifact 指纹这一身份真相正交。

## 用户 UI 验收步骤（AGENTS 验收纪律）

代理自检不充分，以下须由用户亲自在 UI 上确认：

1. `cargo run --release` 启动，进入球面画布，Formation 管线下点"按当前参数重建"。
2. 状态行出现"正在生成世界…"后，**按住**"取消"不放直到构建结束。预期：世界不被
   替换，显示"已取消本次世界构建"，当前世界及其放大网格／河网不变。
3. **本轮新增场景**：让构建**几乎完成时**再单击"取消"（按下即可，不必按住）。
   预期：即使 worker 恰在上一帧完成，这一下按下也不会被吞——世界不被替换，
   同样显示"已取消本次世界构建"。这是 Task 1 修的窗口。
4. 用 Tab 把焦点移到"取消"，按空格。预期：与鼠标一致地取消。
5. 正常完成一次生成。预期：新世界发布后放大网格与河流同时可见；左侧面板的
   面积依从性摘要在结算后的下一帧内更新为新世界的数值，不停在旧值上。
6. 已知语义（规格 R1.3）：在"取消"上按下后拖开再松手，构建**也已经取消**，
   不可反悔。请确认这一取舍可接受。
7. Task C1 的路径（构建线程意外终止）**无法在正常操作下触发**，此处只记录预期：
   状态行消失、出现"世界构建线程意外终止"，且**不需要再动鼠标或键盘**这条消息
   就会显示出来。
