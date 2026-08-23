# P4 水热物理预算校正与连续求解设计

日期：2026-08-23
状态：冻结；是
`2026-08-17-global-atmosphere-ocean-p4-design.md`、
`2026-08-17-global-atmosphere-ocean-p4-completion.md` 与
`2026-08-23-natural-world-scientific-remediation-roadmap.md` 的 R1 实施修订

## 1. 结论

本修订保留 P4 的球面有限体积动力核、C2 层结、守恒传输、生产
`SplitExplicitRk3V1` 和 P3 精确耦合，但撤销四项不再成立的完成结论：

1. 原 `formation_years` 不是物理年：每个“月”只推进一个
   `MACRO_STEP_SECONDS`，随后立即换相位；本修订把它版本化为十二个日历月
   强迫相位上的确定性 climatological continuation，求解循环与物理历时分开
   命名，不再把数值迭代冒充年月。
2. 原 `surface_albedo` 没有进入短波功率，所谓能量闭合只证明牛顿松弛账本
   自洽；本修订建立显式 TOA 吸收短波、线性化长波响应和公开功率账本。
3. 原水汽源是向经验平衡湿度的五日松弛，能无限补水且不受风速、饱和差或
   表面能量约束；本修订以 bulk aerodynamic evaporation、饱和调整、原始
   upslope source 和潜热交换替代。
4. P5 每次外循环都冷启动 P4；本修订增加只在生成器内部流转的、严格验证
   的 continuation state。它只换初值，不换方程、不放宽门禁，也不成为新
   artifact 或用户旋钮。

P4 仍是理想化的大尺度月气候骨架，不声称具备云、土壤水、植被、雪、海冰、
天气事件或完整辐射传输。R2 才拥有显式陆面 ET、土壤和地下水；本修订不把
缺失过程伪装成经验陆面水源。

## 2. 现状实测与因果判定

### 2.1 已发布基线

T0 后 17 粒证据 `target/natural-quality/p4/evidence.json`（BLAKE3
`01007ae263fed76c9901f6fa0ba9d7a30cc16caa30fa0ecd7411516d620c2920`）测得：

- 全球平均降水 `8.87068–11.18974 mm/day`，语料均值
  `10.29649 mm/day`；
- 原完成记录中的默认语料也已是 `6.22348–10.08944 mm/day`，故偏湿不是
  T0 地形校准单独造成；
- 数值 moisture/energy ledger 虽在容差内，却没有独立蒸发功率、TOA 短波、
  TOA 长波或 `E-P` 周期闭合量，因此不能作为地球水热收支证据。

GPCP V3.2 的全球均值为 `2.81 mm/day`。现值约为该观测的 3.2–4.0
倍；先调 P5 河道阈值会把上游偏差固化到下游经验常量，禁止这样处理。

### 2.2 生产算子探针

在提交 `d0c1eab` 上，以现有 `ProfileSurfaceBuilder`、P3、
`ClimateWorkDomainBuilder` 和 `GlobalClimateForcingBuilder` 对冻结 17 粒运行
临时 Release 探针；没有复写强迫公式。面积使用 `CubedSphereGrid` 的生产
cell area：

| 测量 | 最小 | 17 粒均值 | 最大 |
|---|---:|---:|---:|
| surface albedo | 0.0878856576 | 0.0949495016 | 0.1120839047 |
| surface moisture availability | 0.7919630433 | 0.8499526319 | 0.8801245540 |
| climate-grid land fraction | 0.1670452995 | 0.2091095841 | 0.2915302494 |

CERES EBAF Ed4 的全球 TOA incoming/reflected SW 给出观测行星反照率。用
`ATMOSPHERIC_SHORTWAVE_REFLECTANCE =
(CERES_PLANETARY_ALBEDO - measured_mean_surface_albedo) /
(1 - measured_mean_surface_albedo)` 反推唯一的固定大气散射闭合；精确值进入
`src/world/`，测试调用生产 helper，不复制公式。

