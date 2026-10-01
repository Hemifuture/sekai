# 最终集中 fix wave 报告

状态：I1/I2/M2 源码与最低成本控件/worker 证据 Addressed；fresh wasm gate、当前原生窗口代理证据、唯一独立 scoped review 已 PASS。最终 Spec PASS / Quality PASS，I1/I2/M2 Addressed，无新 C/I/M（0/0/0）；见 `final-fix-review.md` 与 `screenshots/final-fix/ui-report.md`，root 均已完整阅读。原生生产源码为 `50e379c`，最终测试后继为 `a369501`。用户本人 UI 验收仍 PENDING，不将代理窗口或 FirstSubmitted 视为 OS present。

BASE `516c23c64eddb7f083caf4ab4a8d2eb6d6805bdf`；源码提交 `50e379cfe2b918bdd8a50445d318a7a7f082d13d`，branch `codex/phase-a-authoring`。本地提交，未 push/merge/deploy。实施沿用获授权的 gpt-6.1-sol/high 上下文，未派子代理。

## I1 — 百分比单位与配方精度

两个实际 Slider 以百分比显示、解析；`40%` 与 `40` 均为领域比例 `0.40`。先转为真实 f32 领域值，再使用既有 MIN/MAX 范围校验，未知格式、非有限和域外文本拒绝。显示保留非整数百分比；未改动文本确认返回已有 f32 位，不生成新配方身份或撤销构建权限。测试在实际 egui 控件上确认 Enter、Escape、两种合法输入以及非整数已有值的 recipe/revision/token/no-op。显式 mark_saved 前置的既有 fixture 补证已完成：原生 helper 退出后仅加两行标记已保存并确认 clean，精确该测试 1 PASS，不新增 fixture，也不改变生产行为。

## I2 — egui 夹值/舍入前拒绝板块文本

板块解析为 u16 并复用 `MIN_PLATE_COUNT..=MAX_PLATE_COUNT`；`0`、`999`、`2.5` 在 egui 转换前拒绝，不提交替代合法值。实际键盘覆盖 Enter/Tab、revision/token/history 保持、原文保留、保存阻挡、纠正与 Escape。

拒绝状态仅保存字段、原数字编辑器 ID 和原文；供错误反馈/格式化/焦点恢复消费，不进入 AuthorInputs 或持久化格式。两项实际 egui 行为必须处理：DragValue 获得焦点会清除自己的临时 String；Slider 在未改动 Enter 时不再次解析，且失焦后可返回轨道 ID。现保留原数字编辑器 ID，恢复该编辑器，防止把键盘焦点移到滑条轨道。合法就地 Ctrl+A 纠正才提交一次事务。Undo/Redo、新项目/发布清理及放弃未保存编辑的既有入口同时清除此临时拒绝状态。

## M2 — 当前请求真实粗阶段

`PendingWorldBuild` 独占一个共享当前阶段，初始化地表准备，在生产实际 graph 入口进入自然世界构建，graph 成功后进入显示准备/发布；两个现有 pipeline 均采用相同真实粗边界。状态行同时显示当前阶段、elapsed、取消状态及入口，计时观测关闭时亦工作。独立请求拥有独立共享对象；不保存阶段、不参与输入身份、不拆分 bundled P2–P5，不引入进度百分比、ETA 或通用框架。

最低成本真实 worker 用现有 LegacyFoundation 162 单元验证，不运行昂贵 Formation：观测关闭仍到达显示准备，真实渲染状态文字存在；旧请求阶段对象不会修改新请求初始阶段；项目 wire 不含阶段或拒绝文本。原单槽、worker guard、取消/旧世界回退及发布先后顺序未修改。

## 实际 RED → GREEN 与原始记录

