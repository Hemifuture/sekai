# 审计整改里程碑 A0 实施计划（2026-09-02）

上位：`AGENTS.md`；设计真相
`docs/superpowers/specs/2026-09-02-audit-remediation-design.md`。

来源：2026-09-02 对全管线（P2→P3→P4→P5→T1、空间基础设施、UI）的静态审计，
交叉核对 `target/natural-quality/` 已记录证据。审计发现按"单位工作量收益"
排序后固化为下面的任务队列。一任务一提交，提交前跑 fmt / clippy / wasm 门禁，
测试范围按影响面在交付说明里写明。

## 任务队列

- [x] Task 1 —— P5 步长选择复用已接受速率
      `evaluate_current_processes` 克隆整个 `FormationState` 并跑一个完整
      1 年 window，只为取 `blocked_by_elevation_domain` 与
      `maximum_elevation_domain_step_years` 两个限幅量。实测
      `pre-migration-one-advance.json`：`step_selection_micros` 521030 对
      `kernel_micros` 517289，占推进耗时 50.2%。改为用已接受 window 自身
      返回的 `process_rates` 作为下一步的预测子（标准 predictor 步长控制），
      只在进入循环前保留一次探针。不改任何被求解的方程。
      验证：`surface_formation_*` 套件 + `causal_formation_generation`。

- [x] Task 2 —— 摘掉模块级死代码抑制并删除编译器证实的死代码
      **审计更正（2026-09-02）**：初稿判断"V4 构造孪生路径无生产消费者"是
      **错的**。`foundation/tectonics.rs:79/102` 调用 `evolve_current_state`
      与 `run_tectonic_evolution`，它们是 `TectonicGenerator::generate_spherical`
      也就是 `WorldPipeline::LegacyFoundation` 的实现。审计当时的 grep 被
      `head -20` 截断而漏掉了该文件。V4 不删。LegacyFoundation 未接 UI、
      文档说明它为任意分辨率（如 162 格测试世界）保留，是否退役属于产品裁定，
      不在本里程碑范围。

      实际执行：摘掉 14 个模块级
      `#![cfg_attr(not(test), allow(dead_code))]` 与 `geodesic_voronoi.rs`
      的无条件 `#![allow(dead_code)]`，让编译器直接指认生产死代码，然后
      按指认结果删除：整文件死亡的 `morphology/area.rs`（1271 行）与
      `morphology/field.rs`；`morphology/metric.rs` 只剩 Dijkstra 真正需要的
      `PositiveEdgeMetric`；`assign_arrivals` 无界包装、`sample_coordinate`、
      8 个失去生产者的 RNG 子流标签、以及没有任何读者的配方字段
      `island_arc_gain_permille`。仅测试需要的助手改用 `#[cfg(test)]` 精确
      标注，而不是整模块放行。
      验证：`cargo check --all-features --all-targets` 零警告 + `--lib` 461 项
      单元测试 + tectonics 集成套件。

- [x] Task 3 / 7 / 9 —— P5 物质账本与侵蚀量级（合并为一次提交）
      三项在物理上不可分割：打开淤积会暴露被过大侵蚀掩盖的量级错误，而量级标定
      必须在侵蚀律改用真实流量之后做一次，否则要标定两遍。详见设计 §2/§4/§6。

- [x] Task 3 —— 启用河道淤积（`V_eff = G · 局地径流`）
      `FORMATION_DETACHMENT_LIMITED_EFFECTIVE_SETTLING_VELOCITY_M_PER_YEAR = 0`
      使 `davy_lague_deposition_fraction` 对每个有受体的格元返回 0，陆上没有
      任何淤积。这是 `corpus-median-land-area-share-below-100m` = 0.0487
      （包络 ≥ 0.10，fail）与 `corpus-median-land-relief-p05-m` = 101 m
      （包络 ≤ 80 m，fail）的机制成因。按设计 §2 改为
      `V_eff = G · 局地径流速率`。
      验证：`formation_sediment` + `surface_formation_quality` + 测高探针。

- [x] Task 4 —— P4 涡动湿度与热量扩散闭合
      P4 只有动量的 `horizontal_velocity_diffusion` 与斜压 Reynolds 应力闭合，
      湿度只被解析平均流平流。24²–48²/面的网格不解析斜压涡旋，于是向极水汽
      输运缺失：`precipitation_low_to_high_latitude_ratio` 实测
      80–3195（均值 681），地球约 3。按设计 §3 增加与既有两点通量算子同构的
      标量下梯度扩散。
      验证：`global_circulation_*` 套件 + `layered_circulation_physics`。

- [x] Task 5 —— 海平面求解改安全化 Newton（扇形面积缓存按实测取消）
      Newton 已落地，解逐位不变（denudation 探针九项分量与改前完全一致）。
      **实测修正审计估计**：Draft 档 `primary_relief_generation` 只从
      `1.262 s` 降到 `1.246 s`（约 1%），全链单 seed `81.35 s → 80.78 s`。
      海平面二分不是该阶段的主导开销（主导的是 conditioned regional detail 的
      FBM 与 sparse Gabor 噪声），因此扇形面积缓存所需的 workspace 穿线不做——
      它要改动多个调用点，换来的是不到 1% 的一部分。

- [x] Task 6 —— **按实测关闭，不实施**
      审计说索引反复重建且用 `Vec<Vec<_>>`，属实；但量级估计错了。
      `from_surface` 是一次 O(E) 扫描加每格一次小排序：Draft 档 60,750 边约
      3–5 ms，一次构建重建约 4 次，合计约 20 ms，占单 seed 全链 `40 s` 的
      **0.05%**；High 档比例相同。改 CSR 要重写一个被广泛消费的类型，换 0.05%
      不成立。按 AGENTS.md「只在有真实消费者与实测收益时才加优化机制」关闭。
      若将来 P5 的图遍历成为主导项，此项可重开。