这项校准只确定未解析大气的短波反射，逐格变化仍完全来自 P3 土地、海洋与
高地雪先验所产生的 `surface_albedo`；不得对输出温度或降水做直方图重映射。

### 2.3 根因

原水汽方程的五日 evaporation relaxation 与三日 condensation relaxation
都是无观测表面通量的体积源汇；upslope 项又以整层柱质量除以经验深度，远
大于 Smith raw-upslope 的 `rho_v * U dot grad(h)`。因此守账只说明“无限水源
与降水 sink 相等”，不说明水循环强度正确。

原温度目标由 authored empirical curve 构造；`monthly_insolation_fraction`
和 `surface_albedo` 不进入任何 W/m2 方程。Held–Suarez 型 Newtonian cooling
适合隔离 dynamical core 的统计测试，不足以单独证明一个带水循环产品的
地球能量预算。P4 可以继续是理想化模型，但必须公开它实际使用的短波、长波
与潜热闭合。

## 3. 时间与求解语义

### 3.1 两种时间严格分离

- `month` 是十二个日历月平均太阳几何的 `ClimateForcingPhase`，不是一次
  `MACRO_STEP_SECONDS` 之后真实流逝的一个月。
- `MACRO_STEP_SECONDS` 始终是动力/传输方程的 SI 积分步长。
- 一次 January→December 扫描名为 `formation_cycle`；它是周期解的数值
  continuation，不叫 year。
- checkpoint 记录 `completed_phase_steps`；solve report 记录
  `formation_cycles`、`continuation_steps` 和由生产步长推导的
  `integrated_model_seconds`。不得再出现 `completed_months`、
  `formation_years` 或暗示十二步等于物理年的校验。
- 公开数组仍是十二个月平均强迫相位的 climatology。P4 不发布日历日期、
  天气轨迹或季节内事件。

这项修订选择“诚实的周期定常近似”，而不是把十二个两小时步伪装成一年，
也不把真实 30 日显式天气积分塞进产品预算。完整季节瞬变和天气变率仍属于
后续能力；当前可验收的是半球相位、月场因果与周期收支。

### 3.2 收敛

每个 cycle 都独立累计最终周期的水量与 TOA 能量。发布同时要求：

- 既有 fieldwise normalized state residual 通过；
- `GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX` 通过；
- `GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2` 通过。

这些是同一周期的源汇闭合，不是把所有历史 cycle 混在一起稀释误差。达到
profile 上限仍不满足时返回 typed non-convergence；不得只因状态 norm 通过
就发布偏湿或持续增热的 climatology。

## 4. 短波、长波与反照率

### 4.1 唯一生产公式

逐格逐月 TOA incoming SW 来自 Kopp & Lean / IAU 的
`EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2` 与现有球面日平均太阳几何：

```text
SW_in = S0 * daily_mean_insolation(latitude, declination)
alpha_planet = alpha_atmosphere + (1 - alpha_atmosphere) * alpha_surface
ASR = SW_in * (1 - alpha_planet)
```

`planetary_albedo_from_surface` 与 `absorbed_shortwave_w_m2` 是 `world`/forcing
侧唯一生产 helper。测试、质量报告和 UI 全部复用，禁止第二份公式。

平衡温度来自灰体有效辐射温度加上由 CERES surface-up LW 与 ASR 推导的
`EARTH_GRAY_GREENHOUSE_OFFSET_K`，再应用既有 P3 高程环境直减率和作者
`temperature_offset`。不再保留 `32 * normalized_insolation` 与陆地冬季
经验惩罚曲线。

### 4.2 线性化长波闭合

现有各热库 toward-equilibrium 的 retained `f32` 温度 tendency 被明确解释为
灰辐射平衡附近的净外部辐射响应。每格实际保留的外部功率相加为
`Q_radiative`：

```text
OLR = ASR - Q_radiative
TOA_net = ASR - OLR = Q_radiative
```

这不是事后改温度：同一 `Q_radiative` 同时推进状态、进入 signed energy
ledger、生成公开 OLR，并接受周期 TOA 闭合门禁。任何一个路径漏项都会失败。

