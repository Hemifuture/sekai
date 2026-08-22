# P4 水热物理预算校正与连续求解实施计划

日期：2026-08-23

设计：
`docs/superpowers/specs/2026-08-23-p4-physical-budget-correction-design.md`

上游路线图：
`docs/superpowers/specs/2026-08-23-natural-world-scientific-remediation-roadmap.md`
的 R1

## 实施纪律

- 逐任务执行 RED → GREEN → 受影响回归 → 三道门禁 → 单独提交。
- 物理常量与证据包络只定义在 `src/world/`；生成器、质量报告、UI 与测试均
  引用生产事实源，不复制公式或数值。
- 不通过输出后处理、直方图重映射、经验增益或放宽门禁让证据“变绿”。若
  Earth-default 证据失败，先定位缺失过程、量纲、符号或积分语义。
- P4 仍是理想化月气候骨架；R2 才引入陆面 ET、土壤水与地下水。本计划不
  提前伪造这些过程。
- 所有算法改动必须在地图、球面字段和左侧预算摘要中可见，才算完成。
- 每次提交前运行：
  `cargo fmt --all -- --check`；
  `CARGO_TARGET_DIR=target/gates cargo clippy --workspace --all-targets --all-features -- -D warnings`；
  `CARGO_TARGET_DIR=target/gates cargo check --target wasm32-unknown-unknown --all-features --lib`。
- P4/P5 迭代测试使用 `CARGO_TARGET_DIR=target/probe` 与 `--release`；最后两档
  全量测试使用 PowerShell `Start-Process` 分离启动并加 `--no-fail-fast`。

## Task 1：版本化 P4 时间、报告与 checkpoint 语义

文件：

- 修改：`src/world/natural/global_circulation.rs`
- 修改：`src/world/natural/profile.rs`
- 修改：`src/world/natural/mod.rs`
- 修改：`src/generators/natural/global_circulation/generation.rs`
- 修改：`src/generators/natural/global_circulation_stage.rs`
- 修改：`tests/global_circulation_contracts.rs`
- 修改：`tests/global_circulation_generation.rs`
- 修改：`tests/global_circulation_stage.rs`
- 修改：受语义重命名影响的测试与调用点

- [ ] 先写 RED：V1 wire 被严格拒绝；checkpoint 只接受正的完整 forcing-phase
  cycle；report 区分 `formation_cycles`、`continuation_steps`、
  `integrated_model_seconds`；序列化不再出现 year/month 伪语义。
- [ ] 将 global-circulation 与 checkpoint schema 升为 V2；把 profile 上限、
  checkpoint、solve report、错误枚举、fingerprint domain separator 与生成器
  变量统一改为 cycle/phase-step 语义。
- [ ] 保留十二个日历月平均强迫相位和 SI `MACRO_STEP_SECONDS`；由生产常量
  唯一推导 integrated seconds，不新增用户旋钮。
- [ ] 跑 contracts/generation/stage 受影响测试；确认 P0–P3 证据身份不变。
- [ ] 跑三道门禁并提交。

提交：`Version P4 continuation time semantics`

## Task 2：建立短波、灰长波与公开能量账本

文件：

- 修改：`src/world/natural/global_circulation.rs`
- 修改：`src/world/natural/mod.rs`
- 修改：`src/generators/natural/global_circulation/forcing.rs`
- 修改：`src/generators/natural/global_circulation/tendency.rs`
- 修改：`src/generators/natural/global_circulation/generation.rs`
- 修改：`src/generators/natural/global_circulation/project.rs`
- 修改：`tests/global_circulation_forcing.rs`
- 修改：`tests/global_circulation_integrators.rs`
- 修改：`tests/global_circulation_contracts.rs`
- 修改：`tests/global_circulation_generation.rs`

- [ ] 先写 RED 解析测试：surface albedo 上升必使 ASR 降低；太阳辐照为零时
  ASR 为零；CERES 参考 surface albedo 经生产 helper 得到参考 planetary
  albedo；辐射功率与温度 tendency 使用同一项。
