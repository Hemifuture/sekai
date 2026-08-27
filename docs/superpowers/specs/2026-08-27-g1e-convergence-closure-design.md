# G1e — 汇聚的物理闭合与张开相开局（2026-08-27）

状态：**执行中**（用户指令 2026-08-27："把不合理的门禁全部删掉，按修法开修"）。
实现期偏离以修订条目记录。

上位：`AGENTS.md`；短程路线
`2026-08-26-natural-geography-short-horizon-roadmap.md`；G1
`2026-08-26-g1-continental-crust-on-plates-design.md`；G1d
`2026-08-27-tectonic-driving-forces-design.md`。本规格**修订** G1d §3.3 与 R3、
G1 §5.1（仅 Continents / Archipelago 的选核域）。Cortial 2019 步进、GDH1、Airy、
浴缸恒等式、CRUST1.0 台地表、P2 时域全部不动。

## 1. 病征实测（HEAD 73665a3，草稿档，推荐陆壳分数）

地壳类型层（`tests/g1d_endstate_crust.rs::probe_archipelago_endstate_mesh_anatomy`）：

| Archipelago | 陆壳块数 | 最大块份额 | 最大块跨板数 | 封闭洋壳洞 |
| --- | ---: | ---: | ---: | ---: |
| 42 / 12 板 | 10 | 0.338 | 5 | 0 |
| 42 / 22 板 | 7 | 0.313 | 6 | 0 |
| 3 / 12 板 | 9 | 0.207 | 5 | 0 |
| 3 / 22 板 | 7 | 0.603 | 11 | 0 |

开局 12–14 块、每块一板；终态最大块由 5–11 块开局岛缝合而成。预设规格 §9.3
的群岛验收（≥ 8 块、最大 ≤ 30%）四例全不达标。

P3 之后与主洋不连通的湿区（临时探针，已删）：19–37 个，**100% 为陆壳**，厚度
p10/p50/p90 = 21/27/30 km，低于台地表最小 28 km；露出陆地为 33/38/43 km。内湖是
裂谷减薄、未完成洋化的陆壳带被淹，不是洋壳洞。

同一次运行 104 次重采样的 `moved_area_share`（`SEKAI_V5_TRACE`）中位 5.3%、最大
44%。

## 2. 根因

1. **群岛"核 = 板"使全球没有面积汇。** `K = min(14, P)`；12 板时每板一核，R3 给
   每块板打张开相标签，`involves_opening_phase` 对任意板对为真，`classify_pair`
   永不产出 `OceanicSubduction`：256 Myr 零俯冲、零板片拉力。封闭球面上扩张
   必须有汇。
2. **被拒的汇聚变成"无事件"，由重采样静默吞掉水道。** 张开相板对的汇聚返回
   `None`：力矩里既无拉力也无阻力，板照样重叠，`deposit_continental_volumes`
   把重叠柱体搬到最近未填格元。这是没有海沟的俯冲，属 `AGENTS.md` 禁止的无
   成因位移。
3. **Stern 被动缘规则被套在板间边界上。** Stern (2004) 的"完整被动缘不自发俯冲"
   说的是**板内**被动缘不会自发变成板块边界。Sekai 的洋–陆接触若是两块板之间
   的边界且在汇聚，那就是 Stern 的**诱导（受迫）成核**情形（"induced nucleation
   ... where plate convergence is forced"），地球对应安第斯型边缘。把它当被动缘
   禁掉，是把板内规则用到板间。
4. **最远点选核把系统放在威尔逊旋回的"最大分散"端。** 从最大分散出发跑 256
   Myr 只能走拼合半程（Wilson 1966）。Continents 42/12 → 0.963 一块、群岛缝成
   带洞大块，都是这一半程的正常产物。地球今日的多大陆是 Pangea 裂解后约 180
   Myr 的**分散半程**快照，起点是一个半球的拼合大陆与另一个半球的
   Panthalassa（Wegener 1915；Seton et al. 2012）。
5. **力系数全是未钉的排序占位**（`PLATE_*` 常量自述）；G1d 任务 4 的"从生产板速
   重钉"未做。

## 3. 机制

### 3.1 删除张开相标签（废止 G1d R3）

删除 `SubductionInitiation::opening_phase_lineages` 及 `mark_opening_phase` /
`is_opening_phase` / `involves_opening_phase` / `mark_opening_phase_lineages`
与相应测试。预设不得再按名字禁止某类板参与某个过程。

