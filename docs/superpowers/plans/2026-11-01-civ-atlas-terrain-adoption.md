# Civ Atlas 地形移植实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Sekai 中直接生成移植自 Civ Atlas 的权威地形，并完成 Sekai 气候、水文、显示与配方闭环。

**Architecture:** `crates/civ-atlas-terrain` 承载有来源的纯 Rust 地形算法；Sekai 的新地形来源适配器连接既有气候域和发布接口。上游 TypeScript 只作离线参照，不进应用运行时。

**Tech Stack:** Rust、Cargo、已有 serde/thiserror；离线 Node 参照。新几何依赖只在确认真实算法需求后引入并记录理由。

**Spec:** `docs/superpowers/specs/2026-11-01-civ-atlas-terrain-adoption-design.md`

## Global Constraints

- 固定上游 `176cc9168a69be30d1e09bfd85db84a5f3fb99da`；保留 AGPL/第三方许可与来源。
- 原生和 WASM 的主要计算均为 Rust；不把预制世界或 TypeScript 桥接当成完成。
- 不伪造板块历史、基底、气候、水量或湖泊闭合；初态先验与物理演化分开。
- 初期地形固定，生产气候与最终水文读取同一终态；双向反馈另作实测验证。
- 生成结果、schema、上游输入变化提升对应身份和配方版本；旧配方不静默改变。
- 构建 `-j 2`，生成/性能单进程，命令限 600 秒后检查并终止本任务进程；不做 UDP 压测。
- 用户已批准设计并要求持续执行，不再为任务间切换请求许可；无法科学闭合的项如实记录。
- 用户已澄清要算法而非某张地图；固定 seed 只用于对照，不再等待网址或用户指定场景（规格 A2）。

## Review Focus

- JS 整数溢出、UTF-16 字符与浮点舍入改变随机序列：任务 1 钉上游样本。
- 球面极点、接缝与邻接顺序变化改变大陆：任务 2 对照拓扑及有向邻接。
- 栅格细岸改变海陆却未进入气候与水文：任务 4/5 检查同源身份及聚合。
- 上游河谷与新气候河流冲突、湖面循环依赖：任务 5 做有流量消费者的闭合检查。
- 取消、配方重开或切来源时残留旧图/旧站点：任务 6 复用作者事务契约并加来源回归。

## Task 1: 确定性基础与许可

文件：新增 `crates/civ-atlas-terrain/{Cargo.toml,LICENSE,NOTICE.md,src/lib.rs,src/random.rs,src/noise.rs}`；新增离线参照测试数据；根 Cargo 接入在任务 5 进行。
接口：内部 `Mulberry32::new(u32)` / `next_f64()`、`sub_seed(u32,&str)->u32`、`keyed(u32,i32,i32,i32)->f64`、`round24(f64)->f64`；`Noise3::new(u32)` / `sample(f64,f64,f64)->f64` 及上游 fBm/ridged 参数。

- [x] 从固定源码生成随机数、UTF-16 子流和 3D simplex/fBm 小样本；先写断言并运行失败测试。
- [x] 直接移植所需基础助手和噪声实现，保留上游及 simplex-noise 第三方许可；禁止换成现有 OpenSimplex。
- [x] `cargo test --manifest-path crates/civ-atlas-terrain/Cargo.toml` 通过，定向 Clippy/fmt 通过后提交。
  提交 `0fec733`；6 项上游 oracle 测试通过，独立规格/质量审阅无发现。

## Task 2: 球面网格和几何

文件：新增包内 `src/mesh.rs`、`src/geometry.rs`。
接口：`Mesh` 保留上游 width/height/spacing、x/y/xyz、CSR 邻接及三角形；`build_mesh(width,height,spacing,&mut Mulberry32,&dyn Fn()->bool)->Result<Mesh,TerrainError>`；`blur_field(&Mesh,&[f32],usize)->Vec<f32>`。几何方法按上游真实调用逐项迁移，不实现无消费者的 UI 查询辅助。

- [x] 上游一个小球面夹具记录点、三角形、邻接与跨接缝标记；测试缺失接口应失败。
- [x] 移植 Poisson 球面点、球面 Delaunay、邻接排序、球面距离/噪声/运动；若库的拓扑次序不同，不默许结果差异。
- [x] 拓扑欧拉关系、无孤立点、邻接对称和参照样本通过；执行包测试并提交。
  `b4346f4` 实现；`d834e4f` 修复大半径查询及风带常量；`c76acda` 合并 oracle 来源。
  本包 13 项通过，严格 Clippy/fmt 通过，独立复审通过；原数值夹具未重钉。

## Task 3: 粗地形与侵蚀初态

文件：包内 `src/{params,tectonics,erosion,initial_climate,terrain}.rs` 与对应测试夹具。
接口：`TerrainParams` 对应上游当前用户可用参数；`generate_terrain(&TerrainParams,&dyn Fn()->bool)->Result<Terrain,TerrainError>`；`Terrain` 包含 mesh、高程、海陆/湖泊、水面及来源参数。中间算法类型私有。

