# 目标尺度下的物理近似与全链优化设计

日期：2026-10-14。
状态：**首轮代码、离线验证、两档原生发布与取消应用层契约通过；窗口操作复核受工具连接故障限制，见 §10.8。**
规则事实源：`../../../AGENTS.md` 的“产品边界：最终态与科学近似”修订。

## 1. 用户意图与成功标准

用户要求：保持整体物理正确，停止追求极致物理正确性，只在一定分辨率下正确，
并对算法进行全方位优化。首轮范围已明确为 **Draft + Standard**。

产品成功是这两档最终地图的必要物理关系、可见质量与操作性成立，同时生成、
修改后重建和发布尽可能快。每项新增精度都必须说明最终地图或下游算法的消费者。
复杂模型的历史投入、原方程组、完整演化轨迹及旧输出的逐位相同不构成保留理由。

High 不进入本轮模型质量与性能扩展语料；既有 High 行为不得静默切换为未经验证的
新模型。新路径进入生产时，必须明确其档位支持，不能用高显示分辨率暗示高物理精度。
过渡期 High 沿用原路径或显式标记新模型尚不支持的具体方案，在接入规格中确定。

旧时延预算保留为历史基线，不能作为“已经够快”的停止条件。本轮不预先钉一个
没有测量依据的新秒数，也不承诺达到 Civ Atlas 的时延。

## 2. 证据与现状

当前 Sekai 基线为 `51a9b36dbb84da1ae267aadcfd300a3a5ed944d0`。
2026-10-14 同机、seed 42、Release、无 RUSTFLAGS 的串行测量已完成：

- Draft 原生请求至 GPU 发布三次中位 32.43 s；这是端到端基线，含显示准备。
- 独立阶段探针中，起点气候约占 46%，地貌与终点气候合计约占 37%；不能把后者
  全部归为气候，也不能把该单次探针与三次 UI 中位简单相减来分摊成本。
- Civ Atlas 默认首图约 3.23 s，但其计算内容、格元数和计时终点不同，仅证明
  同机产品等待时间存在大幅差距，不证明同精度下的速度上限。
- Civ Atlas 的受控地形测试确认了定性雨影，同时发现显著网格敏感性及缺少物理
  水量账本；它提供低成本制图先例，不能直接作为物理真值或全盘移植目标。

原始证据位置：`D:/hemifuture-probes/civ-sekai-20261014/summary.json`、
`sekai-stage-draft.log`、`climate-correctness/results.json`。
这些是本机研发证据，不属于产品缓存或世界 schema。已有证据直接复用，不为本
文档重复运行相同基准；源代码或测试边界变更后才更新对应基线。

当前档位定义在 `src/world/natural/profile.rs`：
`authoritative_target_cell_count`、`tectonic_control_target_cell_count`、
`climate_face_resolution` 是现有分辨率事实源，本设计不复制其数值。
气候已有独立工作域及双向保守映射，并非需要从零引入多分辨率架构。

已读 A6、A7、A8、A8b 的相关结论。A8 已处理多处重复校验、哈希缓冲和边通量复用；
A8b 已处理终点初猜校正复用。不得把它们重新包装成尚未实现的优化。
A8b §12 还记录了旧模型季节、接缝和雨量包络的遗留问题，故旧模型只能作为对照，
不能要求新模型逐项复刻旧误差。

## 3. 正确性的边界

### 3.1 保留的契约

- 有出处的整体机制、单位与因果关系；水、质量以及模型声明的能量交换应在
  有依据的离散容差内闭合。守恒不等于要求所有浮点残差严格为零。
- 有限、合法领域、非负储库、合理终态；模型不得用裁剪或末端重映射藏匿错误。
- 河流、湖泊、海岸、侵蚀与气候使用一致的最终状态，身份与版本可追踪。
- 成功原子发布；取消和失败保留旧有效世界，支持域外不发布伪有效产物。

这些要求约束过程的结果与账本，不强制保留具体层数、短时波动、细节过程或积分器。

### 3.2 可简化的内容

允许按目标尺度采用降阶模型、诊断式平衡、有依据的参数化、较少垂向层、固定
物理工作域、有界迭代和较低耦合频率。可改变旧方程组和离散策略，接受符合
目标尺度质量预算的近似误差。

不再把任意网格一致性、无限加密后的收敛、旧逐位金样或高成本完整轨迹作为
新模型的默认验收条件。某一模型只在固定尺度有效并不构成缺陷；必须清楚声明
该尺度，且 Draft 与 Standard 若共用该工作尺度，映射后的下游结果也须有效。

### 3.3 工作尺度与显示尺度

先为每项发布字段列出现实消费者和最小必要尺度，再决定工作域。优先评估
Draft 与 Standard 共用气候工作尺度的可能性；权威地形与显示细分保留自身职责。
共用的具体工作网格尚未选定，不把当前档位大小直接减半作为结论。

连续显示插值不产生新的科学信息；涉及水量、通量或面积汇总的重映射仍消费
生产守恒助手。输出更细不意味着重新执行更贵的全球气候。

## 4. 路线选择

### A. 目标尺度的简化物理模型（主研究方向）

以低维水热收支和诊断式环流替代对最终地图无明显收益的快速动力演化，按真实
需求保留地形降水、海陆差异和季节性。低复杂度能量平衡与水循环模型提供一手
依据 [S1–S3]；不得直接拷贝其中依赖地球观测风场或事后校正的部分到未知世界。

先核清发布字段消费者：`spherical_formation_display.rs` 消费月气候并生成年均
温度、降水、风及水热收支显示；P5 还有月尺度陆面水量消费者。不能仅因 UI 展示
年均量，就以年均降水填充每个月。诊断式季节场可以成为候选，无须因此保留完整
短时动力轨迹。