- `final-fix-red.log`：原生产代码 + 实际 egui 测试，5 executed，1 PASS / 4 FAIL，exit 101；编译 47.56s，行为 0.01s。直接复现 `40%` 留在 0.38、`0` Enter 变成 2、非法百分比失焦及无阶段文字。已有非整数 no-op 测试初始通过，不能将其说作 RED。
- `final-fix-green-first.log` / `final-fix-green-diagnose.log`：3 PASS / 2 FAIL；`final-fix-green-second.log`：4 PASS / 1 FAIL。`final-fix-slider-diagnose.log`：1 FAIL，定位未改动 Enter 未重新解析。
- `final-fix-green-third.log`：原 5 项全部 PASS。随后保留新增消费者检查的 6 PASS / 1 FAIL 原始记录：`final-fix-green-final.log`、`final-fix-correction.log`、`final-fix-correction-diagnose.log`；最后一次明确显示 Slider 轨道焦点导致合法就地纠正被忽略，属于产品失败而非删减断言。
- `final-fix-green.log`：7 PASS / 0 FAIL，exit 0，编译 33.66s、行为 0.03s。Clippy 两处测试初始化写法修正后的最新完整源码再跑 `final-fix-green-current.log`：7 PASS / 0 FAIL，exit 0、行为 0.03s。
- 不把编译错误计作行为 RED；没有零筛选 PASS，没有隐藏原始失败或运行科学全量。

## 已完成工程检查与成本

`cargo fmt --all -- --check` exit 0；`git diff --check` exit 0。

`cargo clippy --workspace --all-targets --all-features -- -D warnings` 最新 exit 0，21.20s：`final-fix-clippy.log`、`final-fix-clippy-receipt.json`。初次两个 field_reassign_with_default 错误保留在 `final-fix-clippy-first.log`，仅改测试初始化，没有放宽 lint。

相关原有回归顺序运行，26 次执行全部 PASS：AuthoringSession/token 9，project_flow 9，keyboard 6，same-pass settlement 1，旧有效地图保留 1。各 `final-fix-regression-*.log` 与 `final-fix-regressions-and-clippy.json` 保留真实命令/数量。keyboard 包含 3 项新增测试，因此不宣称 26 个独立旧测试。

最终 `cargo build --release --lib` exit 0，40.93s：`final-fix-production-build.log`。此命令与 libtest 的依赖特征合并不同，实际重编译了一部分 native 依赖，未称全热构建。最初的热估计遗漏此特征差异；发现时已经只剩本 crate，root 批准完成当前命令。`final-fix-current-build-receipt.json` 包含真实前后 free、UTC、环境和命令，新增峰值 500MB 是保守准入预算，不是实测峰值。原低空间 HOLD 无编译；恢复后准入 free 约 107GB，1GB 操作 reserve，未删除任何 cache 或停止未知进程。

root 批准一次 fresh wasm 冷依赖检查，在 helper EXIT `2026-10-01T00:53:31.6649202Z`、CaptureAppRunning=false 后运行：`cargo check --target wasm32-unknown-unknown --all-features --lib` exit 0，17.94s（真实包含冷依赖 metadata 检查）；见 `final-fix-wasm.log` / `final-fix-saved-and-wasm-receipt.json`。没有并行 UI/CPU 验证或采用旧 T6 PASS。

## 当前生产产物与接口

- `target/release/deps/libsekai-5c9dba06e85c9e1f.rlib`（root `target/release/libsekai.rlib` 相同内容），SHA256 `135FB169A9DF61EA216573DE9A0EBED52959088DF7C4DECBA54EB017A430A756`。
- 匹配 `target/release/deps/libeframe-34ef0bb63ee25fb3.rlib`。所有真实依赖路径/特征由一次热 `cargo build --release --lib --message-format=json` 查询得到，exit 0，0.26s；见 `final-fix-production-artifacts.jsonl` / `final-fix-production-library.json`。不硬编码旧 7716/旧 eframe 产物。
- `50e379c` 生产编译源码 `src/app.rs` SHA256 `CDF8562A579913634456D9834E4D271E7D55604D95C8CE8D8A3039E042669514`；`final-fix-source-manifest.json` 记录当前源码与编译输入哈希。manifest 的文件集合见其 Path，不把文件数当作运行验证。
- 新增接口均为 app 内部消费者：`WorldBuildPhase` 与 observed build 的可选当前阶段对象，`RejectedAuthorInput` 临时错误载荷；公开构建 API 和项目 schema 不变。
- 只改 `src/app.rs`、`src/app/project_ui.rs`、`src/app/spherical_presentation.rs`；575 insertions / 29 deletions（包括实际 UI fixtures 和格式化）。没有改求解器、质量 profile、FAS/CFL/cycles、冻结门禁、T1 候选或数组后处理。

## 自审关切与剩余门禁

