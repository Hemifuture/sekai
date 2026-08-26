# G0 自然地理基线实测计划（2026-08-26）

**Goal:** 用生产 P2+P3 量化陆壳不跟板块走、洋龄无脊轴结构的现状，并把洋龄接到形成链 UI，供 G1/G2 钉数。结论已写入规格 §4.5。

**Architecture:** 独立 ignored Release 探针消费已有 Draft 球面夹具与
`build_primary_relief_for`；测量函数只读权威快照和生产质量/测高助手。形成链
`spherical_formation_field_registry` 增员 `ocean_age_myr` 并绑定 P2
`crust_age_myr`，生成器零改动。

**Tech Stack:** 现有 Rust 测试夹具、`hypsometric_*`、`evaluate_primary_relief_quality`、
物质/谱系账本访问器、egui 字段目录。

**Spec:** `docs/superpowers/specs/2026-08-26-g0-geography-baseline-design.md`

## 任务

- [x] Task 1 —— 冻结测量协议：本规格 §1–§3、§5–§7。不写合格带。
- [x] Task 2 —— 形成链接入 `ocean_age_myr`：注册表、payload、中文标签、
      Data 值域、冻结哈希。验证：字段出现在形成链列表，切换后球体着色。
- [x] Task 3 —— ignored Release 探针 `tests/geography_baseline_probe.rs`：
      Draft、默认半径、Continents/Supercontinent/Archipelago × 种子 42 与 3，
      停在 P2+P3。辅助函数带人造邻接单元测试。把 stdout 数字写入规格 §4。
- [ ] Task 4 —— 门禁：fmt / clippy -D warnings / wasm 已跑；用户按规格 §5
      看三种预设的地壳类型、板块、洋龄、高程（最终验收归用户）。

## 非目标

不改生成器；不钉 G1/G2 带；不跑 17 粒；不进 P4/P5。

## 每项承重技术的出处

见规格 §7。