### 3.2 板间汇聚的闭合分类（修订 G1d §3.3）

`classify_pair` 对**不同板**的汇聚只按材料与浮力分类，不再读"谁在碰撞""是否
本轮裂谷对"：

| 接触 | 分类 | 出处 |
| --- | --- | --- |
| 陆–陆 | `ContinentalCollision`（阻力有限，继续缓慢汇聚并增厚） | England & McKenzie 1982；Molnar & Stock 2009 |
| 洋–陆，洋侧年龄 ≥ `CLOOS_OCEANIC_NEGATIVE_BUOYANCY_AGE_MYR` | `OceanicSubduction{descending: 洋侧}` | Stern 2004 诱导成核于受迫汇聚边界；Cloos 1993 负浮力 |
| 洋–洋，较老侧年龄 ≥ Cloos | `OceanicSubduction{descending: 较老侧}`（不变） | Cloos 1993 |
| 汇聚但下插侧年龄 < Cloos | **`LockedConvergence`**（新增） | Cloos 1993：年轻洋壳正浮力，不能下插 |

已建立海沟继续消耗（不变）。`colliding_continents` 与诱导判定删除；
`continental_rift_pairs` 只保留给 `OceanizationPolicy`。

板内被动缘（裂谷两侧与新洋同属一块子板）在 Sekai 里从不产生板块边界，因此
"被动缘不自发俯冲"由结构本身满足，不需要门闩。

### 3.3 `LockedConvergence` 进入力矩平衡

`LockedConvergence` 与 `ContinentalCollision` 一样以阻尼项进入 \(C\)（对方速度
滞后一步，与现有算子分裂一致），系数 `PLATE_LOCKED_MARGIN_RESISTANCE_PER_M`。
它不触发任何消耗/增厚过程，也不进入现今强迫场。

钉值程序（先测后钉）：在语料上扫 κ，取使锁定边界的残余法向汇聚中位数低于
`MINIMUM_ACTIVE_RELATIVE_SPEED_MM_PER_YEAR` 的最小量级。碰撞阻力
`PLATE_COLLISION_RESISTANCE_PER_M` 同法复测：地球锚是印度–欧亚碰撞后汇聚率由
约 150 降到约 40–50 mm/yr（Molnar & Stock 2009；Copley et al. 2010），即碰撞
**减速而不锁死**。

### 3.4 重采样重叠沉积入账

`EvolutionMaterialLedger` 记录每次重采样的重叠搬运面积（`deposit_continental_volumes`
的 `moved_area`）累计值；探针报告"搬运面积 / 陆壳面积"。本规格不钉门禁：先在
3.2–3.3 落地后量出稳定量级，再由后续修订钉带。

### 3.5 张开相开局：半球帽选核（修订 G1 §5.1，仅 Continents / Archipelago）

Continents 与 Archipelago 的发布相位是**分散半程**，因此开局必须是拼合态：

- 选一个稳定的帽中心（`INITIAL_CRUST_V3_LABEL` 计数器选格元，与现有第一核
  规则同源）。帽 = 以该中心为极的**半球**（Wegener 1915：Pangea 占一个半球，
  Panthalassa 占另一个；Seton et al. 2012 全球重建）。
- 核在帽内格元上做最远点采样，\(K\) 仍按 G1 §6（Continents 6，Archipelago 14），
  **不再要求一核一板**：核可同板（Cogley 1984：格陵兰在北美板上、马达加斯加在
  非洲板上、新几内亚在澳大利亚板上）。同板碎块之间的板内洋不会被任何过程关掉，
  这是地球群岛"许多岛在少数板上"的机制。
- 生长与面积命中不变（图距离前缀）。帽外板块全为洋壳，成为俯冲汇（Panthalassa）。
- Supercontinent / GreatIsland / VolcanicIslands 选核不变。

开局连通块数不再是 Continents 的合同（拼合态本就相连）；G1d §5"开局 Continents
= 6 块"废止，改为"开局 6 核"。

### 3.6 力系数钉值

任务 4 补做：用生产算子量草稿语料的板速分布（板代表格元处 \(|v|\)），分"有
海沟下插的板"与"无下插的板"两组，对照 Forsyth & Uyeda (1975) 的"有板片则快、
有陆则慢"与 DeMets et al. (2010) MORVEL 量级（板速 10–100 mm/yr）。量级落在带内
则以实测记入常量注释；否则整体缩放 \(C\) 或 \(\boldsymbol{\tau}\)，不改比例排序。