- [ ] 在 `world` 定义有文档出处与推导说明的 S0、CERES TOA 参考量、
  atmospheric shortwave reflectance、Stefan–Boltzmann 常量、gray greenhouse
  offset、TOA closure 阈值和唯一 helper。
- [ ] 用 `S0 × daily_mean_insolation`、surface/atmosphere 组合反照率生成逐格
  ASR；删除 `32 * normalized_insolation` 与陆地冬季经验惩罚，以灰体有效
  辐射温度 + greenhouse offset + 高程直减率 + authored offset 构造平衡温度。
- [ ] 把实际 retained external heat 汇总为 `Q_radiative`；同一项推进状态、
  进入数值 energy ledger 并生成 `OLR = ASR - Q_radiative`。
- [ ] V2 fields 增加 `surface_albedo`、`monthly_absorbed_shortwave_w_m2`、
  `monthly_outgoing_longwave_w_m2`；分别走 bounded-intensive 与
  extensive-flux 保守投影，并计入字段 fingerprint 与内存清单。
- [ ] 跑 forcing/contracts/integrators/generation 的 focused Release 回归；禁止
  用输出缩放修正温度。
- [ ] 跑三道门禁并提交。

提交：`Close the P4 radiative energy budget`

## Task 3：以受限水源、相变和潜热替换经验湿度松弛

文件：

- 修改：`src/world/natural/global_circulation.rs`
- 修改：`src/world/natural/mod.rs`
- 修改：`src/generators/natural/global_circulation/forcing.rs`
- 修改：`src/generators/natural/global_circulation/state.rs`
- 修改：`src/generators/natural/global_circulation/tendency.rs`
- 修改：`src/generators/natural/global_circulation/generation.rs`
- 修改：`tests/global_circulation_forcing.rs`
- 修改：`tests/global_circulation_integrators.rs`
- 修改：`tests/global_circulation_generation.rs`
- 修改：`tests/global_circulation_contracts.rs`

- [ ] 先写 RED 解析测试：Bolton `q_sat` 随温度增加；零风、饱和空气、零
  water fraction 的蒸发严格为零；平坦地形 raw-upslope 为零；过饱和经一步
  adjustment 不越界；蒸发/凝结只在 sensible/latent reservoir 间交换能量。
- [ ] 在 `world` 定义 lower-layer reference pressure、air density、Large–Pond
  moisture transfer coefficient、latent heat、参考相对湿度、水循环闭合阈值
  及唯一 production helpers，并在 doc comment 标明来源和推导。
- [ ] forcing 的 surface moisture availability 改为权威 water fraction；初始化
  humidity 使用 `moisture_scale × REFERENCE_SURFACE_RELATIVE_HUMIDITY × q_sat`
  并钳在饱和物理上限。
- [ ] 删除五日 evaporation relaxation、三日 condensation relaxation、
  `OROGRAPHIC_CONDENSATION_DEPTH_M` 与 uplift cap；实现海洋 bulk evaporation、
  saturation adjustment、`rho_air q max(u·grad h, 0)` raw upslope 和最终水汽
  availability limiter。
- [ ] 把 retained evaporation/condensation 同时写入 water mass、surface/lower
  atmosphere 潜热交换与 moist total-energy ledger；生成器只发布同一 retained
  通量。
- [ ] fields 增加 `monthly_evaporation_mm_day`；预算报告增加 final-cycle 全球
  precipitation、evaporation、`E-P`、ASR、OLR、TOA net 与 planetary albedo，
  并以生产 hard closure 规则校验。
- [ ] 跑解析测试、integrators、contracts、generation；用 Release 17 粒探针
  测量而非调参，若偏差存在先作因果分解。
- [ ] 跑三道门禁并提交。

提交：`Conserve P4 water and latent heat`

## Task 4：按名字发布质量指标与 Earth-default 独立证据

文件：

- 修改：`src/generators/natural/quality/global_circulation.rs`
- 修改：`tests/global_circulation_quality.rs`
- 修改：`tests/global_circulation_evidence.rs`
- 修改：`tests/support/global_circulation.rs`
- 修改：`tests/support/natural_quality.rs`
- 修改：受 P4 quality schema 影响的 stage/quality 测试

