# A10 R9 能水耦合研究实施计划草案

> 后续执行按父代理确认的派发方式逐任务进行，并按 executing-plans 或
> subagent-driven-development 做最小验证；本文件当前不授权生产/UI 切换。

**正确性审计修订（2026-10-30）：** 状态仍为研究草案、未冻结。原三个最小核只通过
代数验证；二维输送闭合、周期算法成本及新的非线性停止预算未通过。下列 M1c/M2a
是完整候选之前的淘汰任务，不能拿既往 builder-check 或 Debug 测试替代它们。

**目标：** 保留已验证的能水代数，先裁定开放闭合、实测周期求解及停止预算，再判断
两档 seed42 是否得到有依据的最终态收益。

**架构：** 只在 `cfg(test)` 原型中增加同参考高度 h 与配对边通量；固定有效参考质量，
周期水/热库存共用后向差分。现几何、线性内核、Budyko 与块体 E_pot 是复用边界，
月扫与普通 Picard 只是待测候选，不因复用而预判低成本或收敛。

**技术：** Rust、现 serde_json、Release 忽略探针；无新依赖、无产品开关。

**规格：** `docs/superpowers/specs/2026-10-30-a10-coupled-moist-static-energy-design.md`（R9 草案）。

## 全局约束与审查重点

- 本次审计的开放闭合与停止契约经复核后才派候选实现；规格未冻结，本地提交待终审。
- 不 push、merge；不改生产/UI、不占湖线 UI 窗；cargo/探针/性能先申请独占计算窗。
- 保留 Budyko 与块体 E_pot，不调 RH/D/GMS；不扩17 seed、不保存可加载月世界快照。
- 同高度仍须标注压力；A/B 差异不得隐藏到公共默认助手或让旧原型数值改变。
- 核查低压/非有限/未支持高度与近奇异 GMS，显式失败；不 clamp 科学状态救回候选。
- 核查 Hadley/纬向反号的共同 donor、混合岸格面积、相变双计和周期首尾。
- 新相对误差或质量阈值不在实现时拍定；先输出测量，验收界限须来源/评审明确。
- 旧线性 L2/RHS 精度与物理 RMS/max 不是同一门；真实残差失败时不得 warm-start
  零迭代循环。月扫的慢储量反例和二维纬向盲区必须在世界之前受控检查。

## 本次正确性审计收口（仅文档与外部标量核查）

- [x] 修订二维开放闭合、不同残差意义与停止预算，撤回算法已选定/旧精度自动继承。
- [x] 以只读源码常量的有界脚本重现旧 norm 通过/物理 max 失败、真实行残差修正，
  并实求标量周期 oracle 与空气/海洋月扫；证据见 correctness-audit 子目录。
- [x] 将三个旧 Rust 核限定为代数证据，保留其历史 Debug/Release 验证身份差别。
- [ ] 后续 M1b/M1c/M2a/M2b 实现与科学采纳仍未完成；本次脚本不代替 Rust solver 验证。

## M0：储量量级与定义（本轮）

文件：`prototype/probe.rs`、`prototype/probe/latent.rs`；R9 草案与本计划。

- [x] 核实 `e7a7f84` 干净起点、主树 AGENTS、实际参考高度/压力/质量定义和既有消费者。
- [x] 确认旧证据没有月 q，获准只增加聚合统计忽略入口
  `a10_monthly_latent_storage_probe`；不改求解器。
- [x] 在独占 target offline 增量构建，核对 Cargo 成功 JSON、实际二进制 SHA256 与独有入口。
- [x] Draft/Standard42 各一次；等月相邻采样统计，不冒充 E1 CN 精确储量残差。
- [x] 与旧档案比对默认年字段/预算/非计时诊断逐项相同，记录脚本包装异常与原生退出码。
- [x] 记录旧审阅曾补月块/非线性算法描述；本次正确性审计撤回其精度自动继承与
  算法已选定的推论，按 M1c/M2a 重新验证，不作为完整模型通过记录。
- [x] fmt/clippy/wasm 通过；Release绿色编译因 rustc 访问冲突未验证，按协调者许可用
  默认Debug窄测替代：直接套件16项通过（新3+既有13），3个ignored未运行；不重跑两例。
- [x] 本轮统计/受控核与文档按依赖本地提交；不 push、不 merge、不冻结。

## M1a：同高度状态与配对边核（当前最小受控范围）