P4 V2 公开：

- `surface_albedo`；
- `monthly_absorbed_shortwave_w_m2`；
- `monthly_outgoing_longwave_w_m2`。

三者经保守球面投影；功率场按 extensive flux 投影，反照率按 bounded
intensive scalar 投影。

## 5. 水汽与潜热

### 5.1 饱和湿度

`saturation_specific_humidity_kg_kg` 使用 Bolton (1980) 的饱和水汽压公式和
固定 P4 lower-layer reference pressure。初始近地相对湿度使用
`REFERENCE_SURFACE_RELATIVE_HUMIDITY`，来源是 Manabe & Wetherald (1967)
的标准近地值；作者 `moisture_scale` 只缩放该初始/参考湿度并受物理饱和上限
约束，不改变水的饱和曲线。

### 5.2 蒸发

海洋及 fractional-water surface 使用 Large & Pond (1982) bulk formula：

```text
E = rho_air * BULK_MOISTURE_TRANSFER_COEFFICIENT
    * |u_lower| * max(q_sat(T_surface) - q_lower, 0)
    * water_fraction
```

本里程碑没有土壤水，因此 full-land evaporation 必须为零。删除原
`1 - 0.72 land + ...` 经验水分可用度；不得在 R1 假造 ET。R2 以显式土壤、
地下水和雪冰储量添加陆面源。

不加无出处的 minimum wind 或 gustiness 常量。低风不确定性通过质量证据
暴露；若观测包络不通过，先查风场或缺失边界层过程，不放大 transfer
coefficient。

### 5.3 凝结、地形雨与潜热

- grid-box supersaturation 用 saturation adjustment 在当前物理 step 内移除；
- 地形源改为 Smith (1979)、Smith & Barstad (2004) 的 raw upslope source
  `rho_air * q * max(u dot grad(h), 0)`；
- 删除 `OROGRAPHIC_CONDENSATION_DEPTH_M` 与
  `OROGRAPHIC_UPLIFT_MAX_M_S`；水量 availability limiter 仍是最终上限；
- 同一 retained evaporation/condensation mass flux 分别从 surface thermal
  reservoir 吸收潜热、向 lower atmosphere 释放潜热；
- total energy 同时包含 temperature sensible energy 与 atmospheric vapor
  latent energy，因而相变为内部交换，不会伪装成 TOA source。

P4 V2 公开 `monthly_evaporation_mm_day`；同一数组进入水量预算、保守反投影、
质量测量和 UI。

## 6. 确定性 continuation warm start

增加 crate-private `GlobalCirculationContinuation`：只包含 validated work-grid
terminal state、grid/model/equation identity 和来源 forcing fingerprint。

- 同 forcing 可精确复用；不同 P5 terrain forcing 只允许在 grid、profile、
  integrator、quantization 和 equation model 全匹配时作为 initial guess。
- forcing fingerprint 不匹配不能冒充 checkpoint resume；solve report 明确
  `warm_started`，新 checkpoint 始终绑定新 forcing。
- warm path 与 cold path调用同一个 generator loop、同一个 tendency 和同一组
  hard gates；禁止 preview 方程、降低 resolution、少算月份或放宽 residual。
- 不兼容 continuation 返回 typed rejection；P5 可安全冷启动，但不能静默
  接受错模型状态。
- P5 在第一次 candidate climate solve 后保留 continuation，后续外循环传回；
  它不跨 build、不卡入 engine cache、不中途发布。

同输入重复运行必须逐位相同。warm/cold 不要求因停止路径不同而逐位相同，
但必须通过同一物理门禁、同一 morphology gates 和锁定的 field agreement
语料；warm start 不能改变可用能力或月场定义。

## 7. 质量、证据与 UI

### 7.1 生产质量报告

在既有 P4 指标中按名字绑定边界，并以字母序加入以下 measurement；除闭合
项外均为 unbounded，避免把 Earth 参考强加给玩家参数：

