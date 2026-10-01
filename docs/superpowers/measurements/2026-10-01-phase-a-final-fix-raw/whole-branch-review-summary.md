# 已收到的独立审查结论归档（root整理，非新的审查）

本文件归档既有 agent 消息和 `progress.md` 中已收到的结论；不冒充本次新审查或 root 自审。原详细发现及定位完整保存在 `final-fix-brief.md`。2026-09-30 满盘时详细报告仅以消息交付，现整理为恢复依据。

## T8 范围审查

独立 reviewer `/root/t8_review`，用户指定 gpt-6.1-sol/high。受审源码 checkpoint `49e20840db736a233876e1028dedf2cc5b0c70b6`，源 diff package `task-8-source-review-package.md`。

代码及核心规格无 Important；Critical0/Important0/Minor1。该 Minor 是 task report 与 measurement 两处仍写预算/UI未完成的过期文案，随后 doc-only `516c23c64eddb7f083caf4ab4a8d2eb6d6805bdf` 已修正。当时新增 all-target Clippy未运行，因此任务证据完成保留；后来真实26.54s exit0覆盖新增fixture，原始日志在耐久 `t8/` 证据目录。已核对399条源码/11张PNG身份，没有失配，并查看3D/Open/Cancel图；用户本人验收仍未完成。

## 最后全分支审查

独立 reviewer `/root/whole_branch_review`，用户指定 gpt-6.1-sol/high；受审产品源码 `49e2084`（后继516c23c没有src/tests/Cargo差异）。完整源证据为 `whole-branch-source-package-v2.md`；原 `whole-branch-source-package.md` 只有180bytes、不完整，不作为依据。

结论：**Spec FAIL / Quality CHANGES REQUIRED**；Critical0、Important2、Minor2。

- I1 Important：百分比显示单位与共用裸f64 parser不往返，`40%`拒绝、`40`夹为0.75而错误提交。需统一单位，并覆盖非整百分比无改动确认保精度。
- I2 Important：板块输入仅parse+finite检查，控件会先将域外数夹值、非整数取整，错误推进作者历史并使构建身份失效。无效原文应保留，禁止提交；纠正和Escape需实际控件证据。
- M1 Minor，既有T3延期：未知Tectonic schema被分类为Input而非UnsupportedSchema，仍严格拒绝、未覆盖/静默升级，认定非阻断。
- M2 Minor：生成UI只有“生成/取消”和elapsed，没有实际粗阶段；R2§6要求未有免除裁定，应显示真实Surface/Graph/Presentation边界，不能伪造bundledP2–P5独立调度。

其余所审作者SSOT、一个真实worker槽、原子发布、存档与版本/升级副本安全、真实图边界未发现新增Important。性能红门、地理T1红门、用户本人UI验收均没有被审查或截图改称通过。

root已裁定一次集中修复I1/I2/M2，M1继续明确延期；修补完仅进行一次独立 scoped复审，覆盖这三项及修补直接引入的破坏，不重复 broad审查。现有5实际egui回归草稿的第一次完整执行为1PASS/4FAIL，exit101；最终结果待 `final-fix-report.md` 和 scoped报告，不在本归档预先宣称修复。
