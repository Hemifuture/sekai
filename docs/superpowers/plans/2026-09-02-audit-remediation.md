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

- [x] Task 2 —— 删除 V4 构造孪生路径与模块级死代码抑制
      `publication.rs` 只调用 V5；`run_tectonic_evolution` /
      `evolve_current_state` / `build_initial_state` / `apply_subduction` /
      `apply_collision` / `apply_divergent_extension` / `fill_spreading_gaps` /
      `commit_process_actions` / `resample_current_state` /
      `reconstruct_connected_plate_domains` /
      `relax_legacy_compatibility_elevation` 共 8 对孪生的非 `_v5` 一半没有
      生产消费者。同时摘掉让这批死代码不可见的 13 个模块级
      `#![cfg_attr(not(test), allow(dead_code))]` 与
      `geodesic_voronoi.rs` 的无条件 `#![allow(dead_code)]`。
      验证：`cargo check --all-features` 无警告 + tectonics 全套件。

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

- [x] Task 5 —— 水面几何扇形面积缓存与海平面 Newton 求解
      `normalized_fan_areas` 只依赖曲面几何却随每次海平面求解重算；
      `solve_level_interval` 二分到 f64 ULP（约 57 次 O(6N) 迭代），而 wire
      路径最终量化到 f32 只需约 24 次。改为按曲面缓存扇形面积，并用
      `dV/dz = 湿面积(z)` 做 Newton 收缩后再以同一 ULP 二分收尾（保持逐位身份）。
      验证：`surface_water_geometry` + `water_volume_sea_level` + `primary_relief_*`。

- [x] Task 6 —— `NaturalTopologyIndex` 改 CSR 并按曲面共享
      索引是曲面的纯函数，却在 `primary_relief.rs` 与
      `surface_formation/hydrology.rs` 每次调用重建；邻接用
      `Vec<Vec<NeighborArc>>`，High 档每次重建 20 万次小堆分配。改扁平 CSR
      并在 P5 window 之间复用。
      验证：`spherical_*` 拓扑消费者套件 + P5 套件。

- [x] Task 7 —— P5 常量出处与重标定（先测后钉）
      `world/natural/surface_formation.rs` 的 P5 常量全部没有作者-年份或
      数据集。先用生产算子在 Task 3/4 之后的地形上实测，再据实测与文献钉值；
      至少覆盖 `FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR`（当前 5000，
      文献坡面值 3e-3 量级）与 `FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR`
      （当前 2e-5 m/yr，100 kyr 累计 2 m，实际等于关闭）。无直接出处者按
      AGENTS.md 记为开放问题交用户裁定。

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

- [x] Task 10 —— 生成热点并行化
      `rayon` 是声明依赖且 AGENTS.md 要求用于 CPU 密集并行，但全仓只有
      `view/spherical_mesh.rs` 使用；生成管线全单线程。按固定分块 +
      顺序合并的既有模式（`spherical_mesh.rs`）并行化逐格热循环，保持归约
      顺序与串行一致。
      验证：确定性断言（同 seed 逐位一致）+ 性能探针。

- [x] Task 11 —— 工程卫生
      `.gitignore` 末尾缺换行使 `docs/**/*.pdf` 与 `screenshots/*.ppm` 粘成
      一行，导致 160 MB 未压缩 PPM 被跟踪；`debug.log`（Chrome crashpad 日志）
      被提交；`benches/`、`tools/` 是空目录；CI 的 clippy 是
      `cargo clippy -- -D warnings` 而门禁要求
      `--workspace --all-targets --all-features`；README 声称 CI 执行 20,000
      单元 release 性能预算，`.github/workflows/rust.yml` 中并无该 job。

## 用户验证步骤

每个改动 UI 可见结果的任务在其提交说明里给出启动方式、面板位置与预期现象。
里程碑收口时给出一次完整的 UI 验收清单。