- absorbed-shortwave-global-mean-w-m2；
- evaporation-global-mean-mm-day；
- evaporation-precipitation-relative-imbalance（hard）；
- outgoing-longwave-global-mean-w-m2；
- planetary-albedo-global-mean；
- precipitation-global-mean-mm-day；
- precipitation-low-to-high-latitude-ratio；
- precipitation-seasonal-hemisphere-phase-fraction；
- toa-net-radiation-global-mean-w-m2（hard）。

`EXPECTED_METRIC_NAMES` 保持字母序；边界由 metric name 查表，禁止再次用数组
位置偶合。

### 7.2 Earth-default 独立证据

冻结 17 粒默认 Earth-like corpus 报告观测偏差，不用 seed 自身分布制定门槛：

- GPCP V3.2 `EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY`；
- CERES EBAF Ed4 incoming/reflected SW、OLR、TOA net；
- Stephens et al. (2012) / Wild et al. (2015) latent heat flux range；
- 低纬降水应强于高纬，且默认轴倾下两个半球的雨带季节响应符号正确。

降水证据以 GPCP V3.2 均值为中心，并使用 Adler et al. (2017) 从多产品离散度
估计的 `±7%` 全球均值误差作为
`EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE`。这是默认 Earth-like
语料的证据包络，不是任意玩家世界的生产 hard gate；其他 authored climate
values 也只显示相对 Earth reference 的偏差，不钳制。

合成守卫必须人工注入 water/energy Fail，不能依赖某个 seed 恰好失败。

### 7.3 UI

形成链地图与球面视图新增可选字段：

- 年蒸发量；
- 年均吸收短波；
- 年均出射长波；
- 表面反照率。

左侧形成摘要新增“P4 水热预算”，直接读取 formation product 自己的 final
climate `ClimateBudgetReport`，显示全球降水、蒸发、`E-P`、ASR、OLR、TOA
net、行星反照率与 warm/cold solve。Earth reference 数值来自 `world` 常量；
标签来自本地化/字段注册表，不在 `app` 重写第二份事实。

## 8. Schema、身份与性能

- `GLOBAL_CIRCULATION_SCHEMA_V2`、`CLIMATE_CHECKPOINT_SCHEMA_V2`；V1 frozen
  artifact 不做猜测迁移。
- global-circulation stage identity 升版；equation model fingerprint 覆盖全部
  新物理常量、公式 semantic ID、时间语义和公开字段。
- P4 输出变化必然刷新 P4、P5、T1 及其下游身份；P0–P3 必须保持不变。
- 只刷新实际变化的 fingerprint/golden。修订完成记录必须列 old/new 与原因，
  包括 natural field registry hash、sampled IDs 和 16 幅 GPU 金样是否变化。
- continuation state 计入 mechanically derived dense-owner inventory；High 仍受
  既有内存上限。
- P4 Draft/Standard/High 独立预算不放宽；P5 Standard 必须通过既有端到端
  wall-clock gate。性能证据同时记录 cold P4、changed-forcing warm P4 与完整
  P5。

## 9. 验收

自动验收：

1. 生产 helper 的解析极限：零风/饱和空气零蒸发、升温提高 `q_sat`、平地零
   orographic source、过饱和精确受限、相变 moist-energy 闭合。
2. albedo counterfactual 改变 ASR、equilibrium temperature 和最终气候；改
   moisture 不得改变太阳功率公式。
3. final cycle `E-P`、TOA net、数值 mass/moisture/energy ledger 同时通过。
4. 默认 17 粒通过独立 Earth water/energy evidence；重复生成逐位相同。
5. cold 与 compatible warm 通过同一方程 agreement；错 grid/model 状态被拒。
6. P5 Standard performance 通过；取消延迟与 High memory 不回退。
7. P0–P3 指纹不动；受影响下游清单完整。

用户 UI 验收：

1. 启动应用，生成“形成链”世界；在字段列表依次选择年降水、年蒸发、吸收
   短波、出射长波和表面反照率。
