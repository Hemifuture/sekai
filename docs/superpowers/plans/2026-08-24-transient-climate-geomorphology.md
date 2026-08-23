# P5 瞬态气候—地貌共演实施计划

**Goal:** 用连续海岸、前向多速率演化和守恒全水圈替代 100 ka 终态不动点，
再在可信径流上完成河网标定与 UI 交付。

**Architecture:** `world` 保存统一水面几何、水库与报告；`generators` 以一个
水面算子服务 P3/P4/P5，并从 P3 只初始化一次完整形成状态。每个慢时间窗在
当前地形上重求 P4，整步/两半步控制误差。地图、球面和摘要只读同一发布
payload。

**Tech Stack:** Rust 2024、现有球面多边形/保守重映射、P4 split-explicit、
P5 地貌算子、serde/BLAKE3、egui、cargo。

## 执行纪律

- 严格 RED -> GREEN -> 受影响回归 -> 三道门禁 -> 独立提交。
- 新常量先用生产算子测量，再把语料、来源和取值过程写入规格修订；不得先钉
  数字再倒补依据。
- 经验地球范围只做 evidence；守恒、非负、拓扑、时间离散误差、schema 和
  身份是 hard gate。
- 测试复用生产水面、体积、月水量、时间误差和摘要 helper，不复制算法。
- 每次提交前运行：

```powershell
cargo fmt --all -- --check
$env:CARGO_TARGET_DIR='target/gates'
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --target wasm32-unknown-unknown --all-features --lib
```

- P4/P5 迭代用 `CARGO_TARGET_DIR=target/probe` 与 `--release`；最终 debug/release
  全量加 `--no-fail-fast`，用 `Start-Process -WindowStyle Hidden` 分离运行。
- 本计划不推送。算法任务必须完成 UI 和用户验收步骤后才能称为交付。

## Task 1：显式废止 Aitken 设计并清理失败原型

文件：

- 新建：`docs/superpowers/specs/2026-08-24-transient-climate-geomorphology-design.md`
- 新建：`docs/superpowers/plans/2026-08-24-transient-climate-geomorphology.md`
- 修改：`docs/superpowers/specs/2026-08-23-p5-coupling-stability-design.md`
- 修改：`docs/superpowers/plans/2026-08-23-p5-coupling-stability.md`
- 修改：`docs/superpowers/specs/2026-08-18-coupled-geomorphic-formation-p5-design.md`
- 修改：`docs/superpowers/specs/2026-08-23-natural-world-scientific-remediation-roadmap.md`
- 删除：未提交 `surface_formation/coupling.rs` 及只服务 Aitken 的工作树改动

- [x] 记录真实接口残差、强制未松弛复核和海岸活动集证据，说明为何不是继续
  调系数或换 Anderson/IQN。
- [x] 冻结统一水面、前向共演、全水圈库存语义与 UI 边界；未实测常量明确留到
  对应 probe task，不伪造默认值。
- [x] 旧冻结规格加显式 R1/A4 修订，旧计划标记终止；路线图依赖顺序改为
  P4 证据债 -> 连续海岸 -> 前向共演 -> 陆面水量 -> 河网标定。
- [x] 精确移除未提交 Aitken schema/report/forcing interface/状态机/测试，不动
  已提交 P4/P5 行为；`git diff --check`。
- [x] 提交。

提交：`Supersede the P5 fixed-point coupling design`

## Task 2：修复 P4 比较参考与阶段身份债

文件：

- 修改：`src/generators/natural/global_circulation/comparison.rs`
- 修改：`src/generators/natural/global_circulation/mod.rs`
- 修改：`src/generators/natural/global_circulation_stage.rs`
- 修改：`tests/global_circulation_comparison.rs`
- 修改：受影响的 P4 stage/evidence 期望
- 修改：P4 规格显式修订、本计划与 R1 完成记录

- [x] RED 证明 refined reference 当前错误实例化
  `SplitExplicitRk3Integrator`，并让 reference/candidate procedure identity
  自比较；伪造同 identity 的候选必须不再通过。
- [x] reference 改为实际 `ExplicitRk3Integrator` 的细步同方程路径；实际执行
  identity 与报告 identity 由被运行的积分器给出。
- [x] P4 stage/model identity 只按方程/比较证据实际影响刷新；重跑 formation
  cycle comparison、integrator corpus、17-seed evidence。
- [x] 完成 P4 计划中尚未完成的 Task 7 产品身份清单，显式关闭旧 fixed-point
  P5/T1 金样范围；三道门禁并提交。

提交：`Use an independent RK3 climate reference`

## Task 3：实现统一亚格元水面几何生产算子

文件：

- 新建：`src/generators/natural/surface_water_geometry.rs`
- 新建或修改：`src/world/natural/surface_water_geometry.rs`
- 修改：`src/world/natural/mod.rs`
- 修改：`src/generators/natural/mod.rs`
- 修改：`src/world/natural/primary_relief.rs`
- 修改：水面解析/契约测试

- [x] RED：全陆/全海、单个线性三角形半淹没、共享边两侧逐位同湿润比例、
  面积分数和为 1、常数高程退化、取消、长度/NaN/拓扑拒绝。
