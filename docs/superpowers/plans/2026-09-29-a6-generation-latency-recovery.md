# 生成时延里程碑 A6 实施计划（2026-09-29）

上位：`AGENTS.md`；设计真相
`docs/superpowers/specs/2026-09-29-a6-generation-latency-recovery-design.md`。

目标：Draft 全链 10–20 s，Standard ≤ 60 s（用户 2026-09-02，A1）。基线 `883cc22`
阶段探针 48.2 s（Windows）。一任务一提交，提交前跑 fmt / clippy / wasm 门禁。

## 任务队列

- [x] Task 0 —— 剖面与基线：阶段探针 + WSL `samply` 剖面，设计 §1。
- [x] Task 1 —— 取消观测：剖面归因核实为采样堆积，不单列，并入 Task 2 的按块
      轮询（设计 §3）。
- [x] Task 2 —— 快张量确定性并行：实现并逐位核对后**按实测撤回**（设计 §9 R1：
      3456 格网格上 1–32 线程均在噪声内，线程唤醒吞掉收益）。
- [x] Task 3 —— 瞬态梯度一律普通投影（设计 §5）：阶段探针 48.0 → 43.5 s；循环与
      残差 4 位不变；最大发布场差 0.0010 %；随机 seed 冷启动扫描 Draft 24/24、
      Standard 8/8 通过（Draft 种子 `random.seed(20260929)` 取 24 个 64 位数）；
      P4 lib 与 8 个环流集成二进制 Release 通过。
- [ ] Task 4 —— 收尾：产品级时延（Draft / Standard，本机多线程与单线程），Release
      全量回归，推送后 CI 全绿；若仍超目标，把设计 §8.1 交裁定。

## 每项承重技术的出处

见设计 §10。