2. 地图与球面都应显示相同权威场；高反照率高地吸收短波更低，蒸发只从
   水面/fractional-water cell 起源，陆地降水来自输送与地形抬升。
3. 查看左侧“P4 水热预算”：默认世界全球降水应接近 GPCP reference，蒸发
   与降水闭合，ASR/OLR 接近且 TOA net 在门禁内。
4. 以 Standard 重建；预期 P5 不再因四次 P4 冷启动超过预算，界面仍只原子
   替换完整世界。

## 10. 每项承重技术的出处

- Held, I. M. & Suarez, M. J. (1994), *A Proposal for the
  Intercomparison of the Dynamical Cores of Atmospheric General Circulation
  Models*, DOI `10.1175/1520-0477(1994)075<1825:APFTIO>2.0.CO;2`：说明
  Newtonian relaxation 是 dynamical-core benchmark，而非完整水热预算。
- Frierson, D. M. W., Held, I. M. & Zurita-Gotor, P. (2006), *A
  Gray-Radiation Aquaplanet Moist GCM. Part I*, DOI `10.1175/JAS3753.1`：
  灰辐射、mixed-layer lower boundary、显式水汽与 large-scale condensation
  的理想化湿 GCM 先例。
- Kopp, G. & Lean, J. L. (2011), SORCE/TIM total solar irradiance，DOI
  `10.1029/2010GL045777`；IAU 2015 Resolution B3：太阳辐照度事实源。
- Payne (1972), DOI
  `10.1175/1520-0469(1972)029<0959:AOTSS>2.0.CO;2`：开阔海面短波反照率；
  Masson et al. (2003), DOI
  `10.1175/1520-0442(2003)016<1261:AGDOLS>2.0.CO;2`：业务陆面方案中的
  植被反照率量级；Moody et al. (2007), DOI `10.1016/j.rse.2007.07.002`：
  MODIS 积雪覆盖生态系统反照率。P4 的静态高地亮化仍只是无雪量状态下的
  冻结 V1 几何先验，不冒充物理雪线。
- U.S. Standard Atmosphere 1976：P4 理想化下边界使用的标准对流层环境直减率；
  它不是逐格湿绝热线率。
- Loeb et al. (2018), CERES EBAF TOA Ed4，DOI
  `10.1175/JCLI-D-17-0208.1`；Kato et al. (2018), CERES EBAF Surface
  Ed4，DOI `10.1175/JCLI-D-17-0523.1`：TOA SW/LW/net、surface-up LW 与
  反照率校准。
- Stephens et al. (2012), *An update on Earth's energy balance in light
  of the latest global observations*, DOI `10.1038/ngeo1580`；Wild et al.
  (2015), DOI `10.1007/s00382-014-2430-z`：全球能量与潜热通量约束。
- Huffman et al. (2023), GPCP V3.2，DOI `10.1175/JCLI-D-23-0123.1`：
  `2.81 mm/day` 全球降水 reference；Adler et al. (2017), DOI
  `10.1007/s10712-017-9416-4`：跨产品离散度给出的全球均值 `±7%` 证据包络；
  Hersbach et al. (2020), DOI `10.1002/qj.3803`：独立 `P/E` 水量平衡审计先例。
- Bolton, D. (1980), DOI
  `10.1175/1520-0493(1980)108<1046:TCOEPT>2.0.CO;2`：饱和水汽压与湿度
  换算。
- Manabe, S. & Wetherald, R. T. (1967), DOI
  `10.1175/1520-0469(1967)024<0241:TEOTAW>2.0.CO;2`：标准近地相对湿度
  初值及潜热能量项。
- Large, W. G. & Pond, S. (1982), DOI
  `10.1175/1520-0485(1982)012<0464:SALHFM>2.0.CO;2`：海气 bulk latent
  heat / moisture flux 与 neutral transfer coefficient。
- Smith (1979) raw upslope model；Smith & Barstad (2004) linear
  orographic precipitation，及 Barstad & Smith (2005), DOI
  `10.1175/JHM-404.1`：`rho_v U dot grad(h)` 地形凝结源与后续搬运/降落实践。
