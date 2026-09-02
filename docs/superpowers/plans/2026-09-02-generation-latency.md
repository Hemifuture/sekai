# 生成时延里程碑 A1 实施计划（2026-09-02）

上位：`AGENTS.md`；设计真相
`docs/superpowers/specs/2026-09-02-generation-latency-design.md`。

目标：Draft 全链 10–20 s，Standard ≤ 60 s（用户 2026-09-02）。基线 44.0 s /
77.3 s，P4 的两次求解占 88 %。一任务一提交，提交前跑 fmt / clippy / wasm 门禁。

## 任务队列

- [x] Task 0 —— 分阶段计时探针
      `causal.rs::timing_probe::stage_timing_probe`（ignored / Release，
      `SEKAI_PROBE_PROFILE` 选档）逐阶段打印 P1–P5 与两次 P4 的耗时和求解报告，
      `SEKAI_PROBE_TAG` 可把两次气候快照落盘供逐场对比。它产出了设计 §1 的基线
      与 §3.3 的扫描表。

- [x] Task 1 —— 快子步 Courant 目标 0.2 → 0.5，删除 1200 s 上限
      三处重复常量收敛到 `src/world/natural/global_circulation.rs`；世界层契约的
      「每宏步 ≥ 6 快子步」字面量改为从自转限推导的 3。设计 §3。
      实测 Draft 全链 44.0 → 26.0 s；场差异 ≤ 0.37 %。
      验证：P4 全部 14 个测试二进制 Release 全绿。

- [x] Task 2 —— 终点 P4 从起点末态热启动
      `generate_continuing` 返回末轮工作网格状态；P5 闭合把它交给终点求解。设计
      §4。实测 Draft：终点 9 轮 / 503 快子步 → 2 轮 / 120 快子步，全链
      26.0 → **16.5 s**。
      **发现**：无论冷热，终点气候与起点气候相差 25–135 %（温跃层高度、海面高、
      海流、降水）——P4 的一轮是 12 × 7200 s = 1 个模式日，6–9 轮是一段瞬态而
      非平衡；降水不在残差度量内且对湿度极敏感。热启动使终点延续起点轨迹，差异
      略小（降水 26 % vs 31 %）。记入设计 §6 开放问题 4，不在本里程碑解决。
      验证：P4 套件 + 17 seed 气候证据 + 因果链证据（见 Task 5）。

- [ ] Task 3 —— 编译配置
      实测 `opt-level = 3` / `lto` / `codegen-units = 1` 对 Draft 全链的影响，
      按实测决定；不改浮点语义。

- [ ] Task 4 —— 快子步簿记清理（逐位一致）
      仅当 Task 1–3 后仍需要时做：每次快求值分配整套张量、RK 阶段合成对常量标量
      场重算并逐格线性查找层。实测两次求解合计 2.3 s / 44 s，收益有限。

- [ ] Task 5 —— 产品级时延门与证据刷新
      新增 `tests/generation_latency.rs`（ignored / Release）：走生产 stage graph
      （含质量评估与束校验）计时，Draft ≤ 20 s、Standard ≤ 60 s 直接作断言。
      重跑 `p4/performance.json`、`p5/performance.json`、17 seed 气候证据、全量
      Release 回归；更新 README 的性能预算陈述。

## 用户验证步骤

`cargo run --release`，左侧面板按「按当前参数重建」，看状态栏耗时：

1. Draft 档应在 20 s 内出图（探针实测 16.5 s，应用侧另有质量评估与显示构建的
   开销，见 Task 5 的实测）。
2. Standard 档应在 60 s 内。
3. 字段目录选**年降水**、**近地面风**、**海表温度**：与改动前同 seed 的图相比，
   大尺度形态应一致（起点 P4 的场差异 ≤ 0.37 %）；终点场的差异见 Task 2 的
   发现，属既有 P4 瞬态性质。
