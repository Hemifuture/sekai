# 陆地占比驱动与水线求解（T0b）实施计划

规格：`docs/superpowers/specs/2026-08-22-land-fraction-driver-design.md`
（草案，Task 2 冻结）。

## 背景

T0 校准后形成链的海面由地球水量解出，陆地占比成为结果（0.20），
用户的"目标陆地面积比例"滑块在形成链上被禁用。用户要求陆地占比
成为可调的世界选项。物理上地球水量给陆地占比设了上限（规格 §2.1），
所以主旋钮必须驱动水线；陆壳比例改由预设携带几何，二者正交、UI 互斥
驱动。纪律：AGENTS.md（极简、SSOT、测试复用生产助手、一任务一提交、
接入 UI 才算交付）；P5 套件用 `--release` 迭代，最终跑完整调试回归。

## 任务

- [ ] Task 1 —— 实测（不改产品代码）：扩展 `tests/terrain_audit_probe.rs`
      （a）v5 清单均值/离散度逐步轨迹与账本归因（`SEKAI_V5_TRACE` +
      `SphericalTectonicMaterialProcesses`：裂谷增面积、碰撞缩短、
      消耗）——回答 38.7 → 35.4 km 的 −3.3 km 与厚尾不足各由谁造成；
      （b）五个预设 × 17 粒种子在地球水量下的陆地中位、露出率、
      (1 − L)·D 表与"地球水量下陆地上限"；（c）各预设达到标称陆地所需
      的隐含水量比。数字写入规格 §2 与 §8 决策规则。一提交。
- [ ] Task 2 —— 规格冻结：按 Task 1 回答 §8（默认驱动、提示带、预设
      陆壳是否重定、露出洋壳、标称陆地来源），钉预设
      `recommended_land_fraction` 实测值；**停一次交用户确认**后冻结。
- [ ] Task 3 —— P3 水线求解：`ReliefSpec::sea_level_policy`（schema
      升版、校验、序列化）；目标解复用 `select_area_weighted_sea_level` +
      `water_volume_at_sea_level_m3` 隐含水量（互逆测试）；P3 `generate` 分派，快照 wire 不变；
      P3 报告新增无界测量 `water-inventory-ratio`；默认模式逐位不变
      的守门测试（P3 证据哈希 / P5 seed 42 工件哈希）。验证：单元 +
      `primary_relief_*` / `surface_formation_stage`（release）。
- [ ] Task 4 —— 接入 UI：驱动单选（陆壳比例 / 陆地占比）、陆地占比
      滑块在两条链上启用、互斥锁定并显示推算/实测值、高级组里的陆壳
      滑块、摘要行"海水量 = r × 地球"与提示（带外、露出洋底）、
      `FormationAreaSummary::water_inventory_ratio`；应用测试（持久化
      往返、锁定语义）。验证：用户在 UI 上按规格 §7 走一遍。
- [ ] Task 5 —— 预设标称陆地重钉 + 指纹/证据：按冻结值改
      `recommended_land_fraction`；实测哪些冻结值因 `ReliefSpec` wire
      改变而变（原则：输出哈希不变者不动）；目标模式证据
      （Continents / seed 42 / 0.38）入 P3、P5 完成记录修订条目；
      规格 §3.6 刷新清单落账。
- [ ] Task 6 —— 门禁与验收：fmt / clippy -D warnings / wasm；全量套件
      两档 `--no-fail-fast` 分离进程；计划核对；用户验收步骤；交付报告
      artifact。最终验收归用户。

## 非目标

- 不动预设陆壳值与 v5 运动学（§8.3 决策规则例外须显式修订）。
- 不修陆壳清单离散度（T0 开放项），只归因。
- 不动 P5 方程、T1/T1v2、色带、河流；不改 legacy 链语义。
- 不做直方图重映射。

## 每项承重技术的出处

- **陆地占比 = 测高积分在水量处的取值**——Cowan & Abbot, ApJ 2014。
- **地球洋盆体积/面积/均深**——Eakins & Sharman 2010（ETOPO1）；
  `EARTH_OCEAN_VOLUME_M3` 既有常量。
- **行星表层水量可变**——Hirschmann 2006；Cowan & Abbot 2014。
- **干舷与陆壳厚度分布**——Wise 1974；CRUST1.0（Laske et al. 2013）。
- **浴缸海面解的稳定排序与离散容差**——P3 设计（既有
  `solve_physical_sea_level` / `land_fraction_constraint_tolerance`）。