- [x] Task 7 —— P5 侵蚀量级先测后钉
      新增 `tests/formation_denudation.rs`（ignored / Release）用九项高程组成
      测量陆地剥蚀率并对观测设门。实测把
      `FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR` 从 `5.0e-6`
      钉到 `5.0e-7`（`672/747 → 49/52 m/Myr`，全球 `10Be` 中位数 `54`）。
      实测同时**否定**了审计对 `FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR`
      的量级怀疑：该项只贡献 `13 m/Myr`，本轮不动。
      `FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR` 的几何语义未定，留作开放问题。

- [x] Task 8 —— 裁决「构造活动」旋钮
      `apply_boundary_torques_to_current` 在第一步之前无条件覆盖
      `plate.rotation`，力矩右端项不含旧 ω，所以 activity 选出的初始板速在
      任何物质移动前就被丢弃。按设计 §5 把它接进驱动力量级。
      验证：新增一条端到端断言（同 seed 下 Quiet 与 Active 的发布板速可分辨）。

- [x] Task 9 —— 河流侵蚀改用积分流量
      `A^0.5 × clamp(sqrt(局地径流/1000mm))` 只在径流均匀时是 `Q^m` 的合法
      代理，而 P4 解的是高度非均匀气候。改用同一 window 已算出的
      `mean_annual_discharge_m3_s`。
      验证：`formation_stream_power` + P5 质量套件。

- [ ] Task 10 —— **测完后停下，需用户裁定架构取舍**
      实测分解（Draft，单 seed 全链约 `40 s`）：P1–P3 约 `2.9 s`
      （`p3/performance.json`），P4 约 `17 s`（17 seed 气候证据 `349 s / 17`
      减去上游），其余约 `20 s` 是 P5 加终点 P4。
      结论与审计不同：
      - P5 的主导项是**天然串行**的图算法（Priority-Flood、按拓扑序的隐式
        河流、沉积路由），并行化无从下手；
      - P4 才是最大单项，但它的内层循环只有 `3456`（Draft）到 `13824`（High）
        个气候格元，成本来自**调用次数**而不是单次规模，rayon 的每次调用开销
        与之同量级；
      - 更要紧的是 `rayon` 在 `Cargo.toml` 里是 `cfg(not(wasm32))` 的依赖，
        在生成管线里用它就要给科学 kernel 加平台条件分支，破坏「生成管线与
        平台无关」这条现有不变式。
      因此不擅自实施。是否接受"原生并行 / wasm 串行"的双路径，属于产品与架构
      取舍，交用户裁定；若接受，最小可行范围是 P4 tendency 里逐格不相交的
      热力学循环（`apply_moisture` 等），按固定分块保证与串行逐位一致。

- [x] Task 11 —— 工程卫生
      `.gitignore` 末尾缺换行使 `docs/**/*.pdf` 与 `screenshots/*.ppm` 粘成
      一行，导致 160 MB 未压缩 PPM 被跟踪；`debug.log`（Chrome crashpad 日志）
      被提交；`benches/`、`tools/` 是空目录；CI 的 clippy 是
      `cargo clippy -- -D warnings` 而门禁要求
      `--workspace --all-targets --all-features`；README 声称 CI 执行 20,000
      单元 release 性能预算，`.github/workflows/rust.yml` 中并无该 job。

## 用户验证步骤

启动：`cargo run --release`（应用占用 `target/release/sekai.exe` 时，代理侧用
`CARGO_TARGET_DIR=target/probe`）。左侧「自然世界」面板设参数，按「按当前参数
重建」。本里程碑没有新增字段或面板，改动全部落在既有已接入 UI 的产物上。

1. **构造活动确实控制板速**（Task 8）——固定根种子与世界形态，把「构造活动」
   在 宁静 / 适中 / 活跃 之间切换并各重建一次。在字段目录里看**板块**与
   **构造边界**：活跃档的边界带应明显更宽更密，宁静档更收敛。改动前这个下拉
   框对发布态没有可见作用。

2. **陆上有沉积了**（Task 3）——字段目录选**沉积厚度**（`FormationSediment`
   系列）。改动前陆地上只有内流终端有厚度，其余为零；现在河谷与下游应出现连续
   的沉积带。同时看**高程**：河谷剖面比改动前平缓。

3. **地形不再被过度啃平**（Task 7）——同一根种子对比改动前后的**高程**图：
   山体保留更多，整体不再像被均匀削过一遍。定量上陆地剥蚀率从 `672 m/Myr`
   降到 `49 m/Myr`（全球观测中位数 `54`）。

4. **中高纬有降水了**（Task 4）——字段目录选**年降水**。60° 以外不再是纯零，
   低/高纬降水比从 `681` 降到 `301`。**注意**：离地球的 `≈3` 还很远，这是已知
   的开放问题（设计 §8 开放问题 1），本次只交付了水汽输运那一半。

5. **测高分布仍缺低地**——这是本里程碑**没有**解决的，且现在暴露得更清楚：
   低于 100 m 的陆地只占 `4.3%`（地球 20%+）。成因在 P3 陆壳厚度分布与陆缘
   减薄，归短期地理路线图 §G3。看**高程**图的海岸带：仍然是台阶而不是平原。

6. **回归**：全量 Release 套件 142 个测试二进制、1247 passed / 0 failed /
   47 ignored；fmt / workspace clippy / wasm32 三道门禁全绿。
