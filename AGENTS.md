# Sekai Agent Instructions

## 验收纪律：算法必须与 UI 同步交付（用户指令，2026-08-19）

- 算法与生成管线的验收一定要与 UI 同步：任何一条生成链路、任何算法改动或质量修复，
  只有当它在应用界面上能被看到、被操作时才算交付。只有后台实现的算法是没有用的。
- 最终验收由用户本人在 UI 上完成。代理的自行验证（单元/集成测试、探针、离线渲染、
  无障碍驱动截图等）只能解决一部分问题，必要但永远不充分，不能替代用户上手验证。
- 因此每个算法类任务的计划必须包含"接入 UI"的任务项；未接入 UI 之前不得把算法任务
  标记为完成或宣称"已交付"。
- 每次交付都要附上用户验证步骤：如何启动、在哪个面板/视图看、预期看到什么。

English mirror for tooling: algorithm work is accepted only when it is wired
into the UI. Backend-only implementations count as undelivered. The user
personally performs final acceptance in the running app; agent self-checks
(tests, probes, offline renders, automated UI drives) are necessary but never
sufficient. Every algorithm plan must contain an explicit "reach the UI" task,
and every delivery must end with user-facing verification steps (how to launch,
where to look, what to expect).
