# P2 重采样边界噪声修复实施计划（2026-10-26）

上位：`AGENTS.md`；设计真相
`docs/superpowers/specs/2026-10-26-p2-resample-boundary-noise-design.md`（R1，方案 D）。

基线 `main` 51a9b36（规格 R1 提交 a100b76，代码相同）。一任务一提交，提交前跑 fmt /
clippy（`-D warnings`）/ wasm 门禁。测试范围：碰到重采样、身份与冻结物，跑 Release 的
构造、形成链、层级产品、GPU 金样与显示哈希套件，不跑调试档全量。

## 任务队列

- [ ] Task 1 —— 界面距离状态与开局初始化（设计 §5.1、§5.2、§5.5）：`CrustSample`
      增加私有强度量 `interface_distance_m: Option<f32>`（`None` 只用于采样洞标记）；
      保零集重初始化 `reinitialize_interface_distance`（零点：符号与种类一致的线性
      零交点，否则 Voronoi 边中点；距离：最近点传播）；`build_initial_state_v5`
      末尾按开局掩膜初始化；新生样本按 Voronoi 边规则取值。种类仍由覆盖赢家决定，
      产物逐位不变。纯函数测试：倾斜大圆界面的距离精度、开局掩膜符号复现。
- [ ] Task 2 —— 重采样种类改由 ψ 的符号决定（设计 §5.3、§5.4、§5.5）：同 owner、
      非标记候选的重心插值；回退最近候选，再回退覆盖赢家；本种类无物质可落的格
      预先退回赢家种类，余下无包格按孤立格同法向邻格借物质；重采样末尾重初始化。
      身份 `natural.causal-formation` 5 → 6，冻结哈希、显示哈希、层级探针指纹、GPU
      金样按实测重钉。新增 ignored 窄集成：刚体诊断（超额位移 < 同次运行二值下限、
      陆块数恒定、无新生小陆块）与 3 格窄水道夹具。门：账本闭合、确定性、V5 §11
      17 种子证据门、`g1d_endstate_crust` 关系门、V4 `natural.spherical-tectonics`
      哈希不变。
- [ ] Task 3 —— 实测与规格 R2：成对计时（A8 纪律，`.cargo-timing.lock`）；在修后代码
      上重跑 §1 探针（草稿 + 标准，种子 42 / 1950802277578115328 / 3）；Q2 面积收缩
      对照二值下限；Q4 孤立格触发率；Q5 低于厚度下限的破裂重测；结果写入规格修订。

## 每项承重技术的出处

见设计 §14。