1. root 已检查当前真实窗口阶段/键盘三张图与实际 Save 结果；全部为 `50e379c` 新产物，无旧 49e 图片。唯一独立 scoped review 已读最终测试/wasm PASS 和 `a369501`，最终 Spec PASS / Quality PASS，无新 C/I/M 0/0/0；最终文档 `final-fix-review.md` 已由 root 完整阅读。
2. fresh wasm 已 PASS；新增显式 Saved 前置后精确测试 1 PASS、fmt PASS、Clippy PASS 13.48s；没有重跑 7/26 或科学预算。
3. M1 未知 Tectonic schema 错误类别继续明确 deferred；严格拒绝仍在，此 wave 不扩展 codec/schema。
4. Draft 58.83s / Standard 91.25s 原预算仍 FAILED；本波不重跑或宣称性能达标。既有 ignored T1 `.0139 > .01` 红仍保留，科学策略没有变化。
5. 用户本人 UI 验收仍 PENDING。
6. 最终测试后继提交 `a36950134c6d87246e4c5e56bb1881aee8a0f52f` 仅两行显式 Saved 前置；与生产/图片来源 `50e379c` 分别标注。`final-fix-source-manifest-final.json` 记录后继完整 source SHA；不将后继全文件哈希称作原生产编译时哈希。

最终测试 checkpoint：`a36950134c6d87246e4c5e56bb1881aee8a0f52f`。`final-fix-saved-percentage.log` 1 PASS，`final-fix-saved-fmt.log` exit 0，`final-fix-saved-clippy.log` exit 0 / 13.48s，`final-fix-wasm.log` exit 0 / 17.94s。最后源码修改仅 cfg(test) 前置条件；生产 rlib SHA 与 50e 原始 manifest 保持不变，后继 manifest 分文件记录真实不同的 app.rs SHA。
## root 当前真实窗口回传证据（生产 50e）

root 已查看四张实际窗口图，其中下列三张作为交付图且可读；完整操作记录 `screenshots/final-fix/ui-report.md` 已由 root 阅读。三张交付图来源生产 `50e379c` / rlib SHA `135FB169...`，并保存至 Library（private receipts 在 screenshot folder）：

| 图 | root 提供路径 | UTC | Library file/version |
| --- | --- | --- | --- |
| 真实当前粗阶段 | `screenshots/final-fix/current-build-phase.png` | 2026-10-01 00:47:38.6587863Z | `libfile_225406c8a1b08191a0a1563d26f21611` / 0 |
| 40% 被接受 | `screenshots/final-fix/percentage-40-accepted.png` | 2026-10-01 00:49:29.1736860Z | `libfile_a87de74eb6fc8191968d167bca259bf8` / 0 |
| 板块 0 原文保留 | `screenshots/final-fix/invalid-plate-zero-retained.png` | 2026-10-01 00:51:52.2706494Z | `libfile_7960fc27cbb48191b097139845f1d8ed` / 0 |

root 回传实际 UI Save 文件确认：合法百分比得到 target 0.4 / TargetLandFraction / plate 12；非法板块 0 后 Save 原文件不变；Escape 后 Save 恢复 plate 12 / WaterInventory / 0.38。只有初始自动 build，参数编辑未启动额外生成。Native helper PID 4916 于 2026-10-01T00:53:31.6649202Z 退出，CaptureAppRunning=false；之后才补 Saved 测试并编译 wasm，避免 CPU/UI 并行。这些是代理窗口与文件行为证据，用户本人验收仍待进行。

最终测试后继 `a36950134c6d87246e4c5e56bb1881aee8a0f52f` 的 app.rs SHA `6C55483AC2DFCC440FCD85B9CFBD89C3C6520C2BC3DA1CAC8EE56B44CFDB3084`，manifest SHA `A8E8A02E6EE30D9FA0C78E344B99F8320E4D1BE39286D40033015FB19D3B5AB4`；这两行 cfg(test) 差异没有改变上述生产 API/产物或图片来源。

最终关闭状态：集中 fix wave DONE；生产修补提交 `50e379cfe2b918bdd8a50445d318a7a7f082d13d`，仅测试 Saved 前置后继 `a36950134c6d87246e4c5e56bb1881aee8a0f52f`。代码、实际 egui/worker、对应原有回归、fmt/Clippy/wasm、真实代理窗口与唯一独立 review 的本波证据均闭合。M1 deferred、原性能预算 FAILED、既有 T1 红与用户本人验收 PENDING 继续明确保留；不把这些未通过事项包装为已通过。root 负责将报告与原始记录归档到最终 docs 提交，未再修改生产代码或运行额外检查。