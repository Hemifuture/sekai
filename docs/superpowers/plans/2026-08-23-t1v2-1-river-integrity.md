# T1 v2.1 河网完整性实施计划

> **For Codex:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 在不改变 P5 权威水文工件的前提下，消除 T1 河线的水体穿越、
无节点交叉和回折，把生产侧物理河宽传到显示图元与 GPU，并让全球到局部
缩放逐级显露河网。

**Architecture:** P5 的 `RiverSegment`、`SurfaceWaterField` 与共享球面格边是
唯一拓扑事实；`TerrainAmplifier` 把河段拆成陆地扇区 leg 并保存同一物理
宽度，`hierarchical_rivers` 在 leg 内派生纵向单调路径，`app/amplified_mesh`
只按当前 LOD 选择路径，`view` 计算真实横向宽度向量，GPU shader 用相机
uniform 把它连续投影到像素。P5 wire、流量、河级和阈值零改动。

**Tech Stack:** Rust 2024、`wgpu`/WGSL、现有 Goldberg 球面网格、
`cargo test`、BLAKE3 artifact 指纹。

---

## 执行纪律

- 严格 RED → GREEN → REFACTOR；每一任务独立提交。
- 测试调用生产助手或检查生产对象，禁止复制宽度公式、门户算法和 LOD 规则。
- 每次提交前运行：

```powershell
cargo fmt --all -- --check
$env:CARGO_TARGET_DIR='target/gates'
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --target wasm32-unknown-unknown --all-features --lib
```

- P5/T1 产品套件迭代使用 `--release` 和 `target/probe`；最终 debug/release
  全量都加 `--no-fail-fast` 并通过 PowerShell `Start-Process` 分离启动。
- 本会话不推送。

## Task 1：建立共享边门户、陆地 leg 与河宽事实源

**Files:**

- Modify: `src/generators/natural/terrain_amplification.rs`
- Modify: `src/generators/natural/hierarchical_derivation.rs`
- Modify: `src/app/amplified_mesh.rs`（仅更新合成 fixture）

### Step 1：先写失败测试

在 `terrain_amplification.rs` 的现有河流测试旁新增：

1. `river_reaches_split_at_shared_edge_and_omit_water_legs`：构造
   dry→dry、dry→lake、lake→dry、lake→lake，断言 leg 数量分别为 2/1/1/0，
   门户逐位等于权威共享边 `midpoint`，每个 leg 只归属陆格。
2. `river_width_depends_on_discharge_not_strahler_order`：两条相同流量、不同
   河级的 reach 读取生产 `width_m` 后逐位相等。
3. `non_adjacent_river_segment_is_rejected`：非相邻端点返回带 from/to 的明确
   构造错误，不允许质心弦回退。

运行并确认 RED：

```powershell
cargo test --lib terrain_amplification::tests::river_reaches_split_at_shared_edge_and_omit_water_legs -- --exact
cargo test --lib terrain_amplification::tests::river_width_depends_on_discharge_not_strahler_order -- --exact
cargo test --lib terrain_amplification::tests::non_adjacent_river_segment_is_rejected -- --exact
```

### Step 2：实现最小生产模型

- `with_rivers` 增加 `&SurfaceWaterField` 输入；`from_formation_product` 直接传
  `formation.hydrology().surface_water()`。
- 为每条 segment 从两格 `boundary_edges` 的唯一交集取得
  `SphericalSurfaceEdge`；新增 `RiverSegmentNotAdjacent` 错误。
- `RiverReach` 保留单调床端和 `width_m`，新增最多两个 `RiverLeg`；leg 保存
  有向起终点、owner cell、扇区两个边界顶点及完整河段累计弧长分数。
- touching CSR 只登记存在陆地 leg 的 owner cell；水格不参与雕刻。
- 删除 `RIVER_ORDER_GAIN`，宽度只使用现有流量幂律和上下界。
- `HierarchicalEvaluator::with_rivers` 同步接收水体字段并重建缓存。

### Step 3：GREEN 与回归