- Hu, Adams & Shu (2013), *Positivity-preserving method for high-order
  conservative schemes solving compressible Euler equations*, DOI
  `10.1016/j.jcp.2013.01.024`：检测会越过物理可行域的守恒通量并以共同系数
  缩放；P4 将同一原则窄化到两个 ocean reservoir 的内部成对热通量。
- CESM 1.2 User Guide 的 *Restarting a run* 与 *History and Restart Files*：
  continuation restart 保存完整状态并以 bit-for-bit exact test 验证；Sekai 既有
  V1 transient strict warm-start identity：continuation 只换初值、方程与验收
  不变。P5 的 changed-forcing initial guess 明确不是 exact restart，故另做
  forcing identity 与 cold/warm agreement 验收。

## 11. 修订记录

- R1（2026-08-23）：冻结以上水热预算、时间语义、continuation、UI 与证据
  边界。它显式替代旧 P4 规格中“一个 7,200 s step 即一个月 endpoint”可被
  称为 formation year、原经验 moisture relaxation 可代表物理水循环、以及
  原 energy ledger 足以证明地球能量预算的表述。
- R2（2026-08-23）：Task 2 的真实生成 RED 暴露两条原设计未写出的必要
  可行域约束。其一，局地 Newtonian 正加热不得超过同格同月 ASR，否则
  `OLR = ASR - Q_radiative` 会变负；`PlanetForcing` 因而必须携带经生产 helper
  计算的月 ASR，方程在状态推进前等比例限制正加热、保留负冷却，并以有界
  f32 ULP 投影保证同一 retained tendency 同时进入状态、数值账本与公开 OLR。
  这不是输出钳制。其二，压力梯度必须来自当前解析温度，辐射平衡目标只经
  热 tendency 作用；否则极夜目标会绕过热容直接驱动风场。周期求解初值改为
  十二 forcing phase 的逐格年均值，避免任意 January 相位获得动力学先手；
  后续每月方程、时间步与收敛门槛不变。以上修订不增加用户旋钮或经验增益，
  equation fingerprint 显式升级。