- [x] 生产重建共享顶点高程、格元三角扇、解析湿区面积/线性水深积分；只保留
  一个 `SurfaceWaterGeometry` payload。
- [x] `water_volume_at_sea_level_m3` 和海平面求解调用同一算子；RED 证明体积
  连续单调、水量闭合、旧平顶格元分支已删除。
- [x] 运行 fixed surface、P3 contracts/generation/quality 与性能 probe；登记
  dense owner、取消延迟和默认水线差异，不用后处理恢复旧分布。
- [x] 三道门禁并提交。

实测记录：17-seed 连续陆地面积中位数为 `0.1943376362323761`；Draft seed 42
海平面为 `-64.06 m`。Draft/Standard/High dense owner 为
`20,252 / 79,212 / 198,812` 格元，Draft 全 P3 为 `2,752,963 us`，后两档
取消延迟为 `372,670 / 851,267 us`。完整 old→new 身份与算法细节见冻结规格 R1。

提交：`Unify fractional surface-water geometry`

## Task 4：把连续海岸接入 P3、P4 与 P5

文件：

- 修改：`src/world/natural/primary_relief.rs`
- 修改：`src/world/natural/circulation/forcing.rs`
- 修改：`src/generators/natural/primary_relief.rs`
- 修改：`src/generators/natural/global_circulation/forcing.rs`
- 修改：`src/generators/natural/surface_formation/{isostasy,coast,hydrology,generation}.rs`
- 修改：P3/P4/P5 contracts、forcing、generation 与 stage 测试

- [ ] P3 snapshot 发布权威水面几何/指纹，物理陆地比例改为连续面积；目标陆地
  模式使用同一连续面积求根。
- [ ] P4 land fraction 保守重映射权威分数；work-grid 海洋边通透性来自 P1
  共享边湿长，删除 `min(first_water, second_water)`。
- [ ] P5 海岸交换、海洋终端和每步海平面共同消费水面 payload；离散
  `LandOceanField` 只由该 payload 派生。
- [ ] Release probe 复现旧 0.181 m 海平面扰动，证明通量/面积连续且总水量
  不变；真实海峡开闭仍产生合法拓扑事件。
- [ ] 刷新实际受影响 P3->P5 身份，三道门禁并提交。

提交：`Drive the natural pipeline with fractional coasts`

## Task 5：将 P5 改为前向多速率共演

文件：

- 修改：`src/generators/natural/surface_formation/generation.rs`
- 修改：`src/world/natural/surface_formation.rs`
- 修改：`src/generators/natural/surface_formation_stage.rs`
- 修改：P5 contracts/generation/stage/performance 测试
- 修改：规格时间误差实测修订

- [ ] RED：两个连续 coupling window 必须从前一完整 component/sediment 状态
  继续；测试能区分旧“回到 P3”行为。
- [ ] 把 `solve_geomorphic` 拆为一次初始化和一次可克隆/可回滚的前向 window；
  checkpoint 保存累计模型时间，不再保存 fixed-point outer iteration 语义。
- [ ] 先跑固定 `dt`、`dt/2`、`dt/4` 的 seed 3/7/42 Release 探针，记录生产
  高程、沉积、水文拓扑和性能收敛；据方法阶数与量化事实冻结误差归一尺度。
- [ ] 实现整步/两半步接受与确定性缩步；中点重求水面与完整 P4，拒绝/取消
  原子回滚，接受更细的两半步状态。
- [ ] `FormationSolveReport` 改为接受/拒绝窗、P4 solve 次数、累计时间、最小
  时间窗和最大误差；旧 residual 仅保留为诊断时不得继续宣称 fixed point。
- [ ] seed 7 不再出现 A/B 历史重放；满足时间精度、守恒与预算后提交。

提交：`Evolve climate and landforms forward in time`

## Task 6：实现可迁移全水圈与月尺度陆面水量

文件：

- 修改：`src/world/natural/{relief_spec,primary_relief,surface_formation,global_circulation}.rs`
- 新建：陆面水量 world/generator 模块
- 修改：P4 forcing/tendency/generation 与 P5 hydrology/generation
- 修改：schema、contracts、quality 与 evidence 测试
- 修改：T0b/P4/P5 规格显式语义修订

- [ ] RED：总库存精确分解为海洋/湖泊/土壤/地下水/雪冰/大气；任何负水库、
  单边通量或篡改派生总量严格拒绝。
- [ ] 用当前生产 P4/P5 语料测量月 P/T/辐射/湿度、基质和潜在库容范围；从
  WaterGAP/同行评审数据选择容量、相变与排泄参数，写明数据版本/取值过程。
- [ ] 实现雪、单层土壤和慢地下水月平衡；实际 ET 受能量需求与可用水共同
  限制，`Q_fast + Q_base` 成为唯一 P5 径流事实。
- [ ] ET/降水/径流/补给/基流与 P4 潜热、水汽源成对记账；海平面只使用扣除
  其他水库后的实际海洋体积。
- [ ] `water_inventory_ratio` wire/文案升级为可迁移库存，默认值和合法范围仍
  引用 T0b 事实源；P3 初始分配经实测修订冻结。