```powershell
cargo test --lib terrain_amplification::tests -- --nocapture
cargo test --lib hierarchical_derivation::tests -- --nocapture
cargo test --lib amplified_mesh::tests -- --nocapture
```

### Step 4：提交

```powershell
git add src/generators/natural/terrain_amplification.rs src/generators/natural/hierarchical_derivation.rs src/app/amplified_mesh.rs
git commit -m "Split river reaches at authoritative water boundaries" -m "Use shared surface edges and published water classes so T1 carving cannot invent channels through water, while making discharge the sole river-width driver."
```

## Task 2：在权威扇区内生成无自交、无回折路径

**Files:**

- Modify: `src/generators/natural/hierarchical_rivers.rs`
- Modify: `src/generators/natural/hierarchical_derivation.rs`

### Step 1：先写失败测试

新增测试并复用生产 leg/gnomonic 检查助手：

1. `river_path_starts_and_ends_at_published_water_boundary`；
2. `every_path_vertex_stays_in_its_authoritative_sector`；
3. `river_path_progress_is_strictly_monotone`（覆盖随机位移与四点平滑深度）；
4. `different_reaches_only_meet_at_authoritative_cell_nodes`（分支汇流 fixture）；
5. 继续运行既有 query-order、河床单调、走廊与最近点精确性回归。

先运行新测试并确认旧的质心—质心模型不能编译或断言失败。

### Step 2：按 leg 重构缓存和路径

- 每个 reach cache 改为两个独立 leg slot；种子加入 `leg_index`，避免两半路径
  共用随机域。
- `ReachWalk` 以 owner cell 质心建立 gnomonic 纵轴；候选必须同时通过球面
  三角扇区、父节点纵向严格夹持和原走廊检查，零侧移大圆中点是确定回退。
- 四点插值逐点复核同一三项约束，不合格时仅退回该段大圆中点。
- `materialize_path` 按流向拼接非空 legs，并去掉重复门户。
- 最近点分别在 leg 树上执行既有界剪枝，再用 leg 的累计弧长分数映射回完整
  reach 河床分数；复杂度仍为缓存构建 O(节点数)、查询界剪枝。
- `path_depth_cap` 取非空 legs 的最大 cap；每个 leg 自身仍独立钳制。

### Step 3：GREEN、确定性与性能烟测

```powershell
cargo test --lib hierarchical_rivers::tests -- --nocapture
$env:CARGO_TARGET_DIR='target/probe'
cargo test --release --test surface_formation_stage hierarchical_rivers_follow_the_t1v2_contract -- --exact --nocapture
```

### Step 4：提交

```powershell
git add src/generators/natural/hierarchical_rivers.rs src/generators/natural/hierarchical_derivation.rs
git commit -m "Constrain hierarchical rivers to authoritative sectors" -m "Gnomonic longitudinal monotonicity and shared-edge legs prevent self-crossing, cutbacks, and invented waterbody traversals without quadratic intersection scans."
```

## Task 3：把物理河宽写入图元并按 LOD 抽稀

**Files:**

- Modify: `src/generators/natural/hierarchical_derivation.rs`
- Modify: `src/view/spherical_mesh.rs`
- Modify: `src/app/amplified_mesh.rs`
- Modify: `src/app.rs`

### Step 1：先写失败测试

1. `river_polylines_carry_the_production_reach_width`：图元 `width_m` 与 evaluator
   公开的只读生产 width 逐位相等。
2. `river_visibility_reveals_one_order_per_level`：合成 1–4 级链在 L1/L2/L3/L4
   的最低可见级为 4/3/2/1，集合严格单调增加。
3. `visible_river_selection_keeps_downstream_continuity`：任何已选河段的已发布
   下游河段仍在集合中。
4. 更新“路径随更深端点细化”测试，使其区分显示选择与几何深度。

### Step 2：实现最小端到端选择

- `HierarchicalEvaluator` 只读公开 `river_width_m(reach)` 与物理半径，不暴露
  `RiverReach` 内部结构。
- `RiverPolylineSegment` 用 `width_m: f32` 替换 `strahler_order`。
- `build_river_polylines` 从现有 `river_orders` 动态求 `max_order`，以冻结公式
  `order + leaf_level - 1 >= max_order` 选择 reach；几何深度仍由较深端点决定。