## 4. 不做

不改 P2 时域、步长、碎裂帽；不改 `OceanizationPolicy` 与 `dominant` 标签
（开放问题 §7）；不加岸线噪声；不重涂终态；不为凑块数改填色；不加三维对流；
不新增发布 schema。

## 5. 测试（最小充分证据）

| 层 | 覆盖 | 为何更小层不足 |
| --- | --- | --- |
| 纯函数 `classify_pair` | 洋–陆板间汇聚且洋侧 ≥ Cloos = 俯冲；< Cloos = 锁定；洋–洋不变；同板 None | 无需球面 |
| 纯函数 torques | 锁定边界给出与碰撞同型的阻尼；κ 越大残余汇聚越小 | 无需球面 |
| 开局 `build_initial_state_v5` | Continents / Archipelago 核全在帽内；Archipelago 存在同板双核；帽外板全洋；面积命中与嵌套不变 | 纯函数没有真实分区 |
| 终态窄集成（草稿、42 与 3、12 与 22 板） | Supercontinent 一块；Continents 块数 > 1 且最大份额 < Supercontinent；Archipelago 块数 > Continents 且最大份额 ≤ 预设规格 §9.3 的 0.30 上界 | 开局契约看不到 256 Myr 后 |
| ignored Release 探针 | 板速分布、锁定/碰撞边界残余汇聚、重叠搬运份额、内湖数 | 钉系数的证据 |

## 6. UI 与用户验证

草稿档、同一种子（42），形成链，依次切五种预设，看"地壳类型"：多大陆为数块被
洋盆分开的大陆；群岛为许多被洋盆分开的小块，不得并成带洞大块；超大陆一块主导。
再看高程图：内湖应明显少于修前（修前 19–37 个）。陆地占比滑块仍只动海平面。

## 7. 开放问题（交用户裁定）

1. `OceanizationPolicy::SuppressContinentalBreakup` / `ExceptDominant` 与
   `dominant` 标签仍是按预设禁止过程的门。Nance & Murphy (2013) 的稳定相有出处，
   但更干净的做法是用开局与裂谷率表达。本规格保留，待裁定。
2. `MINIMUM_ACTIVE_RELATIVE_SPEED_MM_PER_YEAR = 8`、`STRONG_NORMAL_FRACTION = 0.4`
   无出处注释，本规格未动。

## 8. 每项承重技术的出处

| 技术 | 出处 | 用法 |
| --- | --- | --- |
| 洋盆张开–闭合；分散 vs 拼合半程 | Wilson (1966), *Nature* 211 | §2.4、§3.5 |
| Pangea / Panthalassa 半球对置 | Wegener (1915/1966); Seton et al. (2012), *Earth-Sci. Rev.* 113 | §3.5 帽 = 半球 |
| 多块大陆共板；微陆块 | Cogley (1984), *Rev. Geophys.* 22 | §3.5 同板多核 |
| 受迫汇聚边界的诱导成核 | Stern (2004), *Earth-Sci. Rev.* 66 | §3.2 板间洋–陆汇聚 = 俯冲 |
| 洋壳负浮力年龄 | Cloos (1993), *GSA Bull.* 105 | §3.2 Cloos 门槛与锁定 |
| 板内被动缘强度 | McKenzie (1977), *GJI* 48 | §3.2 结构性满足 |
| 碰撞减速不锁死 | Molnar & Stock (2009), *Tectonics* 28; Copley et al. (2010), *Tectonics* 29 | §3.3 碰撞阻力锚 |
| 板片拉力主导、有陆则慢 | Forsyth & Uyeda (1975), *GJI* 43 | §3.6 |
| 拉力 vs 脊推量级 | Conrad & Lithgow-Bertelloni (2002), *Science* 298 | 排序不变 |
| 板速量级 | DeMets et al. (2010), *GJI* 181 (MORVEL) | §3.6 带 |
| 碰撞缩短增厚 | England & McKenzie (1982), *GJI* 70 | 不变 |
| 2 Myr 步进与过程 | Cortial et al. (2019), CGF 38(2) | 不变 |
| 过程成因、无隐性汇 | `AGENTS.md` | §3.4 |

## 9. 修订

- R0（2026-08-27）：草案；用户指令开修。