修改/新增范围：`prototype.rs` 与其私有 `prototype/coupled.rs`；测试同模块。
仅测试原型提取需要的热力/边装配，不为测试创建公共生产 API。

**接口：** `specific_energies_j_kg(point: SurfacePoint) -> (f64, f64)` 返回 s/h；
SurfacePoint 只含实际参考高度的 T/q/H。边核输入同一两格状态、已验证几何交换系数、
参考质量流，返回 Fq/Fs/Fh 与 HC 上支反号质量；
输出类型只作为私有候选边结果，消费者是受控夹具与后续候选求解。

- [x] 先写失败测试 `r9_edge_pair_preserves_reference_height_and_shared_donor`：
  用生产常量构造同h异H，要求非零潜热通量被干静能补偿而总输送为零；
  同一边另给 psi/Fz 反号，检查独立预期 donor 与 HC 上支质量反号。
  这些不能靠 Fh getter 自身相加或零通量 stub 通过。
- [x] 最小核直接消费已验证的几何交换/质量流系数；未来装配仍从既有
  `mse_edge_transport_kg_s`/`water_edge_transport` 边界取得系数，不复制几何公式。
  Kelvin转换复用现零湿度干焓；HC实际pair与参考驱动区分。
- [x] Release红版重试后3项按缺失实现预期失败；绿色Release遭rustc访问冲突。
  默认Debug新3夹具通过，并进入上述16项直接suite；未用残留红版exe冒充绿色。
  M1a数值核的替代验证完成，不能称M1b或完整耦合模型完成。

## M1b：压力诊断与完整支持域（未实现）

- [ ] 源头/支持域评审后才实现
  `reference_pressure_pa(height_m: f64) -> Result<f64, R9DomainError>`，读取确认的标准大气段。
- [ ] A/B 压力助手复用现 Bolton 常量/实现；测试 p_ref 起点、来源层界、正温压域与
  未支持 H 明确失败，记录 B 的固定有效质量解释。此任务不改默认生产 q_sat。
- [ ] 单独审阅域覆盖、海陆 bulk 与发布 RH 的压力解释；在此之前不启动压力B世界对照。
  ISO 使用位势高度；固定常重力近似的误差与支持域需记录，不能混入未声明的坐标变换。

## M1c：二维分配与干质量开放闭合（未裁定）

文件：R9 §3.3；受控实现仍在 `prototype/coupled.rs` 私有边界，不增加产品参数。

- [ ] 写 `r9_equatorial_zonal_perturbation_exposes_transport_partition`：同纬度两格
  Δh 非零、纬带平均梯度为零、Fz=0，确认 Eddy0 在赤道的缺失项；用最小非赤道邻域
  和纯经向扰动区分方向，避免只检查 Fh getter。比较 Eddy1 的经/纬投影分配及邻接要求。
- [ ] 写 `r9_zonal_projection_accounts_for_dry_mass_and_energy_datum`：对指定可解析
  两库剖面直接积分质量/水汽/干静能，比较 Z0 的 Mq 干质量假定与 Z1 的剖面投影；
  逐格检查连续与配对账。给 s/h/上支能同时加 c 后，闭合柱残差应保持一致；
  不错误要求非辐散流每边能量不变，不以重套 alpha 乘法代替已定义能量的坐标变换。
- [ ] 记录 Eddy0/Eddy1、Z0/Z1 的来源、适用域、干质量/热容关系、GMS 和成本；改变质量时
  同步重推 q/W 与共同 donor，不单改 Fs_Z 系数。能水代数通过不自动证明气候有效。
- [ ] 有直接尺度依据才提交采纳；最接近出处仍需类比的选择列开放问题交用户裁定。
  未裁定时不装配完整世界，也不因陆雨更接近参照而选闭合。

## M2a：周期算法与停止契约的最小淘汰（未完成）

文件：`prototype/coupled.rs` 的私有受控路径；外部
`D:/hemifuture-probes/a10-impl/r9-20261030/correctness-audit/` 保存本次标量反例证据。

**停止接口：** 按 R9 §4.3 实现
`solve_r9_month_block(grid, row, initial, request) -> Result<RealSolve, R9SolveError>`；
row 借用现四邻点 diag/off/rhs，request 仅含本月有单位的逐格行预算和调用上限。
它是候选月块的真实消费者，不升级生产 API、不构造通用 solver。

- [ ] 在任何求解前声明标量月更新、线性/修正调用数和总时长预算及取消/清理方式；
  不根据失败结果临时扩预算，不使用故障 Release 工具链重试来做本次文档核查。