- R3（2026-08-23）：Task 3 的守恒 RED 与冷启动实测进一步修正以下实现
  细节；本条显式替代 R2 中“每个热 reservoir 各自施加外部辐射”的可误读
  表述。

  1. TOA 只有一个下边界外部功率：同格 land/water fraction 将同一份
     `ASR - OLR` 分给 lower atmosphere 与 mixed layer；upper atmosphere、
     thermocline 与 deep reservoir 只接受内部热交换。`OLR` 由灰体平衡点
     的 Stefan–Boltzmann 一阶响应计算并保持非负，故外部功率不会按活动层数
     重复计入。数值、水量与 TOA 账本每个 formation cycle 独立清零，只有
     最终完整 cycle 可进入公开报告。
  2. 原线性扰动连续方程 `-H div(u)` 在长积分中会让上层厚度穿过零。生产
     full/fast 核改为有限体积 donor-upwind 的 `-div((H + eta) u)`，逐边使用
     同一法向体积通量作等量反号更新；IMEX 的线性隐式参考仍保留
     `-H div(u)`，非线性差额属于显式项。该修订同时保证封闭球面层质量守恒
     与正厚度，不做事后 clipping。
  3. 年均冷启动不再直接平均十二个月的非线性 `q_sat`，而是先从各月 forcing
     恢复平均相对湿度，再在年均 lower-air temperature 上重建湿度，避免
     Jensen 偏差制造首周期假降水。粗网格 lower condensation 使用 SPEEDY
     的 `0.9` 相对湿度阈值与四小时解析松弛，物理过饱和仍在当前 step 内精确
     移除；C2 lower/upper 水汽交换之后对 upper layer 再做同一饱和调整，降水
     和潜热分别进入同一 retained mass/energy tendency，禁止上层无降水储存
     过饱和水汽。
  4. 严格 `E-P` 门禁揭示：以冻结单一终端风场连续推进几十个热力周期会让
     温度梯度与风场失配，随后完整耦合 cycle 产生伪降水尖峰。最终实现改为
     bounded thermodynamic-moisture shooting preconditioner：在最近一次完整
     cycle 的十二月速度轨迹上，复用生产 tendency 回放温度与水汽，以当前
     状态和干燥/饱和边界括住一维湿度初值，并以 Ridders (1979) 的括区间
     exponential interpolation 求解精确周期根 `E-P=0`。已经进入公开闭合集合
     的探针直接接受，避免无意义地继续扰动已可发布的 tracer；只有尚未闭合时
     才求数学零点。内部停止条件直接复用公开
     `GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX`，探针上限由被插值
     `f32` 状态的 `MANTISSA_DIGITS` 机械推导，不另设经验目标、容差或次数。它在
     C1 状态残差不大于同为无量纲的公开水量闭合界后启用；C2 在公开状态
     残差通过，且 TOA 已闭合或已进入 profile horizon 的后半段时启用。前半段
     只形成 C2 动力/辐射背景，后半段给 block moisture iteration 留出多次
     “修正→完整耦合验证”机会。seed 11 实测 TOA 从 `13.41090` 单调降至
     `10.57268 W/m2`，单纯以 TOA 作前置条件会令其在 Draft horizon 内一次湿度
     修正也不能执行；从首个状态门槛就修正则使 seed 42 局地空气达到
     `80.23986 C` 并被 wire 验证拒绝。两段边界由 profile cycle horizon 的整数
     中点机械推导，不新增经验阈值。若下一完整 cycle 仍未闭合，可在 profile
     既有最大 cycle 内重复同一有界修正。二者都只选择下一 cycle 的初值，
     不发布探针状态、不改变方程，也不计作物理 `continuation_steps`。
     下一次正常 `SplitExplicitRk3V1` 完整 cycle 仍须独立
     通过 state、`E-P`、TOA 和数值守恒全部门禁，否则不得发布。
  5. 身份相应升级：equation fingerprint domain 为 V5，覆盖非线性层连续性、
     单下边界辐射、Bolton/Large–Pond/Smith/SPEEDY 相变、上下层凝结与全部新
     常量；global model domain 为 V4，覆盖相对湿度年均初始化与 shooting
     参数；公开 state field fingerprint 为 V3，新增逐月蒸发字段。P0–P3
     不在影响链；P4 及把 P4 纳入身份的 P5/T1 下游旧→新清单留待 Task 7
     以实测哈希登记。
  6. 数值预条件不新增地学系数：括区间指数插值沿用 Ridders (1979),
     *A New Algorithm for Computing a Single Root of a Real Continuous Function*,
     DOI `10.1109/TCS.1979.1084580`；退化步回到已经计算的二分中点。最大探针数
     来自 IEEE 754 binary32 有效位数。最终接受仍完全由公开物理门禁决定。

     固定 Draft/seed 42 生产探针给出的选择证据如下；数值只记录实测，不进入
     公式或门禁。C1 若在首个公开状态门槛（cycle 1）就求零，随后水量残差回到
     `0.55783`；等状态残差降到公开水量界再作周期求根，cycle 7 的水量残差为
     `0.023159`、TOA net 为 `9.99403 W/m2`。C2 在 state+TOA 门禁首次同时通过时
     作同一操作，cycle 5 水量残差为 `0.036307`、TOA net 为
     `6.29858 W/m2`。这组结果
     解释 profile 启用条件；Task 4 的 17 粒语料仍负责检验可迁移性，不能由本
     单种子代替。
  7. 出处审计移除旧 shared thermodynamics 的 `0.2` specific-humidity 魔法上限：
     上限改为 specific humidity 质量分数定义直接给出的 `1`；月蒸发/降水 wire
     只要求非负有限，最大值由其 `f32` 存储类型机械给出，不再以无出处的
     `1000 mm/day` 钳制玩家世界。默认 Earth-like 结果没有触碰这些结构边界。
  8. Debug 取消同步测试删除无产品语义的 `120 s` phase-arrival 看门狗。该固定
     墙钟既不是 P4 性能门禁，也会在零容量 channel 超时后让 scoped worker
     死锁；测试改用非阻塞发送并等待“目标 phase 或 sender 结束”，仍严格保留请求取消
     后 `250 ms` 的既有产品延迟门禁。Release C1/C2/High 性能由独立 evidence
     测试负责。
  9. 为守住冻结的性能门禁而作的数值路径收敛不改变物理方程：forcing 构造时
     已验证并缓存的地形梯度成为唯一生产输入；同一次 macro step 的 declared
     tendency 同时供预算与积分器消费；split/RK3 工作区、首级导数和状态缓冲复用；
     标量梯度与 donor-upwind 厚度通量合并为同一次规范边遍历并以逐位等价测试
     守门；RK 中间态只作 `v - (v·n)n` 的 `f64` 正交投影，最终公开态仍走带 ULP
     修正的严格切向投影。公开构造器仍完整验证外部输入，只有 crate-private 的
     已验证 forcing 路径跳过循环内重复验证。
  10. dense-owner 公式按实际所有权修订：投影 `Vec` 被 `Monthly*Field` 接管后
      直接移动进 `GlobalCirculationFields`，不产生第二套 27 分量月场，因此发布
      峰值只有一个 output owner；真实隔离 RSS 门禁继续独立约束实现。最终 Release
      实测：Draft/C1 `6.175 s`，Standard/C2 `22.217 s`，High/C2 `72.160 s`；High
      独立子进程 `73.890 s`、增量 RSS `364,756,992 B`，机械 dense-owner 估算
      `280,811,216 B`，均低于规格冻结上限；High 取消延迟 `1.574 ms`。这些数值
      仅为证据，不进入算法、阈值或 profile 选择。
  11. 全 crate RED 揭示旧 C2 内部热交换把“参考层结”错当成“待消除异常”：状态
      初始化按 `UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C`、
      `THERMOCLINE_EQUILIBRIUM_OFFSET_C` 与
      `DEEP_OCEAN_EQUILIBRIUM_OFFSET_C` 构造有序冷层，旧项却直接扩散原始温度，
      因而持续从唯一辐射下边界抽热；延长到公开最大 cycle 时 TOA 反而由
      `9.229 W/m2` 增至 `13.467 W/m2`，排除了“少积分几轮”的解释。修订由同一
      `role_reference_temperature_c` 为状态初始化和 tendency 逐格逐月给出参考温度；
      它同时包含 forcing 相位、固定层间 offset 与已声明 ocean clamp。内部热通量
      改为 `C_min ((T_k-T_k_ref)-(T_l-T_l_ref))/tau`，仍由同一
      `paired_heat_exchange` 作等量反号 extensive 更新。两段海洋内部 exchange
      同时改为按权威 `1-land_fraction` 缩放，全陆地严格为零。以上不新增常量、
      旋钮、外部热源或事后温度修正，并由“完整参考层结严格零热通量”和“C2 海洋
      内部交换均为 water-only”解析测试守门。原 changed-terrain 回归与固定 C2
      生产例均通过既有水量、TOA 和温度 wire 门禁，无需放宽阈值。
  12. High RED 进一步区分了重映射误差与源状态越界：fractional-water cell 在
      climate work grid 已出现 `-5.000025 C`，故不得在发布层钳值。海洋内部成对
      热交换现按 Hu, Adams & Shu (2013) 的 conservative positivity-preserving
      flux-limiter 原则，在一个 production step 会穿过
      `SUBSURFACE_OCEAN_MIN_C` 时求唯一 `alpha in [0,1]`；同一 `alpha` 同乘交换
      两端，因而只删除不可行的内部传热部分，不产生或销毁热量。解析测试覆盖
      边界零通量与部分缩放；High 实测最低温精确为既有下限，wire、热预算和性能
      门禁同时通过。该修订复用既有物理状态边界与步长，不新增容差或地学参数。
