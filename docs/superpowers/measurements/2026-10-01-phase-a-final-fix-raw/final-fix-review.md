# 最终集中修补 scoped 独立复审

审查者：用户指定 gpt-6.1-sol/high。遵守只读产品源、不派子代理、不运行 cargo/UI/科学全链。唯一输出为本报告；未修改产品、未提交。

BASE：`516c23c64eddb7f083caf4ab4a8d2eb6d6805bdf`。
最终复审 HEAD：`a36950134c6d87246e4c5e56bb1881aee8a0f52f`（完整范围为 BASE..a369；50e..a369 仅两行 cfg(test) 保存状态断言增量，已读取实际 git diff）。
生产修补 HEAD：`50e379cfe2b918bdd8a50445d318a7a7f082d13d`，branch `codex/phase-a-authoring`。
下述源定位采用 50e 包；a369 的两行插入使该 fixture 后续源码行号顺延两行。
完整读取 `final-fix-brief.md`、`final-fix-report.md`、56396 bytes 的 `final-fix-source-review-package.md`；核对 BASE..HEAD 三个源文件完整差异及必要直接消费者。未使用 HEAD~1 代替指定范围。R2 §4.2/§6/§7/§9.2 与 AGENTS 为审查依据；历史 design-only 状态不覆盖本次授权。

## 明确结论

**Spec verdict：PASS（本次 I1/I2/M2 限定工程范围）。**
**Quality verdict：PASS（该修补代码、目标回归及工程门禁证据）；用户最终验收与既有产品目标仍未全部完成。**

本次新增发现：Critical **0** / Important **0** / Minor **0**。未发现修补直接引入的重要破坏。
既有 M1 未知 Tectonic schema 错误分类仍是 **1 个已裁定延期的 Minor**：严格拒绝未改变，本次不重开 codec。若把既有延期项计入剩余发现，总量为 Critical 0 / Important 0 / Minor 1。

## I1 — Addressed

两个可编辑百分比控件位于 `src/app.rs:2636` 与 `src/app.rs:2684` 附近，共用 `format_authoring_percentage`（2923）和 `parse_authoring_percentage`（2927）。显示与解析均以百分比为单位，`40%` 与裸 `40` 定义为 0.40；有限性和范围检查采用实际 f32 领域值及领域 MIN/MAX 常量。显示保留非整百分比，确认原显示文字时返回已有 f32，避免百分比乘除导致配方位变化。领域范围没有改动。

`final_fix_percentage_keyboard_accepts_display_units_and_escape_discards`（3482）在实际 egui 控件验证两个字段的两种输入、Enter、Escape、revision/token 和单次 undo。`final_fix_percentage_untouched_enter_preserves_noninteger_recipe_bits`（3518）验证非整百分比 no-op 的输入、RecipeId、revision、token、历史及 pending 状态。控件仍使用既有 Slider/DragValue 数值路径；原 `finish_author_input` 的 pointer_down/松手事务路径保持，已有 drag 回归通过。

补充核对：50e 后唯一源增量是在上述 no-op fixture（cfg(test)）先执行 `mark_saved(capture_save)`，并断言 `!is_dirty()`；两行没有改变生产行为。`final-fix-saved-percentage.log` 实际 1 executed / 1 PASS，编译 34.74s、行为 0.00s；`final-fix-saved-and-wasm-receipt.json` 目标命令 exit 0。由此“已保存的非整百分比值”证据已补齐；原 RecipeId/revision/token/history/no-pending 断言保持。

## I2 — Addressed

`parse_authoring_plate_count`（`src/app.rs:2909`）先解析 u16，再引用 `src/world/natural/spec.rs` 的 MIN/MAX_PLATE_COUNT。0、999、2.5 在 egui clamp/round 前返回 None；百分比同样在 egui 控件赋值之前拒绝非有限/域外实际领域值。没有复制或放宽科学范围。

`RejectedAuthorInput`（638）、`authoring_number_stays_pending`（2966）与 `finish_author_input`（2872）保存拒绝原文、字段和实际数字编辑器 ID，显示错误并恢复对应焦点。已核对本机 egui 0.31.1 的 DragValue 与 Slider 直接消费者：文本编辑 response.changed 与 Slider union 的焦点/ID 语义支持合法纠正，未改动 Enter 不重新解析时也不能遗失非法 pending。Escape 清除待编辑和临时错误；Undo/Redo、项目清理和 Discard 同步清除此临时状态。

`final_fix_plate_keyboard_rejects_outside_domain_before_clamp_or_round`（3540）实际覆盖域外/非整数 Enter、Tab blur、原文可见、revision/token/history 不变、Save 阻挡、Escape 后重新聚焦合法提交。`final_fix_percentage_keyboard_rejects_domain_errors_without_committing`（3595）覆盖两个百分比字段；`final_fix_invalid_numbers_can_be_corrected_in_place`（3658）覆盖三个字段的非法 Tab、Save 后 Ctrl+A 合法就地纠正，只提交一次。`src/app/project_ui.rs:157` 的 Save 与 229 的 start_project_save（也承接 SaveAs）都拒绝 input_edit，未绕过保存门。

## M2 — Addressed