- [ ] 写 `r9_month_block_rejects_linear_success_above_physical_goal`：由生产热容/OLR/
  等月常量构造 R9 §4.3 恒定海柱偏差，确认旧全局 L2/RHS 可过而物理 max 不过；
  原 solver 零迭代不能被包装成物理成功。用真实残差修正或物理目标停止消除偏差，
  达不到请求精度时给失败；上限/停滞路径必须有界，不能反复回到相同 warm start。
- [ ] 实求 `r9_periodic_scalar_sweep_matches_direct_cycle`：同一 BE 标量12月方程，
  由递推消元直接闭合得 oracle，另跑顺序月扫；以空气/无冰海洋两种生产热容检查
  全周期真实残差、首尾、单谐波幅相和月更新数，核对 R9 §4.1 的慢收缩推导。
  这是实际求解对照，不能用已有“解析响应代入差分”测试替代；不扩 seed 或新依赖。
- [ ] 对月块比较真实残差修正与按物理行目标停止的最低成本路径；以水/热残差恒等式
  分配 Bq/Bh，热剩余预算不足时修正 water，避免独立精度混用，不拍安全倍率。
- [ ] 根据 oracle、可达精度、已有全局契约与受控字段误差，先测后钉分别的线性、
  周期行、非线性 RMS/max 和绝对预算；评审后写规格/world SSOT，目标未裁定只能
  报诊断，不称候选已通过。不用原线性常量替代新门的来源。
- [ ] 对低热容、有水相变反馈的受控柱测普通 Picard 收缩；失败则淘汰/重审分块或
  降阶近似，不自行增加混合系数。比较含/省略库存的幅相，并量化相对旧 CN 的差异。
- [ ] 在进入完整候选之前，根据上述误差/成本/资源证据选择周期路径；月扫若不合用
  就停止它或显式修订最小专用路径，不因复用既有 solver 而保留、不造通用框架。

## M2b：同一周期离散的水/热库存（依赖 M1b/M1c/M2a）

文件：`prototype/coupled.rs`；需要共用的最低层线性装配留在 `prototype.rs` 私有边界。
不复制整套 Q1 蒸发/凝结/Budyko 公式。

**接口：** `periodic_storage_rate(values: &[f64; CLIMATE_MONTH_COUNT]) -> [f64; CLIMATE_MONTH_COUNT]`
使用唯一等月 SSOT；同一差分同时进入 Q1 与 E1。既有 12 月 W/T 是未知量，不增加轨迹字段。

- [x] 先写 `r9_periodic_water_and_energy_reduce_to_the_same_dry_budget`：
  给任意非零残差的月数组，证明总能量残差减 L×水残差等于 DSE 残差，差分全年零和。
  未求解闭合水循环，不能把该代数断言称为气候能水守恒已验证。
- [x] 写 `r9_backward_euler_single_harmonic_matches_discrete_response`：
  将无输送线性库的独立BE解析离散响应代入唯一差分核，验证幅相、首尾与常值零变化。
- [ ] 确认 M2a 已实际求解周期夹具、选择路径与误差预算；三个旧代数核不能替代此项。
- [ ] 水式在原物理源汇/边算子上增加本月库存对角与上月库存 RHS；E1 使用干扩散
  与同一 Fq、同一 D_tW。候选协调器单独存在，避免在旧默认路径加入无关开关。
- [ ] 按 M2a 采纳的周期路径实现 R9 月空间块；若采用月扫，water/heat 各按0..11
  顺序更新、前月库存只放 RHS，通过物理预算月块接口调用线性内核。heat 按 DSE
  装配并复用 water 的同一 E/P；不把旧 CN/DFT 或对称系统直接套到非对称候选上。
- [ ] 外层按 R9 §4.1：从 T/W 重建并冻结 psi/Fz/donor/风、冰/ASR/C、active set 和 eps；
  先解周期 W，再解周期 T；从新 T/W 完整重建物理诊断，验收失败后才直接更新 Budyko
  target 进入下一轮。外层必须已在 M2a 证明受控收缩，不自动套 Budyko-only Anderson。
- [ ] 写 `r9_periodic_sweep_rechecks_january_with_final_december`：使初始12月值与最终值
  不同，证明单月线性解通过不代表环闭，并检查最终1月行及全周期残差。