这一方向允许最大的计算量削减，但具体方程、背景风与海洋热输送闭合仍需窄探针
选型。本草案批准的是选型范围，不宣称某篇论文已能直接拼成完整的 Sekai 气候。

### B. 原模型的工作域、耦合与求解简化（低迁移成本对照）

保留当前机制，只测成本较低的工作尺度与必要耦合频率，判断能否更便宜地满足同一
目标尺度契约。已有求解器与快照可复用，迁移成本较低；受复杂模型本身成本限制。
不得重新启用已有失败记录的热启动，也不得只删迭代或提高 CFL 而忽略稳定域。

### C. 逐位内核优化（配套工作）

删除重复重建、减少分配与内存流量、改善真实热点的复杂度；只有测量支持才增加
并行或 SIMD。独立安全且有端到端收益的改动可单独交付，但不能把它作为替代
模型简化的唯一途径。已实现优化和历史负收益方案不重复试验，除非前提发生变化。

最终以支持域内的质量、成本和实现复杂度选最小方案。不会为了备选而永久新增
多套用户模式、通用求解器框架或无人使用的公共 API。

## 5. 全链审计与改动顺序

全面指覆盖所有成本来源；执行按实测收益排序，不同时重写整个项目。

1. **消费者与门禁审计。** 梳理气候/地貌真实消费者，把现有门禁分为最终态硬
   不变式、目标尺度质量、旧模型内部精度。保留第一类，评估后两类的适用性。
   旧残差或旧金样不能自动约束新模型；取消某门必须指出其原消费者及替代证据。
2. **P4 与 P5。** 先比较 §4 的简化路径，分开记录起点气候、地貌、终点气候，
   检查下游河网、径流、侵蚀和水库存。耦合终点要自洽，但不要求完整历史轨迹精确。
3. **P1–P3 与映射。** 检查 `formation/graph.rs` 与 `climate_work_domain.rs`
   中控制表面、几何和映射的构建及复用边界。只移除实际仍存在的重复工作。
4. **引擎与内存。** 复核 A8 之后剩余的序列化、哈希、校验、拷贝和布局成本。
   同一不可变对象可在信任边界验证一次；边界验证与廉价身份检查仍保留。
5. **显示与 GPU 发布。** 从 `run_spherical_world_build`、
   `assemble_spherical_candidate` 到细分、上传、可交互发布分别测量。只减求解器
   秒数却不改善实际等待时间的方案必须解释其消费者收益。

既有“不保存世界快照、重开现算”的用户决定保持有效。可以复用经过身份验证的
当前进程内数据，不能以性能之名恢复磁盘世界缓存或隐式保存历史世界。

## 6. 证据与验收

### 6.1 选型证据

单元或受控夹具先证明离散守恒、单位、海陆边界和因果响应；只对跨域或统计性质
增加代表案例。原模型偏差对照只是证据之一，解析约束与目标尺度质量更优先。
观测只约束可类比的地球参数和尺度，不强迫玩家世界长得像地球。

首轮真实链路采用已有 Draft / Standard、seed 42 基线；气候候选的机制风险若需
补充干湿、海陆或地形情况，优先受控输入，新增完整世界必须写明其独立覆盖价值。
对新工作尺度只验证声明支持范围，不为不使用的任意分辨率建立加密扫描。

记录最终温度/雨量场与下游径流、河网和地形变化，水热/质量账本、非有限/非法值、
耗时、峰值内存及取消/失败结果。误差归一方式按字段消费者确定；不先自定统一
百分比，不把“肉眼类似”作为全部证据。

### 6.2 性能证据

Release、同机、串行、无 RUSTFLAGS。复用已有预热与成对比较方法，报告中位及
离散范围；信号接近噪声时才增加重复。阶段与端到端、冷构建与内存复用分别报告。
模型改变后可能迁移瓶颈，因此每一轮按新剖面重新排序，而非机械完成所有候选。
性能近似收益先实测，不把文献复杂度或别的产品耗时当作本项目承诺 [S6]。

### 6.3 接入 UI

候选必须通过现有字段文档接入实际生成链，更新模型身份与必要 schema；不伪造
已经取消的垂向层、物理量或诊断来适配旧 UI。若需改变字段支持能力，在该候选
接入规格中列出具体消费者和迁移，不默认扩大所有公共接口。

用户验收步骤：启动测试构建，在原有生成设置中分别选择 Draft / Standard，使用
同一输入配方生成；查看高程、温度、降水、风、水系和水热收支，执行一次参数
修改及取消再生成。预期发布完整有效的新地图、必要地理关系成立、等待缩短，
取消不破坏旧地图。用户上手验收通过前，不宣称算法交付完成。

## 7. 历史规格的适用修订

本节登记本轮设计与历史约束的关系，不改写已完结实验记录：

- A1/A6 中“方程、离散、分裂结构不变”仅约束其历史优化任务，不再限制新路线。
- A8 的逐位等价要求仅适用于其已声明的等价重构，不适用于经过版本化的模型替换。
- 旧气候内部层数、积分步数、全态残差和参考解吻合度需要按消费者重新分类，
  不得整体带入新模型；最终态物理与身份契约仍有效。
- 自然分辨率剖面与旧工作域身份目前仍是运行时契约。未实施新的、有明确版本的
  分辨率设计前，不直接改常量、伪造指纹或把新结果装进旧身份。
- Phase A 作者闭环仍是独立工作，已有未提交文档不在本轮编辑范围；其“不存
  世界快照”决定保持，性能预算按本次方向继续改进。

## 8. 尚未冻结的技术选择

以下是选型研究的输出，不阻止方向确定，也不允许以未测数值开始生产替换：

- Draft / Standard 的实际物理工作尺度及是否共享工作域。
- 简化环流、季节场、海洋热输送与地形降水的具体组合和适用范围；论文依赖的
  地球边界资料如何由任意世界输入产生，尚无直接答案的部分保留为开放技术问题。