- worker 初始构建、相机 LOD 重建、地图和球面继续共用同一折线集合；不新增
  第二套 UI 状态或河网副本。缩放就是用户操作入口。

### Step 3：GREEN 与 UI 数据流回归

```powershell
cargo test --lib amplified_mesh::tests -- --nocapture
cargo test --lib spherical_mesh::tests -- --nocapture
cargo test --lib app::tests -- --nocapture
```

### Step 4：提交

```powershell
git add src/generators/natural/hierarchical_derivation.rs src/view/spherical_mesh.rs src/app/amplified_mesh.rs src/app.rs
git commit -m "Drive river primitives by physical width and view scale" -m "Preserve the full P5 network while exposing one lower Strahler tier per terrain LOD and carrying the production hydraulic width into every visible primitive."
```

## Task 4：在地图与球面 GPU 中连续投影物理宽度

**Files:**

- Modify: `src/view/spherical_mesh.rs`
- Modify: `src/gpu/spherical/overlay.rs`
- Modify: `src/gpu/spherical/renderer.rs`
- Modify: `assets/shaders/spherical_field.wgsl`
- Modify: `src/app.rs`
- Modify: `tests/spherical_presentation_gpu.rs`（仅实际像素改变时）

### Step 1：先写失败测试

1. view helper：给定 start/end、`width_m` 和权威球半径，生成的球面横向
   向量弧宽等于生产宽度；地图横向向量等于左右真实偏移的投影差。
2. renderer/shader：全球尺度使用生产常量 `RIVER_RASTER_FLOOR_PX`，深缩放
   后同一实例的投影物理宽度主导；改变相机只更新 uniform，不重建河网实例。
3. GPU layout/pipeline 测试确保新增 width-vector attribute 与 WGSL 对齐。

### Step 2：实现物理宽度向量

- 在 `view` 提供唯一河宽几何助手：以河段球面中点和大圆法向构造
  `± width_m / (2 × radius_m)` 的真实左右偏移，返回 globe 三维差向量与 map
  投影差向量。
- 复用 overlay instance 现有 padding 空间：map 保存 2D width vector，globe
  保存 3D width vector；普通 overlay 写零，不改变其语义和实例尺寸。
- Rust 唯一定义 `RIVER_RASTER_FLOOR_PX = 1.0`；river instance 的旧 `width`
  槽传该采样下限。
- river shader 在 segment 中点用 `detail_transform` 投影 width vector，求真实
  像素长度，再与传入采样下限取 `max` 后复用现有 quad expansion。
- `app` 不再按河级创造像素宽，只调用 view 助手装配 map/globe instance。

### Step 3：GREEN 与金样

```powershell
cargo test --lib gpu::spherical::renderer::tests -- --nocapture
cargo test --lib spherical_mesh::tests -- --nocapture
$env:CARGO_TARGET_DIR='target/probe'
cargo test --release --test spherical_presentation_gpu -- --nocapture
```

若 GPU 测试只因真实河宽像素改变而失败，按失败输出逐幅核对后才刷新
`EXPECTED_SAMPLED_IDS` 和对应 16 幅金样；非河流视图必须不变。

### Step 4：提交

```powershell
git add src/view/spherical_mesh.rs src/gpu/spherical/overlay.rs src/gpu/spherical/renderer.rs assets/shaders/spherical_field.wgsl src/app.rs tests/spherical_presentation_gpu.rs
git commit -m "Project physical river widths in both presenters" -m "Keep metre-scale channel geometry through the GPU and derive pixels from the live camera, retaining only a one-sample raster floor at global scale."
```

## Task 5：产品证据、指纹、全量回归与交付

**Files:**

- Modify: `tests/surface_formation_stage.rs`（仅实际受影响指纹）
- Modify: `src/app/spherical_natural_display.rs`（仅实际受影响字段 hash）
- Modify: `tests/spherical_presentation_gpu.rs`（仅实际受影响采样/golden）
- Modify: `docs/superpowers/specs/2026-08-23-t1v2-1-river-integrity-design.md`
- Modify: `docs/superpowers/specs/2026-08-20-t1v2-hierarchical-derivation.md`

