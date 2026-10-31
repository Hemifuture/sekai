# 可采纳产品能力整合记录

目标分支 `integration/sekai-current`，不更新 main/远端，不清理工作区。
总计划由协调者维护，本记录仅描述整合者实际执行结果；用户 UI 验收不由测试替代。

## 任务与顺序

1. 合入已验证海岸 `5d8beb5498f4640d9f5ddbfb75dbe4e4d516390d`。
2. 合入预热 `260b5eff83f33c1e8febe90356abe005bfeae4a3`（含校验/A9前段），补 A9 尾文档。
3. 合入 Phase A `01120487c665ec03b2cca191ce16c4869a4e3fb8`，保留单 worker、取消、预热及海岸预检。
4. 合入 site `485efef5ba6846d772cd557a0ccd260a139a4c0e`，保留历史青岚身份，新世界另导出。
5. 在协调者授予计算窗后验证冲突契约及共享身份，更新 README/PROGRESS 与用户 UI 步骤。

## 海岸合并：构建输入等价核查

基线 `59c984ad0eb195eef118325029b5dec220afaae8`，使用
`git merge --no-commit --no-ff 5d8beb5498f4640d9f5ddbfb75dbe4e4d516390d`，无冲突。
合并索引相对源 SHA 的全部差异仅为五份已有设计/计划文档和 `pelican-bicycle.svg`；
新增本记录也只属文档。`src`、`tests`、Cargo 清单/锁、assets、Web/Trunk 及其他构建
输入逐 blob 完全一致。核查 `include_str!`/`include_bytes!` 只引用源代码与 assets，
未引用差异文档/SVG。因此不重复编译，复用精确源 SHA 的证据，不宣传新的全量通过。
AGENTS 与源 SHA 相同，保留当前 10-14/10-30 授权、出处纪律与最小测试范围。
协调者未暂存总计划不纳入本提交。

源证据：`2026-10-30-procedural-coast-ui-preview.md` 的 2026-10-31 续修记录：

- 首次 Release app 109 项通过；最终新增断言所在定向测试 1 项通过。
- 第二次不必要的并行 GPU app 全套复跑触发 600 秒上限；未取堆栈，不能断言根因。
  后续测试要求单线程；被 `&&` 阻断的其他套件未运行，绝不记作完整 Release 全量通过。
- fmt、Clippy、WASM check、Trunk Release、native Release 均由源提交完成。
- 可运行源 exe：`C:/Users/DYZ/orca/workspaces/sekai/coast-climate-spike/target/release/sekai.exe`；
  源记录 SHA-256 `E095D76132571D6DC72B1F4448E1CF19DB1BC8851E5C1CD783D193C209F2BB57`。
  本整合者尚未重新构建主目录 exe，不把旧二进制称为后续整合版本。

海岸 UI 验证：完整 Formation 链选择草稿与程序化海岸实验；切标准/高后重建禁用且
显示支持域提示，切回草稿恢复；启动恢复非支持组合立即拒绝而不启动昂贵生成。
湖 R3 遗留和气候科学响应不在该入口修补的完成声明内。

## 每项承重技术的出处

- 初态、海岸与水岸：已冻结海岸规格、LOD-1 R3 与源 UI 计划及其引用；不重复移植 LOD-1。
- 校验/预热与共同工作域：A9 resolution-bounded optimization 规格，合法产物的信任边界与
  工作域身份沿用对应修订；改变工作域不冒称与旧世界逐位一致。
- 作者事务与兼容：Phase A 冻结规格及 A1 修订；旧配方必须显式升级副本。
- 站点月表：site climatology 冻结规格；缺能力显示 unavailable，不借用 A10 preview 占位。
- Git 输入等价：Git 三方合并、完整树 diff 与 blob 身份；仅在所有构建输入一致时复用源证据。