- 各最终字段的误差预算、既有模型专属门禁的替代，以及可交互发布的具体时延目标。

下一阶段先形成可复查的消费者/成本清单与窄候选探针；得到证据后显式修订本规格，
再锁定生产接入。不会把尚未选定的组合提前命名为“已批准的新求解器”。

## 9. 每项承重技术的出处

- **S1：低维能量平衡。** Ziegler & Rehfeld (2021),
  [TransEBM v1.0](https://gmd.copernicus.org/articles/14/2843/2021/gmd-14-2843-2021.html)。
  支持通过参数化热输送降低模型复杂度；不提供完整降水或任意世界环流，不能
  据此认定候选已闭合或直接引用其运行速度。
- **S2：简化水循环。** Stassen, Dommenget & Loveday (2019),
  [GREB hydrological cycle](https://gmd.copernicus.org/articles/12/425/2019/gmd-12-425-2019.html)
  及[勘误](https://gmd.copernicus.org/articles/12/425/2019/gmd-12-425-2019-corrigendum.pdf)。
  支持低复杂度蒸发、降水与水汽输送路线；其预设气候场、观测拟合和修正项限制
  了对任意世界的直接移植，不能用未知来源强迫或修正项掩盖我们的账本错误。
- **S3：地形降水近似。** Smith & Barstad (2004),
  [A Linear Theory of Orographic Precipitation](https://training.weather.gov/wdtd/courses/woc/winter/physiogeographic/cool-season-orographic/story_content/external_files/Smith%20and%20Barstad%202004.pdf)。
  包含输送、凝结物转化/降落和背风蒸发；有线性及背景流假设，不是完整全球模型。
- **S4：离散守恒与配对通量。** LeVeque (2002),
  [Finite Volume Methods for Hyperbolic Problems](https://www.cambridge.org/core/books/finite-volume-methods-for-hyperbolic-problems/97D5D1ACB1926DA1D4D52EAD6909E2B9)。
  复用生产通量和账本结构，低阶离散与守恒可以并存；不要求采用最高阶方案。
- **S5：不同工作域间的守恒映射。** Jones (1999),
  *First- and Second-Order Conservative Remapping Schemes for Grids in Spherical Coordinates*；
  [SCRIP 作者项目实现](https://github.com/SCRIP-Project/SCRIP)。
  Sekai 既有实现事实源为 `generators/spatial/conservative_remap.rs`，本轮优先复用。
- **S6：按整体成本优化。** Amdahl (1967),
  [Validity of the Single Processor Approach to Achieving Large Scale Computing Capabilities](https://www.cs.cmu.edu/~18742/papers/Amdahl1967.pdf)。
  用端到端成本评估局部提速，不能把局部倍数等同于产品倍数。
- **S7：本项目实现与失败先例。** A1、A6、A7、A8、A8b 的冻结记录；源码
  `profile.rs`、`climate_work_domain.rs`、`causal.rs`、
  `surface_formation/generation.rs`、`spherical_formation_display.rs` 与
  `spherical_presentation.rs`。历史失败必须作为候选前提，旧门槛不能自动作为
  新模型的科学依据。
- **S8：最低证据和产品边界。** `AGENTS.md` 的出处、测试范围及 UI 验收纪律；
  本次用户关于目标尺度正确性、速度和 Draft + Standard 范围的明确裁定。

## 10. 实施修订：共同气候工作域与周期内工作区（2026-10-14）

本修订落实首轮路线 B/C。用户已批准总体方向并明确允许牛顿/拟牛顿类求近似解；
具体算法仍按成本选型，未因方法名称引入新的通用求解框架。

### 10.1 已测候选与选择

生产起点 P4 → P5 → 终点 P4，seed 42，两档各一条完整链；这是工作尺度影响最终
气候、河网和地表反馈的最小跨层证据。全部复用生产算子和质量评价器。

- 粗 FAS 预算从 48 改为 12：两档均能发布，但风场相对 RMS 差约 21% / 34%，
  探索计时收益有限且 Draft 未显示收益。保留 `GLOBAL_CIRCULATION_MECHANICAL_COARSE_CYCLES`。
  同理，本轮不为加速这个小成本块新增 Anderson；Walker–Ni 仍只是后续候选。
- Draft 面分辨率 16：新增接缝、西风带与迎风降水指标失败，拒绝进入生产。
- Standard 面分辨率 24：无新增质量失败，可行对照。
- Draft / Standard 共同面分辨率 20：均通过所有既有生产硬不变式；现有质量指标
  均仅剩基线同样失败的季节相位项，选为本轮候选。它是失败的较低尺度与原 Draft
  尺度之间、可整除既有粗化比的中间工作域，来源为本节先测后钉的比较，非物理常量。
  生产数值唯一事实源为 `NaturalQualityProfile::climate_face_resolution`；High 保持原工作域。

离线原始 JSON、计时日志与 SHA-256 在
`D:/hemifuture-probes/civ-sekai-20261014/a9/`；`baseline`、`smaller-grid`、`common20`
为实际运行标签。粗预算候选是 `c12`。探索计时受机器漂移影响，不作为正式加速比。

### 10.2 最终态误差及限制

共同工作域对旧工作域的逐点差并不小：Draft / Standard 近地风分量 RMS 差分别约
3.39 / 3.34 m/s，温度约 1.39 / 1.76 °C，降水约 3.10 / 3.32 mm/day。
这些记录声明地图会变化，**不宣称与旧路径等精度或逐点等价**。依 §3，较细旧工作域
不是物理真值；本轮以已有机制/守恒、支持尺度质量和用户最终地图验收判断候选。
风雨差异及其下游影响必须在 UI 查看，不能仅凭质量汇总就宣布交付。

共同域终点大气质量相对误差小于 9e-10，水汽账本小于 4e-9；两档 E/P 相对不平衡
分别约 2.76% / 0.72%，TOA 净辐射约 -1.64 / -1.47 W/m²。P5 泥沙全局相对账本误差
均小于 4e-15。上述是观测值，不新增容差；生产验证继续消费既有常量。
季节相位缺陷仍开放，不把它归咎于本轮，也不将既有失败包装成质量全通过。

### 10.3 实现、身份与内存

- 每个完整强迫周期复用已有 full/fast `LayeredTendencyWorkspace`，每个慢步仍重算状态
  相关倾向；周期结束释放后才进入 FAS，避免额外细网格工作区与 FAS 所有者重叠。
  机械周期采用相同复用。没有新持久缓存、生产开关或依赖。
- 两个不同端点的最小夹具验证复用与原路径逐位相同，并验证取消；旧工作域的两档完整链
  JSON SHA-256 也完全相同。构建应变几何次数从 Draft 2832 / Standard 2736 降为
  118 / 114；该计数仅供离线成本解释，不是产品 schema。
- 工作域阶段版本递增，防止只包含 profile 枚举的旧输入命中旧域缓存；实际 grid/domain
  指纹继续传播到气候、P5 和最终产物。方程与粗 FAS 预算未改，不伪造新方程身份。
- 原 UI 的 Draft/Standard 选择直接消费新工作域；权威地形和显示分辨率不变。
  原子发布、取消、参数重建与字段切换仍需 UI 验证，用户最终验收尚待完成。
- 全链核查发现 UI 的两个入口只保留 `ProfileSurfaceBuilder::build` 的 authority，
  却先生成又丢弃控制网格、映射及 P1 报告；`CausalNaturalFormationStage` 随后 complete
  同一表面再建这些内容。改为 UI 只构建权威表面，图中仍完整建立并验证 P1。
  该删除不要求把新对象塞进 artifact，亦不改变科学机制或随机种子。
  独立 P1 夹具对照 authority + complete 与完整 build，既有整链检查继续覆盖消费者。

### 10.4 本修订的承重出处

- 数值工作尺度与守恒通量：S4（LeVeque 2002）；跨域保守映射：S5（Jones 1999）及现有生产实现。
- 应变几何复用：既有 `tendency/viscosity.rs` 的 MOM6/Olshanskii 实现与不可变 grid 指纹；
  仅改变已有工作区寿命，实际算子与运算次序由逐位对照约束。
- 选定工作尺度与预算：本节真实生产探针、既有质量包络和 AGENTS 的目标尺度裁定；
  不以旧图一致性替代质量，不降低守恒或有限性门禁。

### 10.5 同机成对计时

Release，无 RUSTFLAGS，每档三对，运行次序 B/C、C/B、B/C；每次新进程冷建完整
生产因果链，测量期间不运行其他构建或基准。使用原 profile 网格的同 HEAD 对照二进制。

| 档位 | 基线三次（秒） | 候选三次（秒） | 中位数变化 |
| --- | --- | --- | --- |
| Draft | 25.466 / 25.104 / 25.231 | 18.552 / 18.513 / 18.175 | 25.231 → 18.513，减少 26.6% |
| Standard | 58.915 / 53.935 / 55.470 | 38.597 / 37.948 / 38.730 | 55.470 → 38.597，减少 30.4% |

原始数据为仓外 `a9/paired-*.log` 和 `paired-summary.json`。这是包含 P1–P5 的
阶段探针，不包含 UI 文档构造、GPU 发布，也不包含后来删除 UI 重复 P1 的额外收益。
不得用此表冒充原生请求至 GPU 发布的基准，也不使用最快一次作为收益。

同一终点报告的生产 `dense_state_bytes`：Draft 从 31,541,456 降至 30,096,848，
Standard 从 113,281,680 降至 108,159,888 字节。这是现有求解器稠密所有者的保守
库存估算，包含权威输出；不是进程 RSS，也不等于整个应用峰值内存。复用并未增加
峰值同时存活的工作区，较大的权威地图输出仍占库存的大部分。

获选 Standard 第一次成对运行中，起点 P4 为 8.486 s，P5 与终点 P4 合计 13.210 s；
P3 relief 为 6.621 s。剩余成本仍主要在气候与地表反馈，地形构造也已不可忽略。
本轮没有给重采样、校验或 GPU 增加未经测量的缓存或并行机制；后续优先以这些真实
消费者继续评估降阶与求解策略，不把首轮提速当成全局优化结束。

### 10.6 验证记录

- 工作区跨两个状态/月份端点逐位等价与取消、P1 authority + complete 等价、工作域
  版本失效均观察 RED → GREEN；不增加全链 fixture 来证明局部行为。
- 共享求解器的 Release 回归覆盖全部普通目标及 doctests：1,325 passed，57 ignored。
  第一次运行在旧世界的层级指纹金样处失败；核对降水/P5/派生条件的依赖后，按 T1 v2
  A15 显式刷新身份金样。重新跑该套件及尚未运行目标，已通过且未改动的目标不重复。
  该过程未修改科学阈值或 ignored 状态。高成本参考仅在上述选型对照离线运行。
- 额外离线运行既有随机语料的第一个 seed `14500982625797044708`，Standard / Active
  完整链成功发布（1 passed）。消费者是历史跨 seed 发布回归；选择一个非默认 seed
  和活跃构造覆盖不同上游地形，不扩为默认八 seed 全扫。该次不计性能收益。
- `cargo fmt --all -- --check`、
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`、
  `cargo check --target wasm32-unknown-unknown --all-features --lib` 通过。
  `trunk build --release` 成功重建 `dist/`；本机 `NO_COLOR=1` 不符合 Trunk 的布尔解析，
  仅在构建进程设为 `true` 后运行，未改产品代码或全局环境。
- 独立 gpt-6-astra 只读审查无 Critical / Important / Minor，未搁置任何待判断项。
  代理检查不代替用户最终 UI 验收。

### 10.7 原生界面检查与用户入口

隔离原生 Release 验证包位于仓外 `a9/ui/sekai.exe`，启动脚本为
`D:/hemifuture-probes/civ-sekai-20261014/a9/run-optimized-ui.ps1`。它只增加独立
配置文件路径和请求至发布的日志；验证构建后已恢复生产源码，未引入产品开关。
该包 SHA-256 为 `68121BCC6016738D559E12F67F1B7261BB61AD5C930F53B5D149449395C28159`。

Draft / seed 42 首次原子发布成功，原生请求至发布观测 18.653 s；高程、年均气温、
年降水量切换成功，河流与地图正常绘制，面板预算与离线报告一致。这是一次 UI 检查，
不是三对基准，不能和历史 UI 中位数直接计算加速比。

标准档与取消的界面检查尚未完成：桌面 Unreal 预览覆盖了 Sekai，工具阻止对遮挡点
点击，激活后仍覆盖，因此停止输入并向用户询问前台安排。不会把离线通过冒充这两项
UI 已通过；用户最终验收同样待完成。

用户验证步骤：切到已打开的 Sekai，或用上述脚本重开；在左侧“质量档位”选择草稿/
标准，点击“按当前参数重建”；生成后检查高程、年均气温、年降水及河网。再修改档位
重建并取消，预期保留上一张有效地图；重建完成后才整体替换为新地图。标准档应显示
更细的权威地形，但不承诺更细的气候物理尺度。

### 10.8 代理补充自验修订（2026-10-14）

用户明确要求代理自行验证，继续完成可独立执行的证据收集。当前 computer-use 原生
连接报 `native pipe is unavailable` / `os error 2`，重置 JavaScript 会话并重新
初始化后仍失败；没有继续发送输入，也没有以后台测试冒充窗口点击。

- 使用独立 `ui-storage-self-check-standard/app.ron` 设置 Standard / seed 42，运行
  §10.7 同一个原生验证包，保留原 acceptance 配置。实际 Vulkan / RTX 4080 SUPER
  发布日志为 `profile=Standard seed=42 elapsed_ms=33431.095 success=true`。
  标准档完整生成并安装至 GPU 成功，窗口保留；这次单次观察不加入成对性能基准。
- 重新运行现有最小 Release 应用层契约，3 passed：
  `keyboard_activated_cancel_still_cancels_the_pending_build`、
  `same_frame_cancel_precedes_successful_world_and_cache_commit`、
  `pending_world_build_hides_the_old_map_until_rollback_or_publication`。
  它们使用生产 egui 帧和取消处理，分别证明键盘激活、完成/取消同帧的优先级及旧图
  恢复；同帧测试同时断言旧发布身份、缓存身份不变。这些局部风险无需再跑全链语料。
- 证据位于仓外 `a9/ui-self-check-standard.log` 与 `a9/ui-self-check-*.log`。
  本修订替代 §10.7 的“标准档生成尚未验证”状态；标准档字段切换和原生窗口中的
  实际取消点击仍未复核。此前 Draft 字段检查与完整回归证据继续有效。

### 10.9 修订：工作尺度与 LOD（用户澄清，2026-10-25）

用户澄清：目标分辨率带 LOD，支持域是每档从 L0 格元到显示叶片的整条层级
（AGENTS.md 同日修订）。§3.3 的“输出更细不意味着重新执行更贵的全球气候”
保留；“连续显示插值不产生新的科学信息”改为：细层显示须由有出处的物理降尺度
获得与派生地形一致的值（例如温度随派生高程、地形降水、与排水一致的细地形、
水面等值线岸线），不得把纯插值或整格继承当作已在该层成立。具体方法与层级
支持范围由后续“LOD 物理降尺度”规格确定，本修订不选方法。

同日只读审计（仓外 `D:/hemifuture-probes/lod-audit-20261025/report.md`）测得，
共同工作域下陆地邻格有效直减率 Draft −1.85 / Standard −2.50 K/km，Standard
过半 |Δz|≥300 m 邻对落在同一气候工作格内、温差为 0。这说明本轮共同工作域
需要直减率类降尺度配套；该配套归入上述后续规格，不在本修订内实现，也不改变
§10.1 的选择与 §10.5 的计时记录。

### 10.10 修订：权威表面与 P4 快照在信任边界只完整校验一次（2026-10-25）

落实 §5 第 4 条“同一不可变对象可在信任边界验证一次”。只读差距分析（仓外
`D:/hemifuture-probes/perf-gap-20261025/report.txt`）测得原生请求至 GPU 发布中，
`SphericalSurfaceSnapshot::validate`（完整拓扑、几何与指纹重算）每次构建对各表面共
调用 124 次，其中权威表面 90 次，Draft 约 3.3 s、Standard 约 13.5 s。P4 的
`GlobalCirculationSnapshot` 有同样的重复（见下文 P4 一段）。本修订不改变任何产物，
阶段身份版本不升。

**调用点。** 把 `validate` / `validate_cancellable` 临时改名后由编译器枚举，生产代码
共 65 处（含旧 foundation 链）。原生构建实际触发其中 27 处消费者站点（另有构造边界本身
2 处），主要是
`SurfaceRef::try_for_spherical`（权威表面 36 次，另含控制表面 3 次、气候表面 6 次）、
旧 `SphericalTectonicSnapshot::validate_against`（7）、P5 水面几何与海平面重建
（`world/natural/surface_water_geometry.rs` 7+2，`generators/natural/surface_water_geometry.rs` 7）、
`EvolvedTectonicSnapshot::validate_against`（6）、地形与基底 `validate_against`
及 `land_fraction_constraint_tolerance`（4+2+1）、地幔（4）、气候工作域对嵌套气候表面
（4）、P1 complete、引擎表面 artifact、quality 证据与 P5 输入各 1 次。

**信任边界。** 以构造约束表达“已验证”（King 2019，parse, don't validate；先例 A8
`70791e5`、`4f32353`）：

- `SphericalSurfaceSnapshot` 只有三个入口：`new`、crate 内 `new_cancellable` 与反序列化。
  反序列化改经私有 `SphericalSurfaceSnapshotWire` 的 `try_from`，原样收下记录（不重排、
  不重算指纹）后完整校验一次。字段只在 `world::spatial` 内可见，构造后不再修改；
  `validate`、`validate_cancellable` 与 `try_total_cell_area` 收为 `pub(super)`，只供
  上述边界使用。因此持有该类型的值就是有效性的证明。
- 之后的消费者只做常数时间的身份绑定：`SurfaceRef::try_for_spherical` 读取已存指纹与
  分配大小（合并原 `from_validated_spherical`），各 `validate_against` 只比对
  `SurfaceRef`、半径和计数；`SphericalSurfaceArtifact::validate` 不再重审已证明有效的
  表面；气候工作域不再重审嵌套气候表面与源表面。
- 删除只由重审产生、现已不可达的错误分支：共 32 个 `#[from] SphericalSurfaceValidationError`
  包装变体（含 `SurfaceRefError::InvalidSphericalSnapshot`、`NaturalSurfaceError::InvalidSpherical`）、
  `ProfileSurfaceBuildError::Invalid{Authoritative,Control}Surface`、
  `ClimateWorkDomainBuildError::InvalidSource` 及三个 P5 过程的 `map_surface_error`。
  `validate_against` 与 `validate_against_validated_surface` 两组入口现已同义，保留原名，
  不在本修订内合并。

**P4 快照。** `GlobalCirculationSnapshot` 本来就只能经 `new` / `new_cancellable` 与反序列化
（`Self::new`）得到，字段私有、构造后不变，但同一快照每次构建仍被完整校验 7 次（构造 2 次，
另有 4 次嵌在 `validate_against` 里、1 次在 bundle `validate` 里），每次都重算全场状态指纹。
按同一原则：`validate_against` 不再先跑 `validate_impl`，只查跨对象事实（表面身份绑定、
检查点网格指纹、风与洋流的切向性）；bundle 校验不再重审气候成员（其表面绑定与检查点指纹
仍由 `validate_bindings` 查）；`validate` / `validate_cancellable` 已无消费者，删除，只保留
构造边界调用的私有 `validate_impl`。原生计数（Draft / Standard 各 seed 42）：

| | `validate_impl` | 状态指纹 | `validate_against`（含嵌套） |
| --- | --- | --- | --- |
| 改动前 Draft | 7 次 / 410 ms | 9 次 / 483 ms | 4 次 / 256 ms |
| 改动后 Draft | 2 次 / 119 ms | 4 次 / 218 ms | 4 次 / 22 ms |
| 改动前 Standard | 7 次 / 1,622 ms | 9 次 / 1,910 ms | 4 次 / 963 ms |
| 改动后 Standard | 2 次 / 462 ms | 4 次 / 844 ms | 4 次 / 35 ms |

（`validate_impl` 的耗时含它内部的一次状态指纹。）现在每次求解只在构造边界完整校验一次。
剩下的重复是：生成器先算一次状态指纹写入检查点，构造边界校验又算一次来核对，每次求解
约 54 / 211 ms（Draft / Standard）。去掉它需要改为由构造函数自己计算指纹并生成检查点，
属于改动构造边界本身，不在本修订内做。

**失败语义。** 生产链只经构造产生表面，从不反序列化表面，因此对生产链能产生的输入
没有任何失败路径或错误码变化。损坏输入不再被接受后留待首个消费者以其自身错误码拒绝，
而是在反序列化边界即被拒绝，错误文案即 `SphericalSurfaceValidationError` 的原文案。
原以“反序列化后 `validate()`”断言具体错误的契约测试改为断言反序列化错误等于（或以）
该变体文案（开头）；新增窄单元测试
`artifact_deserialization_rejects_a_corrupted_surface`（引擎 artifact 不再重审，嵌套表面
须在反序列化即被拒），`surface_ref_contracts` 的过期指纹用例改为在表面边界被拒。
删除只验证未校验快照求和溢出的 `fallible_total_cell_area_reports_unvalidated_sum_overflow`：
该状态已无法从公共 API 得到。

**逐位等价。** 临时 Release 探针（未提交，仓外 `a10_identity_probe.rs.txt`）按生产因果图
对两档 seed 42 构建，改动前（`daba519`）后完全一致：

| 档位 | 表面指纹 | 构建结果哈希 | bundle 哈希（= JSON blake3） | 层级探针指纹 |
| --- | --- | --- | --- | --- |
| Draft | `0d09df7a…806f2e` | `da870182…cf25a5` | `afcde3fe…caa0522`（86,902,550 B） | `7df42191…954a6c3f` |
| Standard | `4247616b…c09e84` | `dd55f8d5…e9e1372` | `b4978e65…3e83ced`（336,825,943 B） | `e5d4fbd8…5c821387` |

Draft 层级探针指纹即 `tests/hierarchical_product_gates.rs` 的冻结金样。完整值见仓外
`identity-baseline.txt` / `identity-candidate2.txt`（只改表面时的中间版本 `identity-candidate.txt`
同样一致）。两档原生构建均成功发布到 GPU。

**同机成对计时。** 原生 Release、无 RUSTFLAGS、严格串行。两个二进制只差本修订的改动，
都打同一套临时计时插桩（`a10-timing-instrumentation.patch` 加 `instrument_p4.py`：请求 / 发布
标记、表面校验站点计数、P4 快照校验计数）。每档先预热一次，再交替跑三对，取请求至 GPU
发布的中位数。本轮开跑前负载 0–10%，没有其他编译或测试进程：

| 档位 | 改动前三次（ms） | 改动后三次（ms） | 中位数变化 |
| --- | --- | --- | --- |
| Draft | 14,660 / 14,505 / 14,453 | 10,790 / 10,822 / 10,798 | 14,505 → 10,798，−3,707（−25.6%） |
| Standard | 34,133 / 34,146 / 34,590 | 18,993 / 19,465 / 19,053 | 34,146 → 19,053，−15,093（−44.2%） |

表面完整校验从每次构建 124 次（Draft 3.33 s / Standard 13.98 s）降到 10 次（0.06 / 0.21 s）。
剩下的 10 次全部在构造边界：权威、控制表面各 1 次，气候立方球表面 7 次（2400 格，每次
约 3 ms，属于后续几何缓存的范围），一个 150 格表面 1 次。

更早一轮只含表面改动的成对计时（`native-summary.txt`）为 Draft 14,601 → 11,110 ms、
Standard 37,709 → 20,391 ms。那一轮改动前 Draft 第 1 次与 Standard 第 2 次开跑时另有 rustc
在跑，Standard 三次离散 14%，所以不用作结论。原始日志、清单和汇总都在仓外
`D:/hemifuture-probes/a10-validate-once/`（`native-summary-2.*` 是本表）。

**验证。** 改动碰到共享的表面类型、引擎 artifact 与 P4 快照，按测试范围纪律跑 Release 全量：
`cargo test --release --all-targets --all-features --no-fail-fast -- --test-threads=1`，
1,321 passed、0 failed、57 ignored。第一次并行运行时 lib 单测二进制与
`spherical_presentation_integration` 卡住（CPU 时间不再增长，属已知的并发创建 GPU 设备问题），
因此终止后改为串行重跑全量。并行那次另暴露 `spherical_artifact_wrappers_round_trip_with_strict_validation`
仍按旧语义期望 artifact 校验返回 `spherical-spatial.invalid-snapshot`；已改为断言嵌套表面在
反序列化边界以同一文案被拒。`spherical-spatial.invalid-snapshot` 仍用于构建器产出无效表面的
错误映射。门禁 `cargo fmt --all -- --check`、
`cargo clippy --workspace --all-targets --all-features -- -D warnings`、
`cargo check --target wasm32-unknown-unknown --all-features --lib` 均通过。编译期间 rustc 多次
`STATUS_ACCESS_VIOLATION`，用 `-j6` 重试后通过，与差距报告记录的现象相同。没有独立审查。

**用户验证步骤。** 本修订不改变界面与产物，只缩短等待。启动 Sekai，在左侧“质量档位”分别
选草稿 / 标准，点击“按当前参数重建”，预期地图与改动前一致（高程、年均气温、年降水、
河网），从点击到地图出现的等待明显变短。最终验收仍由用户在 UI 上完成。

### 10.11 修订：几何预热、海平面复用与身份 / 校验去重（2026-10-26）

承接 §10.10 与只读差距分析（仓外 `D:/hemifuture-probes/perf-gap-20261025/report.txt` 的秒级预算表）。
时延口径按 AGENTS.md 2026-10-25 的澄清：与种子无关的几何可在程序启动时于内存预热，不计入
“点击到可交互”，不得落盘。本修订五个提交都是实现层面的去重，不改变任何产物、指纹定义或
阶段版本。

**改动与实测节省**（原生请求至 GPU 发布，三次中位，Draft / Standard，ms；改动前为 §10.10 HEAD
`fcaf97f`）：

| 提交 | 改动 | 改动前 | 改动后 |
| --- | --- | --- | --- |
| `9e572a6` | P4 状态指纹由封存字段（`GlobalCirculationFields::seal_cancellable`）每次求解只算一次，构造函数比对而不重算；指纹按整片小端字节送入 BLAKE3（`world::fingerprint::update_f32_le`），P5 上游 JSON 指纹加缓冲 | 220 / 856（4 次） | 15 / 60（2 次） |
| `205cd57` | 气候工作域以 `ClimateWorkDomainArtifact` 为信任边界：起点强迫只做常数时间身份绑定，不再重建双向保守映射；删除阶段与引擎对刚构造快照的两次重审 | 起点强迫 191 / 648 | 13 / 17 |
| `c914674` | `SurfaceWaterStencil`：扇形面积、Shepard 权重与顶点权重和只依赖表面，P3、P5 各建一次供全部求解共享；水位扫描按扇形顺序预存顶点高程，水位不高于格内最低节点时整格跳过 | 5 次精确求解 144 / 452 | 94 / 292 |
| `08ba266` | `FormationGeometry`（权威表面、P1 bundle、气候工作域，及表面与工作域的引擎 artifact 哈希）在启动与切换档位时于后台线程预热，只放内存；生成请求经 `PreparedArtifact` / `Stage::prepared_output` 直接复用 | 权威表面 98 / 400，外部 artifact 28 / 109，工作域阶段 235 / 710，P1 complete 172 / 693 | 0.1 / 0.1，其余 0 |
| `b9452e5` | `NaturalFormationBundle::validate` 只查头部与成员间身份；四个成员快照与 §10.10 同为构造即有效 | 引擎 bundle 校验 28 / 104 | 0.02 / 0.04 |

**信任边界与缓存键。**
- `PreparedArtifact<T>` 用发布时完全相同的校验与流式 JSON 哈希准备一次；内容哈希定义不变，
  引擎单测 `prepared_artifacts_publish_the_hashes_of_a_fresh_build` 证明结果哈希与 artifact
  集合哈希和现算相同，且准备好的阶段不运行。
- `FormationGeometryKey` = 质量档位 + 半径（按位比较），即三项几何的全部真实输入（生成代码在
  进程内固定）。阶段只在输入的档位与表面身份（`SurfaceRef`，含完整几何指纹）与几何一致时复用，
  否则照常构建。几何不含种子或任何作者参数，也不含世界状态。契约单测
  `prepared_geometry_matches_the_stage_builds_and_binds_only_to_its_key` 证明各部分等于阶段自建
  结果、表面哈希一致、只绑定自身的键。
- 应用层 `FormationGeometrySlot`（就绪 / 预热中）取代原只缓存权威表面的条目。键相同而仍在预热时，
  生成线程等待（每 10 ms 查一次取消）；几何构建失败或被取消时退回原路径，由原阶段以原错误
  报告。过期的预热槽被丢弃时取消其线程。构建进行中切换档位所启动的新预热不会被返回的旧槽覆盖。
- 预热后常驻内存：点击前工作集 Draft 307 对 273 MB（+34），Standard 393 对 272 MB（+121）；
  Standard 峰值工作集 1384 对 1326 MB。

**逐位等价。** 临时 Release 探针（未提交，仓外 `a10_identity_probe.rs.txt`）按生产因果图构建两档
seed 42，并另以 `causal_natural_formation_graph_with_geometry` 走预热路径。每个提交后都与
`fcaf97f` 比对，表面指纹、构建结果哈希、bundle 哈希（= 整个 bundle JSON 的 blake3）与层级探针
指纹全部一致，预热路径的结果哈希与 bundle 哈希也一致，即 §10.10 表中的值（Draft
`da870182…cf25a5` / `afcde3fe…caa0522` / `7df42191…954a6c3f`，Standard `dd55f8d5…e9e1372` /
`b4978e65…3e83ced` / `e5d4fbd8…5c821387`）。原始输出见仓外 `identity-baseline.txt`、
`identity-c1.txt` … `identity-c5.txt`。

**同机成对计时。** 原生 Release、无 RUSTFLAGS、严格串行，每次开跑前负载 < 10% 且无 sekai /
cargo / rustc 进程。两个二进制只差本修订，计时插桩同 §10.10（外加推迟启动点击的
`SEKAI_PG_REQUEST_DELAY_MS`，只存在于仓外补丁）。每档先预热一次，再交替跑三对，取请求至 GPU
发布的中位数。

| 口径 | 档位 | 改动前三次（ms） | 改动后三次（ms） | 中位数变化 |
| --- | --- | --- | --- | --- |
| 预热完成后点击（启动 8 s 后） | Draft | 10,707 / 10,753 / 10,873 | 9,849 / 9,856 / 9,827 | 10,753 → 9,849，−904（−8.4%） |
| 预热完成后点击（启动 8 s 后） | Standard | 18,761 / 18,715 / 18,729 | 15,160 / 15,241 / 15,099 | 18,729 → 15,160，−3,569（−19.1%） |
| 冷启动立即点击 | Draft | 10,724 / 10,727 / 10,796 | 10,565 / 10,329 / 10,307 | 10,727 → 10,329，−398（−3.7%） |
| 冷启动立即点击 | Standard | 18,727 / 18,646 / 18,713 | 17,168 / 17,436 / 17,562 | 18,713 → 17,436，−1,277（−6.8%） |

冷启动时请求等待预热完成（几何构建 512 / 1,889 ms），其余去重照常生效。原始日志、清单与汇总
在仓外 `D:/hemifuture-probes/a10-prewarm-reuse/`（`native-summary-{prewarmed,cold}.*`）。

**bundle 哈希评估（未改）。** 引擎内容哈希是 bundle JSON 的 blake3：序列化约 148 / 548 ms，
哈希本身只有 15 / 60 ms；气候快照占 JSON 字节的 78%。逐位不变的办法只有加快或并行化同一
字节流，而顶层按成员并行最多 1.27 倍，且须在序列化之外复写 bundle 的字段顺序，违反唯一
事实源，未做。改为组合已有子指纹（P5 上游 JSON 指纹、P4 状态指纹、地表快照 JSON 等）估计可
省约 140 / 490 ms，但结果哈希必然改变，按“改变产物必须升版本”须提升形成阶段版本——而阶段
版本参与阶段种子派生，提升会使每个种子生成另一个世界；同时气候快照正被 `a10-climate-reduced`
替换。是否接受这一身份变更（或另设不参与种子的身份版本）留待用户裁定，宜在气候替换落地后
一并处理。

**剩余最大耗时段**（预热后点击，中位）：Draft 起点 P4 求解 5,312、P5（含终点 P4）3,630、
P2 构造 326、bundle JSON 哈希约 150、LOD 初始烘焙 99、P3 地形 109、海平面 94；Standard 起点 P4
6,113、P5 5,196、P2 1,794、bundle 哈希约 540、LOD 386、P3 地形 376、海平面 292、组装（投影地图
网格，双峰）263。投影地图网格只依赖表面与投影，也可预热或按格并行，留作后续候选。

**验证。** 改动碰到引擎 artifact / 阶段接口、共享表面水体算子与 P4 快照，按测试范围纪律跑 Release 全量：`cargo test --release --all-targets --all-features --no-fail-fast -- --test-threads=1`，1,323 passed、0 failed、57 ignored（§10.10 为 1,321 passed；新增三个窄单测——指纹整片送入等价、引擎预备 artifact、几何预热契约——删除随旧缓存条目失效的键测试）。门禁 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo check --target wasm32-unknown-unknown --all-features --lib` 均通过。本轮编译未出现 rustc 崩溃。没有独立审查。

**用户验证步骤。** 本修订不改变界面与产物。启动 Sekai，等几秒（预热在后台进行），在左侧
“质量档位”选草稿或标准，点击“按当前参数重建”：地图与改动前一致，等待比改动前短；切换档位后
稍等再点同样受益。启动后立刻点击时也能正常生成，只是先等预热完成。生成中点“取消”仍保留上一张
地图。最终验收由用户在 UI 上完成。
