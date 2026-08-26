# G1 陆壳跟板块走计划（2026-08-26）

**Goal:** 开局陆壳改为核落在板块上的连通生长，可跨相邻板、板块陆洋混生，
直到 `continental_crust_fraction`。五种预设在形成链地壳类型 / 板块编号图
上可辨。

**Architecture:** 只改 `initial_crust_samples`（V3/V5 共用）。板块分区与
欧拉运动不动。核数是 `ResolvedWorldFormationPreset` 上的文献计数。形成链
字段已有，不接新键。缓存失效只抬 `natural.causal-formation` 与
`natural.spherical-tectonics` 的 stage version。

**Tech Stack:** 现有拓扑邻接、面积前缀、Draft 夹具、`build_primary_relief_for`。

**Spec:** `docs/superpowers/specs/2026-08-26-g1-continental-crust-on-plates-design.md`

## 任务

- [x] Task 1 —— 冻结规格 R1，写下本计划。核数方法进 `world` 层预设。
- [x] Task 2 —— `initial_crust_samples`：板块代表上最远点选核、图距离最近
      核、面积前缀；GreatIsland 主核域优先。删除 FBM 陆壳分位。
- [x] Task 3 —— 最小测试：纯函数邻接、`build_initial_state_v5` 五预设 ×
      种子 42/3、嵌套 2 种子；缩掉 17 粒掩膜矩阵；契约测试改为板块数会改
      地壳种类。
- [x] Task 4 —— ignored Release 探针（五预设 × 种子 42，撞 Airy 则改 3）。
      抬 stage version。fmt / clippy / wasm。用户按规格 §8 验收 UI。

## 非目标

不改 GDH1、Airy、浴缸、碎裂帽、P2 时域、洋龄 FBM、陆地占比滑块。不提交
G0 混装。

## 每项承重技术的出处

见规格 §10。