- [ ] 先写 RED：`EXPECTED_METRIC_NAMES` 为字母序；边界按 metric name 查找；
  合成 report 人工注入 `E-P` 与 TOA net Fail 时被拒，不能借 seed 巧合。
- [ ] 添加规格 §7.1 的无界测量和两项 hard closure；移除位置耦合的 bounds
  zip，集中到一个名字→边界事实源。
- [ ] 冻结 17 粒默认 Earth-like Release evidence；使用生产面积、生产字段和
  `world` 参考常量报告 GPCP V3.2、CERES EBAF 与 latent-heat 偏差。
- [ ] 默认语料的聚合降水须进入 GPCP 参考 `±7%` evidence envelope；玩家参数
  世界不受这个证据包络钳制。记录低/高纬比和两半球季节相位。
- [ ] 跑 quality/evidence 与 P4 stage focused Release 套件，保留确定性哈希。
- [ ] 跑三道门禁并提交。

提交：`Audit P4 water and energy evidence`

## Task 5：为 P5 接入严格 continuation warm start

文件：

- 修改：`src/generators/natural/global_circulation/mod.rs`
- 修改：`src/generators/natural/global_circulation/generation.rs`
- 修改：`src/generators/natural/surface_formation/generation.rs`
- 修改：`src/world/natural/global_circulation.rs`
- 修改：`src/world/natural/surface_formation.rs`
- 修改：`tests/global_circulation_generation.rs`
- 修改：`tests/surface_formation_generation.rs`
- 修改：`tests/surface_formation_contracts.rs`
- 修改：`tests/surface_formation_performance.rs`

- [ ] 先写 RED：同 forcing continuation 可精确续算；changed-forcing 只有
  grid/profile/integrator/quantization/equation identity 全同才可作初值；错状态
  typed rejection；cold/warm 均走同一 equation 与 hard gates。
- [ ] 新增 crate-private validated `GlobalCirculationContinuation`，只拥有 work-grid
  terminal state 和最小身份；它不进入 artifact、engine cache 或 UI schema。
- [ ] 将生成循环收敛路径抽成 cold/warm 共用实现；solve report 标记
  `warm_started`，新 checkpoint 始终绑定当前 forcing。
- [ ] P5 第一次 climate candidate 冷启动，随后外循环回传 continuation；取消、
  原子发布和最大迭代行为不变。
- [ ] 更新 mechanically derived dense-owner inventory，并以 locked High memory
  上限验证。
- [ ] Release 比较 cold P4、same-forcing exact continuation、changed-forcing
  warm agreement 和 P5 Standard wall clock；不降低分辨率、月份或残差要求。
- [ ] 跑三道门禁并提交。

提交：`Warm start coupled P4 continuation`

## Task 6：把 P4 水热场与预算接入两种呈现

文件：

- 修改：`src/world/natural/fields.rs`
- 修改：`src/world/natural/mod.rs`
- 修改：`src/ui/field/localization.rs`
- 修改：`src/app/spherical_formation_display.rs`
- 修改：`src/app.rs`
- 修改：对应字段注册表、文档、呈现和 app 测试

- [ ] 先写 RED：natural field registry 注册年蒸发、年均 ASR、年均 OLR 和
  surface albedo；形成地图与球面 document 返回同一权威 payload；预算摘要
  来自 final formation climate，不在 app 重算。
- [ ] 以字段注册表 + localization 增加四个字段；形成 display cache 只做必要的
  月→年/年均归约，具体公式复用 `world` helper。
- [ ] 扩展 `FormationAreaSummary` 为正交的 `P4WaterEnergySummary` payload，左侧
  显示降水、蒸发、`E-P`、ASR、OLR、TOA net、行星反照率、warm/cold 与 Earth
  reference；标签与数值事实均取 SSOT。
- [ ] 在平面地图和球面切换所有新字段，验证同字段同范围、无 NaN、无越权
  读取生成器内部状态。
- [ ] 跑 field registry、formation display、app、GPU presentation 的 focused
  测试；只刷新实际变化的 registry/presentation identity。
- [ ] 跑三道门禁并提交。