`WorldBuildPhase`（`src/app.rs:168`）只含 Surface/Graph/Presentation。每个 `PendingWorldBuild`（193、602）拥有独立 Arc<Mutex<phase>>，生产 worker 捕获该请求对象（1435、1515）。Formation 与 Legacy 的实际 graph 边界分别在 `src/app/spherical_presentation.rs:1699/1708` 和 1830/1835 更新 Graph/Presentation；Surface 包含真实准备操作。并非计时观测的副产物，观测关闭仍有效。

真实状态 UI（`src/app.rs:1618`）同时消费阶段、elapsed、取消状态与入口。取消仍只撤销授权/请求协作取消，不提前释放 world_build；原真实 worker 单槽与延后一 pass settlement 均未改变。旧 worker 只持有旧阶段对象，不能写新请求对象，也不能改变输入/候选身份。自定义 TemplateApp 序列化只保存既有 AppPersistenceWire，阶段与拒绝文本不入持久化。公开构建 API 未改变，可选 phase 参数只有 app 内部生产消费者。

`final_fix_pending_status_renders_actual_surface_phase_without_observation`（3626）检查实际渲染状态。`final_fix_actual_worker_phase_is_request_owned_and_never_persisted`（3733）使用最低成本既有 LegacyFoundation 162-cell 真 worker，检查观测关闭仍进入 Presentation、对应渲染、旧对象与新请求隔离、wire 不含阶段/拒绝状态。Formation 边界由直接生产调用者核对；没有为此重复昂贵 Formation 全链。未拆分 bundled P2–P5、未引入阶段百分比/ETA/通用框架。

## 证据及其真实边界

原始 `final-fix-red.log` 是 5 executed，1 PASS / 4 FAIL（行为失败，exit 101）；非整 no-op 原本已通过，未伪称 RED。中间失败和诊断记录保留。最新 `final-fix-green-current.log` 为 7 executed / 7 PASS / 0 FAIL，行为 0.03s；`final-fix-current-build-receipt.json` exit 0。不能把名称含 final 的较早 `final-fix-green-final-receipt.json` exit 101 误认为最新结果。

五份 `final-fix-regression-*.log` 实际为 Authoring 9、project_flow 9、keyboard 6、same-pass 1、旧图保留 1，共 26 次 PASS。keyboard 含 3 个新增目标测试，因此不是 26 个独立旧测试。所有相关原始日志已读取，无 0-selected PASS。

fmt exit 0 见执行 receipt（空 fmt.log 不是单独成功证据）；最终 Clippy 见 `final-fix-clippy.log` 21.20s 和 `final-fix-clippy-receipt.json` exit 0，初次 lint exit 101 保留。本 reviewer 在最终 BASE..a369 运行轻量只读 git diff --check（exit 0）与身份核对，未重复测试。fresh native lib build 40.93s exit 0 含实际 native 特征差异依赖编译，不能称完全热构建。

已保存 fixture 补充后的 fmt 再查 exit 0、Clippy 13.48s exit 0，原始 `final-fix-saved-clippy.log` 与 `final-fix-saved-and-wasm-receipt.json` 已读取。补充只影响测试初始化，七项目标/26 次回归无需重复全跑。

独立读取文件哈希确认补充前 50e 生产 `src/app.rs` SHA256：
`CDF8562A579913634456D9834E4D271E7D55604D95C8CE8D8A3039E042669514`；
`target/release/deps/libsekai-5c9dba06e85c9e1f.rlib` SHA256：
`135FB169A9DF61EA216573DE9A0EBED52959088DF7C4DECBA54EB017A430A756`。
与 50e report/manifest 相符；两行 test-only 后继使完整 app.rs 文本哈希改变，不能把此旧哈希标为 a369 整文件哈希，但不会改变非 cfg(test) 的 native 生产行为。原 report 中 saved-state/wasm PENDING 状态已由上述后继原始凭据取代。仅三个源文件有修补差异，没有求解器、CFL/FAS/cycles、profile、科学阈值、T1 候选或数组后处理变化。

## 剩余阻碍与交付界限

- 本轮 fresh wasm gate **PASS**：`final-fix-wasm.log` 实际检查至本 crate 并 Finished 17.94s，`final-fix-saved-and-wasm-receipt.json` 的 `cargo check --target wasm32-unknown-unknown --all-features --lib` exit 0。这是本轮源码及两行 test-only 增量后的新证据，包含获准冷依赖检查；旧 T6 日志未用作本轮通过依据。
- root 已报告完成并阅读三张匹配 50e 生产产物的真实原生窗口截图，并核对 40% 保存为 0.4、板块 12、非法 0 保留/错误反馈/Save 不改文件/Escape 恢复 12；helper 于 00:53:31.6649202Z 退出，早于补充测试与 wasm。此为 root 的限定 UI 证据，本 reviewer 没有重开窗口或重复 UI；不替代用户本人验收或证明 OS present。
- 用户本人 UI 验收仍 PENDING。建议亲自检查两个百分比控件 40%/40、非整值无改动确认，以及板块 0/999/2.5 的 Enter/失焦、保存阻挡、纠正/Escape；生成时核对阶段/取消及旧地图保留。
- 原预算 Draft 58.83s / Standard 91.25s 仍 FAILED；既有 ignored T1 .0139>.01 红仍保留；本波没有运行或宣称修复性能/地理门禁。
- 上述待验收/已知红门不否定获批准的本次工程修补，但禁止据此宣称整体产品达标。