- [ ] 写 `r9_final_residual_rebuild_rejects_stale_wind_or_active_set`：用受控变动的
  T/W 触发 donor/凝结或冰判定变化，旧冻结行即使残差为零，完整状态重建也不得接受。
- [ ] 用 R9 §4.2 定义逐月逐格水/总MSE/DSE/Budyko残差、同边恒等式和掩膜一致性；
  最终检查从同一 T/W/psi/风/eps/冰状态计算，不读旧 RHS 或只看迭代差。数值尺度和
  已经评审的各项预算按规格执行，同时报告最大绝对残差；不复活已撤回的同数值门。
- [ ] 周期扫/外层的轮数、取消与清理预算在本任务开始前由受控测量和协调者窗口明确，
  作为实验资源边界记录，不把用尽预算称为终态，也不临时发明阻尼/扩大超时。
- [ ] 先用含/省略库存两条已声明方程作受控对照；记录守恒、幅相、轮数和失败路径；
  完整候选残差成立再提交 M2b，失败则回到模型定义，不启动世界语料。

## M3：同强迫两例耦合对照

文件：`prototype/probe.rs` 与 `prototype/coupled.rs`；证据根 `D:/hemifuture-probes/a10-impl/r9-20261030/`。

- [ ] 先获独占计算窗；Cargo `--offline --no-run --message-format=json` 提取实际 executable，
  保存 SHA256/源文件哈希和独有入口 `--list`，不依赖历史 exe 文件名。
- [ ] 核对 M1b/M1c/M2a/M2b 的支持域、闭合裁定、实际周期求解、不同残差预算及成本
  证据都已完成；缺一不运行世界，不能用本次外部标量核查补记 Rust solver 通过。
- [ ] 每档只构造一次生产终点强迫，在内存比较 R8、B 含库存、B 省略库存；先 Draft42
  再 Standard42。A 只在 B 压力解释需要分离时经评审追加，不默认组合爆炸。
- [ ] 输出规格§5要求的最终态/逐月账本、压力适用域、GMS 与 HC 驱动/实际 pair 差、
  轮数、阶段耗时、临时量长度；不输出无人消费的迭代场或可加载世界快照。
- [ ] 实测后写中文结果修订：机制、守恒、质量、成本与失败完整报告；不因两例改善
  宣称17 seed门通过。没有可解释收益就停止候选，不扩语料。
- [ ] 相关目标 suite 与 fmt/clippy/wasm，独立审阅后本地提交 M3；不进入生产 T8。

## M4：后续 UI 接入（本轮不执行）

- [ ] 候选与支持域获批后安排 T8 的 reduced 方程/profile、阶段实现和 recipe 兼容版本
  升版；审查 LOD-2 RH/压力派生规格修订，字段不变时不滥增 schema。
- [ ] 在独立预览分支一次接入最终字段，不加产品开关；不与湖线 UI 窗竞争。
- [ ] 用户运行 `cargo run --release --bin sekai`，选择 Continents、Draft/Standard、seed42，
  看二维降水/温度/风、三维极区和海岸/山地/内陆；L0 到支持叶片点选/显示同源。
  用户本人验收前不称算法交付。

## 每项承重技术的出处

- 同高度 s/h 与标准热力近似：R9 S1/S2，Siler et al.2018；Yano & Ambaum2017。
- 压力/参考柱选择与支持段域：R9 S3/S4，USSA1976/ISO2533 静力式、Bolton1980；
  固定有效质量的二维适用性明确为类比候选，不能据文献名省略验证。
- HC 上下支/GMS：R9 S1/S5，Siler2018、Neelin & Held1987、Raymond et al.2009；
  二维 Eddy0/Eddy1 与 Mq 干质量 Z0/Z1 为开放类比，不宣称这些文献直接背书。
- 共同周期后向差分：R9 S6，PETSc TSBEULER 方法定义；离散幅相解析夹具负责验证实现。
- 月空间 BiCGSTAB、周期块扫/矩阵分裂：R9 S7，van der Vorst1992、Saad2003
  §4.1–4.2、§7.4.2；标量周期 oracle/收缩因子从同一 BE 递推消元，不是新经验模型。
- 月块/周期/非线性误差分开：R9 §4.2–4.3 的单位和残差恒等式，M2a 先测后钉；
  数值书与 PETSc 方法定义均不保证本气候 Picard 收敛或月季节精度。
- Budyko/块体 E_pot/守恒边几何沿用 R7/R8 既有出处与生产助手，不新增系数或阈值。