- [x] 写一个固定 seed 的上游中间输出对照：大陆场、海底、抬升、侵蚀后高程和湖泊。
- [x] 依赖顺序移植 tectonics、inlandSea、erosion、world 的初态步骤及其真实气候依赖；不移植文明或无用历史。
- [x] 每个中间边界先验证再接下游；确定性与取消测试通过后提交。
  `ce5aa51`：Release 22 项、Clippy/fmt 通过，独立审阅通过。
  两个小场景逐项匹配，其中组合作者参数包含真实非零湖面；一次默认规模
  34,031 格高程/海陆哈希匹配。未把粗生成诊断耗时当作全链时延。

## Task 4: 权威细地形

文件：包内 `src/{raster,shoreline,valleys}.rs`，必要的实际地形依赖单独分文件。
接口：`Terrain::sample_surface(direction:[f64;3])->SurfaceSample`，包含米制地面/水面及水体类型；细域身份由参数、算法版本和粗地形共同派生。

- [x] 选择任务 3 同一世界的一个海湾/河口近景与接缝夹具，记录上游粗细差异。
- [x] 移植实际改变高程的插值、细岸、河谷与水体步骤；仅明暗纹理不进入科学高程。
- [x] 全图及近景忠实性与不同查询顺序一致性通过；记录支持尺度和实测误差后提交。
  `00bf788` 实现，`95bfcac` 统一真极点取样；独立复审通过。
  11,273 个原始像素高程/水类零差；初态湖床适配按规格 A3b 单独记录，
  不把 raw 掩膜当最终水体。生产物理支持域与库存在任务 5 测量后锁定。

## Task 5: Sekai 气候与水文适配

文件：根 `Cargo.toml`、`src/world/natural/atlas_terrain.rs`、`src/generators/natural/formation/atlas_terrain.rs`，相关模块导出和 `global_circulation/forcing.rs`。
接口：`AtlasTerrainArtifact` 绑定合法地形/细域/水库存；强迫构造复用现有 `ClimateTerrainInput` 算子，输出现有气候快照。不能给旧 P2/P3 artifact 填占位历史。

- [x] 先写来源错配、水库存错配和粗细域不同身份拒绝测试；缺失来源失败。
- [x] 将权威地形聚合到粗气候域；按已有来源初始化材料或明确不可用；初始水库存由原水面积分。
- [x] 固定地形运行 Sekai C2，重算与降水匹配的流量/河网和湖泊闭合；检查物质/水量及最终身份。
- [x] 在同一场景实测反馈候选；有依据和闭合才采用，无依据的演化时窗记录为未决，不制造反馈假象。
- [x] 最小全链场景与直接消费者通过后提交；共享身份改动后按项目纪律扩 Release 回归。

完成记录：`3dc4de5` 实现，`1fd46e7` 澄清校验范围；独立规格／质量审阅通过并保留明确限制。
Release 库 666 项、146 个集成目标的 793 项、bin 3 项、doc 6 项、纯包 29 项通过，
fmt／严格 Clippy／WASM 检查与二进制重建通过。真实海岸见证由 118 个误判降至 0。
反馈评估依 A4/A5 完成单位与材料适用性核查：没有可合法直接接入的米制候选，
因此未进行演化实测，也未启用反馈。当前生成约 247.5 秒，远未达到时延目标；
两项气候软质量建议未通过。此为后端任务完成，UI、配方及用户验收仍在 Task 6。
## Task 6: UI、配方、交付

文件：`src/app.rs`、`src/app/{pipeline,field_document,authoring,project_file,spherical_formation_display,site_climatology}.rs` 及 view/gpu 的真实来源绑定处；本地化/注册表与身份事实源。

- [ ] 来源切换、取消、重开旧配方与晚到结果的窄测试先失败。
- [ ] 新来源接入原子发布、二维/三维/气候/河网/点选/站点，显示来源及支持域；不伪装 P2/P3 字段。
- [ ] 新配方保存上游版本/参数及地形来源；旧配方显式兼容；版本事实源只定义一次。
- [ ] Civ Atlas 种子明确为 u32；现有 u64 种子在新来源超出范围时拒绝并显示原因，不静默截断；其他来源不改变种子语义。
- [ ] fmt、严格 workspace/all-targets/all-features Clippy、WASM lib check、针对性 Release 与必要全量通过；重建 native/Trunk。
- [ ] 用实际应用与固定上游算法输出做全图和近景对照，记录端到端耗时/内存及限制；用户 UI 验收单独待确认。
- [ ] 独立整分支审阅，修复重要问题，更新 README/PROGRESS 和来源许可后提交；不自动推送或部署。

## 每项承重技术的出处

- 算法、初态参数、许可：批准规格列出的固定 Civ Atlas 提交；逐文件 NOTICE 映射。
- 随机/噪声/三角剖分：固定版本 util/noise/geometry 与其锁定的 simplex-noise、d3-delaunay/delaunator；移植依赖许可一并记录。
- 侵蚀：上游 erosion.ts 的 Braun & Willett (2013) 与 Priority-Flood 实现，保留实际离散顺序。
- 气候、守恒、身份与 UI：现有 Sekai 生产算子和冻结规格；不复写公式，不新增无出处阈值。
- 局部水体支持的共形细分：Cervenka（2004）§2.6.1.1 的 red/green 父面恢复策略（https://www.iue.tuwien.ac.at/phd/cervenka/node14.html）；球面适用前提、实测故障与验证要求见规格修订 A7。