提交：`Expose P4 water and energy budgets`

## Task 7：刷新受影响身份并冻结 R1 完成证据

文件：

- 修改：受影响的 P4、P5、T1 stage fingerprint 期望
- 修改：`src/app/spherical_natural_display.rs`（仅实际 registry hash 改变时）
- 修改：`tests/spherical_presentation_gpu.rs`（仅实际 sampled IDs/goldens 改变时）
- 修改：`tests/global_circulation_atlas.rs`
- 修改：`tests/global_circulation_performance.rs`
- 修改：`tests/surface_formation_atlas.rs`
- 修改：`tests/surface_formation_performance.rs`
- 修改：受影响的 evidence/golden 测试
- 新建：`docs/superpowers/specs/2026-08-23-p4-physical-budget-correction-completion.md`
- 修改：本计划

- [ ] 生成 P4/P5/T1 受影响 Release evidence、atlas、fingerprint 与 golden；逐项
  比对，只接受可由 V2 schema、物理场或注册表解释的变化。
- [ ] 明确列出 P0–P3 不变证据，以及 P4/P5/T1 old→new 指纹、natural field
  registry hash、sampled IDs、16 幅 GPU golden 的实际刷新清单。
- [ ] 记录 17 粒 Earth-default 水热统计、cold/warm agreement、P4 三档、P5
  Standard wall clock、High dense memory 与取消延迟。
- [ ] 跑所有 P4/P5/T1 focused/adjacent Release 套件与冻结的 P0/P2/P3 证据。
- [ ] 用 PowerShell `Start-Process` 分离运行并等待两档全量：
  `cargo test --workspace --all-targets --all-features --no-fail-fast`；
  `cargo test --release --workspace --all-targets --all-features --no-fail-fast`。
- [ ] 跑最后一次 fmt、Clippy、WASM 门禁；在完成记录写明自动验收证据、已知
  边界和规格 §9 的用户 UI 验收步骤。
- [ ] 勾完本计划，提交完成记录；不推送。

提交：`Record P4 physical budget correction evidence`

## 每项承重技术的出处

- 时间/理想化动力核边界：Held & Suarez (1994), DOI
  `10.1175/1520-0477(1994)075<1825:APFTIO>2.0.CO;2`；Frierson, Held &
  Zurita-Gotor (2006), DOI `10.1175/JAS3753.1`。
- 太阳常数：Kopp & Lean (2011), DOI `10.1029/2010GL045777`；IAU 2015
  Resolution B3。
- TOA 与 surface 能量参考：Loeb et al. (2018), DOI
  `10.1175/JCLI-D-17-0208.1`；Kato et al. (2018), DOI
  `10.1175/JCLI-D-17-0523.1`；Stephens et al. (2012), DOI
  `10.1038/ngeo1580`；Wild et al. (2015), DOI
  `10.1007/s00382-014-2430-z`。
- 饱和湿度和参考 RH：Bolton (1980), DOI
  `10.1175/1520-0493(1980)108<1046:TCOEPT>2.0.CO;2`；Manabe & Wetherald
  (1967), DOI `10.1175/1520-0469(1967)024<0241:TEOTAW>2.0.CO;2`。
- 海气 bulk evaporation：Large & Pond (1982), DOI
  `10.1175/1520-0485(1982)012<0464:SALHFM>2.0.CO;2`。
- 地形凝结：Smith (1979) raw upslope；Smith & Barstad (2004) linear
  orographic precipitation；Barstad & Smith (2005), DOI
  `10.1175/JHM-404.1`。
- Earth 降水与水量审计：Huffman et al. (2023), GPCP V3.2, DOI
  `10.1175/JCLI-D-23-0123.1`；Adler et al. (2017), DOI
  `10.1007/s10712-017-9416-4`；Hersbach et al. (2020), ERA5, DOI
  `10.1002/qj.3803`。
- restart/continuation 验证：CESM 1.2 User Guide 的 *Restarting a run*、
  *History and Restart Files* 与 exact-restart tests；changed-forcing 路径另由
  Sekai 的身份检查和 cold/warm agreement 约束。