### Step 1：跑产品级因果 diff

```powershell
$env:CARGO_TARGET_DIR='target/probe'
cargo test --release --test surface_formation_stage -- --nocapture
cargo test --release --test spherical_presentation_gpu -- --nocapture
cargo test --release --test evolved_tectonic_quality -- --nocapture
```

记录：

- P5 seed 42 artifact 必须仍为
  `83a67fc6688db690f0a0e691cce280593febbc5b737b26afcb261479717a7f90`；
- 新的 T1 层级探针、M1 放大器探针以及真实变化的 GPU 金样；
- 未变化的 P3/P4/P5、自然字段 hash 和非河流金样。

只在失败输出与因果链一致时更新 expected。`evolved_tectonic_quality` 的守卫
继续使用合成 Fail 指标，不使用某个 seed 的偶然通过。

### Step 2：追加冻结修订记录

在 T1v2.1 §10 和父规格 A10 写入：旧/新指纹、变化原因、明确未变清单、
代表深度路径完整性测量和构建耗时。禁止把实现数值复制成第二份运行常量。

### Step 3：提交前门禁

运行 fmt/clippy/wasm 三门禁和所有受影响套件；若新 target 首次出现
`thiserror-impl` 瞬时失败，原命令复跑一次并保留第二次结果。

### Step 4：分离启动两档全量套件

为避免 Bash 后台十分钟上限和应用锁住 `target/release/sekai.exe`，分别使用
`Start-Process -WindowStyle Hidden -Wait -PassThru`，输出重定向到
`target/verification/`；release 使用 `target/probe`，debug 使用
`target/gates`：

```powershell
cargo test --workspace --all-targets --all-features --release --no-fail-fast
cargo test --workspace --all-targets --all-features --no-fail-fast
```

逐份检查日志末尾和进程退出码；`--no-fail-fast` 不得遗漏。

### Step 5：最终提交与用户验收

```powershell
git add tests/surface_formation_stage.rs src/app/spherical_natural_display.rs tests/spherical_presentation_gpu.rs docs/superpowers/specs/2026-08-23-t1v2-1-river-integrity-design.md docs/superpowers/specs/2026-08-20-t1v2-hierarchical-derivation.md
git commit -m "Record T1v2.1 river integrity evidence" -m "Lock only causally changed identities and document the unchanged P5 artifact, performance, full regression, and hands-on UI acceptance path."
```

按规格 §9.3 交付启动、地图/球面、缩放、岸线、河宽和汇流检查步骤；明确用户
尚需亲自完成视觉验收。随后开始路线图 R1 的 P4 科学性校正规格，不在本任务
顺手修改 P4/P5。

## 每项承重技术的出处

| 技术 | 计划中的消费者 | 出处 |
| --- | --- | --- |
| 共享边拆线、水体边界节点、禁止自交/回折 | Task 1/2 | USGS EDH Topology Requirements，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-data-acquisition-specifications-7> |
| 河线留在高程表面河槽、下游高程不升 | Task 2 | USGS EDH Positional Assessment / Alignment，<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-acquisition-specifications-1>、<https://www.usgs.gov/ngp-standards-and-specifications/elevation-derived-hydrography-data-acquisition-specifications-19> |
| Gnomonic 大圆直线性质 | Task 2 | Snyder (1987), USGS PP 1395，<https://pubs.usgs.gov/publication/pp1395> |
| 河宽—流量幂律 | Task 1/3/4 | Leopold & Maddock (1953), USGS PP 252，<https://pubs.usgs.gov/publication/pp252> |
| Strahler 河级与多尺度要素选择 | Task 3 | Strahler (1957)，<https://doi.org/10.1029/TR038i006p00913>；Stanislawski (2009)，<https://pubs.usgs.gov/sir/2009/5202/> |
| 一像素线宽下限 | Task 4 | GPU 栅格采样的离散下限；以项目现有 wgpu 像素坐标定义推导，不作为地学参数 |