- [ ] 运行解析闭合、17-seed 水圈 evidence、P4/P5 回归和三道门禁并提交。

提交：`Conserve the mobile hydrosphere`

## Task 7：在守恒径流上标定河道起始与 hydraulic geometry

文件：

- 修改：`src/world/natural/hydrology.rs`
- 修改：`src/generators/natural/surface_formation/hydrology.rs`
- 修改：`src/generators/natural/hierarchical_rivers.rs`
- 修改：河网 quality/evidence/atlas 测试
- 修改：R2 规格实测修订

- [ ] 用生产多分辨率/多 seed 语料测量汇水面积、局地坡度、输水能力、基流和
  当前河道起始；先记录现状再冻结判据。
- [ ] 以 Montgomery–Dietrich 面积—坡度/输水能力机制替换单一绝对流量承担
  所有分辨率；阈值来自文献或公开数据处理，不拟合输出直方图。
- [ ] hydraulic geometry 只消费物理流量和可辨识环境组；不再由 Strahler 级
  乘第二份视觉宽度增益。
- [ ] 河网密度、级序、湖泊连通和地球参考只作 evidence；邻接、无环、单调
  排水面和水量闭合继续 hard gate。
- [ ] 跑 P5/T1/GPU focused 回归、三道门禁并提交。

提交：`Calibrate channels from conserved runoff`

## Task 8：接入地图、球面与左侧形成摘要

文件：

- 修改：`src/world/natural/fields.rs`
- 修改：`src/ui/field/localization.rs`
- 修改：`src/app/spherical_formation_display.rs`
- 修改：`src/app.rs`
- 修改：字段注册表、app、presentation 与 GPU 测试

- [ ] 注册连续海洋分数、土壤水、地下水、雪水、ET、总径流和基流；地图与
  球面共用同一 `FieldDocument` payload/range。
- [ ] 摘要逐位复制前向 solve report 和水圈 budget，显示时间误差、窗口统计、
  库存分配及水量闭合；app 不重算公式或数值。
- [ ] 现有“海水量”更名“可迁移水库存”，另显示实际海洋体积；不增加时间步、
  容差或求解器旋钮。
- [ ] 检查窄/宽桌面与移动视口无重叠、无截字，地图/球面切换字段一致。
- [ ] focused 测试、三道门禁并提交。

提交：`Expose transient hydrosphere diagnostics`

## Task 9：冻结证据、身份与用户验收

文件：

- 修改：P3/P4/P5/T1 evidence、atlas、performance 与 stage 期望
- 新建：`docs/superpowers/specs/2026-08-24-transient-climate-geomorphology-completion.md`
- 修改：本计划与上游未完成记录

- [ ] 生成 17-seed Release evidence；列出 P0–P2 不变及 P3->P5->T1 的实际
  old->new 指纹、字段注册表、sampled IDs 和 GPU golden 清单。
- [ ] 记录海岸连续性、`dt/2`/`dt/4` 收敛、总水/沉积闭合、河网检测、三档
  P4/P5 wall clock、High memory 与取消延迟。
- [ ] 跑全部 affected/adjacent Release 套件与冻结 P0/P2 证据。
- [ ] PowerShell 分离运行并等待：

```powershell
cargo test --workspace --all-targets --all-features --no-fail-fast
cargo test --release --workspace --all-targets --all-features --no-fail-fast
```

- [ ] 最后 fmt/Clippy/WASM；启动应用并提供 URL。完成记录明确自动验收不能
  替代用户 UI 验收，并写明启动、字段/摘要位置和预期现象。
- [ ] 勾完计划并提交，不推送。

提交：`Record transient hydrosphere delivery evidence`

## 每项承重技术的出处

- 独立时间积分参考：Hairer, Nørsett & Wanner (1993), *Solving Ordinary
  Differential Equations I*；Task 2 必须以实际 classic RK3 为 reference。
- 前向地形—气候共演：Paik & Kim (2021), DOI
  `10.5194/hess-25-2459-2021`。
- 多速率耦合：Gladstone et al. (2021), DOI `10.5194/gmd-14-889-2021`。
- 分数海岸与质量守恒：Meccia & Mikolajewicz (2018), DOI
  `10.5194/gmd-11-4677-2018`；CMEPS *Fractional grids*。
- 不规则点距离权重：Shepard (1968), DOI `10.1145/800186.810616`；P1 三角
  线性积分：Dunavant (1985), DOI `10.1002/nme.1620210612`。
- 全局陆面水量：Müller Schmied et al. (2021), WaterGAP v2.2d, DOI
  `10.5194/gmd-14-1037-2021`；地下水—河网：Litwin et al. (2022), DOI
  `10.1029/2021JF006239`。
- 河道起始：Montgomery & Dietrich (1988), DOI `10.1038/336232a0`；
  hydraulic geometry：Leopold & Maddock (1953), USGS PP 252。
- 固定点算法适用域：Walker & Ni (2011), DOI `10.1137/10078356X`；preCICE
  acceleration 文档。它们只支持失败审计，不再作为全历史算法。
