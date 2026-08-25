# 地质管线架构契约恢复与当前态因果生成设计

日期：2026-08-24  
状态：**用户已批准；2026-08-25 最终态与生成效率显式修订已批准**

上位规格：`2026-08-17-complete-natural-world-pipeline-design.md`  
修订对象：

- `2026-08-17-evolved-tectonics-v5-design.md`；
- `2026-08-17-substrate-primary-relief-p3-design.md`；
- `2026-08-18-coupled-geomorphic-formation-p5-design.md`；
- `2026-08-21-t0-hypsometric-calibration-design.md`；
- `2026-08-24-transient-climate-geomorphology-design.md` 的 R3/R4。

本设计不另起一条地质管线。它恢复项目立项时已经冻结的单向职责边界，关闭
后来登记但未退役的兼容债务，并更正“只发布当前状态”被误推导为“固定瞬时
强迫下必须存在绝对高程稳态”的求解语义。

## 0. 2026-08-25 最终态与生成效率显式修订

本修订落实 commit `7648e0f` 已写入 `AGENTS.md` 的产品边界，并显式替代
commit `f1df994` 在本规格加入的以下要求：

- 每个 `2 Myr` P2 构造宏步都执行 start/midpoint/endpoint 三次 P4 与两个半步
  P5；
- 以 `2/1/0.5 Myr` 三层 step-doubling、人工耦合误差包络和用户批准作为公开
  artifact/UI 迁移的发布门；
- 把完整形成轨迹的耦合收敛当作地图生成器每次构建都必须证明的产品性质。

替代后的约束如下：

1. P2/P3/P4/P5 的模块所有权、公式出处、单位、`f64` 科学状态、质量/水量守恒、
   数值域、跨层身份和原子发布仍是硬约束，不因采用近似求解而放宽。
2. 生产默认采用一次 Lie-style sequential operator splitting 加终点闭合：P2
   先完成 resolved timeline，P3 从最终固体状态投影一次基础地形，P4 在该起始
   formation terrain 上求快平衡，P5 以自身稳定子步推进既有 P5 规格冻结的
   `100,000 yr` coarse-grained formation horizon，然后从最终
   `FormationTerrainFields`/`SurfaceWaterGeometry` 重建 forcing 并再求一次
   发布 P4。该顺序保留同一组物理算子、各域已经批准的时域和守恒账本，只改变
   算子之间的调用日程；不得把 P2 的 `128 × 2 Myr` 时域误塞给 P5。
3. 发布 P4 后只以零时间重算 P5 的终点水文和当前过程率；不得再次积分侵蚀、
   沉积或水量。最终 P4 forcing 必须与最终地形/水面几何一致，P5 checkpoint
   必须绑定该 sibling P4。终点闭合不宣称整条私有轨迹已经收敛或达到耦合不动点。
4. 若离线对照证明上述单次分裂不能满足既有最终态质量包络，唯一预留的升级是
   **一次有界 predictor-corrector**：先得到预测终点 P4，再从同一个 P3/P5
   初态重跑一次 P5 校正，最后重建并求发布 P4。校正次数固定为一，不增加运行时
   容差、收敛循环或用户 cadence 旋钮；是否升级必须另作显式规格修订。
5. 上一版每宏步三次 P4 的路径只保留为 ignored/release 离线高成本参考探针，
   默认语料为 `Standard`/seed `42`。它只比较最终地形组成、水面几何、气候、
   水库/沉积库存、既有质量指标、守恒、指纹和耗时，不进入产品 schema、缓存、
   UI 或每次构建门禁。
6. 删除三层 step-doubling 和新造误差包络。离线参考的原始最终态差值是研发证据，
   不是未经来源批准的新失败阈值；产品验收仍由硬不变式、既有质量包络、现有
   profile 性能门禁和 UI 结果承担。
7. Lie–Trotter、顺序耦合和固定次数校正有数值方法依据；Sekai 选择“生产两次
   外层 P4”或候选校正路径“三次外层 P4”的具体次数没有可直接移植的文献常量，
   且 P2/P5 保留各自不同的 resolved horizon，因此这里是 Lie-style sequential
   splitting 的工程类比而非同一 `Δt` 上的形式 Trotter 收敛声明。具体日程属于
   以离线实测裁定的开放问题，不得伪装成学术结论。

## 1. 用户裁定与问题归因

### 1.1 继续有效的用户裁定

1. 产品只发布一个完整、原子的**当前状态**；历史序列、求解中间态、拒绝步和
   伪时间不得进入 artifact、持久缓存恢复契约或 UI。
2. 地质管线必须具有单一、清晰的模块所有权；一个过程只能有一个生产实现与
   一个权威事实源。
3. 物理演化可以在一次构建内部拥有因果时间和私有工作状态。构建成功后只发布
   终态，失败或取消时不留下部分 artifact。
4. 科学状态先保持完整 `f64` 身份，再校验和发布；不得用裁剪掩盖无根、缺项、
   精度损失或数据域不足。
5. 未来可以另行设计用户可配置的高程显示/支持范围；本修订不提前增加该旋钮。

### 1.2 已确认的两类漂移

**所有权漂移。** P2 V5 规格明定“结束于构造成因场”，但当前
`spherical_tectonics::processes::relaxation` 仍在同一过程里执行洋壳年龄推进、
兼容高程松弛、大陆线性侵蚀和海沟填充。P3 又在大陆格元上读取
`compatibility.tectonic_elevation_m` 作为累计响应。T0 R3 已把这种大陆继承登记
为“L2 完成后评估退役”的已知债务；L2 已交付，债务仍在生产权威链中。

**求解语义漂移。** P5 R3 正确保留了“只发布当前状态”，却进一步删除内部
物理时间，把 P2/P3 发布的今日瞬时构造速率当作无限期固定外部强迫，并要求
绝对高程求稳态。当前探针已经证明该方程并不普遍可解。

### 1.3 本轮完整状态证据

原 `9000.000260834617 m` 上界失败来自 `f32` 中间高程与 `f64` 组成状态身份
不一致；修正为完整 `f64` 聚合后，Draft/seed `42` 的 `CellId(19366)` 暴露真实
下界失败：

- 完整候选高程 `-11000.000274626422 m`；
- `uplift = 0.024603449 mm/year`；
- `subsidence = 1.040666938 mm/year`；
- 构造净率 `-0.001016063 m/year`；
- runoff、河流侵蚀、坡面项、路由沉积、海岸项和 Airy 响应均为零。

因此，在固定瞬时强迫、绝对高程为状态量且当前过程集合不变时，该格元没有
`dh/dt = 0` 的域内根。扩大伪时间工作量、放宽残差或钳在下边界都不能改变这
一事实。

## 2. 恢复后的唯一模块所有权

### 2.1 P2：固体地球演化与构造成因

P2 唯一拥有：

- 刚性板块运动、板块身份与边界事件；
- 地壳生成、俯冲、碰撞、拉张、增生、年龄和物质账本；
- 由固体地球过程直接产生的当前抬升、沉降和缩短强迫；
- 构造控制面到权威球面的守恒发布。

P2 不拥有大陆侵蚀、河流输沙、坡面输运、海岸交换或海沟沉积库存。洋壳年龄
推进属于 P2；由年龄导出的热沉降高程属于 P3 的地质投影，不得同时写入一个
会被 P3 再次继承的累计兼容高程。

### 2.2 P3：当前固体状态到基础地形的无历史投影

P3 唯一拥有：

- 由权威地壳物质、密度和年龄导出的 Airy/热沉降基础高程；
- 火山构造、被动陆缘与其他已经冻结的基础地形组成；
- 初始水量几何与海平面；
- P5 开始前的完整基础地形恒等式。

P3 是同一时刻事实之间的确定性投影，不自行推进地质时间，也不得读取
`compatibility.tectonic_elevation_m`。构造运动对地形的影响必须来自权威物质
状态、当前有单位强迫，或在因果协调器内相邻固体状态的可记账差分。

### 2.3 P4：当前边界条件上的快平衡

P4 唯一拥有大气—海洋快时间尺度求解。P4 solver 只读取已验证 forcing，不读取
P5 累计器或 P2 工作区；唯一 forcing builder 把当前完整地形及其
`SurfaceWaterGeometry` 投影为 P4 forcing。P4 可以在一次私有因果演化中被
多次调用，但只发布终态地形/水面几何上的最后一次一致解。最终 P4 的
`forcing_fingerprint` 必须等于从最终形成地形及其水面几何重建的 forcing
指纹；缺少该一致性是 typed failure，不允许回退到宏步起点或中点气候。

### 2.4 P5：地表过程与水圈演化

P5 唯一拥有：

- 河流侵蚀、输沙、沉积和盆地库存；
- 坡面扩散/滑移、海岸侵蚀与沉积；
- 地表加载/卸载产生的等静力响应；
- 湖泊、土壤水、地下水、雪冰和海洋之间的水量交换；
- 当前高程组成与地表物质、水量账本。

P5 不改变板块身份、地壳物质谱或构造事件。构造只以 P2 的权威状态变化或
有单位强迫进入 P5；P5 不从兼容高度图反推构造成因。P5 snapshot 不拥有 P4
snapshot；P5 算子显式借用当前 P4，发布时只记录最终 P4 checkpoint 指纹，
防止跨域状态复制。

### 2.5 因果协调器：只编排，不拥有方程

生产新增一个领域专用的自然形成协调单元。它位于 `generators/natural/`，只
持有 P2/P3/P4/P5 的私有工作状态并按时间顺序调用既有算子。它不得实现第二份
构造、气候、侵蚀、沉积、海平面或 Airy 公式，也不得把反馈能力抽象成无人
消费的通用引擎 trait。

协调器一次成功调用只返回最终当前态 bundle；引擎仍执行单向、可缓存的有向
无环图，不为本任务增加通用循环 stage 或跨 stage 可变共享状态。

### 2.6 后期参数异质性的所有权与输入契约

后期计划所称“统一噪声抖动”在本修订中统一改称**参数异质性**：同一物理算法
仍求解同一方程，但其中经文献或数据允许空间变化的参数，可以由全域常数推广为
一个有指定边际分布和空间相关性的确定性参数场。它不是独立地貌算法，不在最终
高程、边界、洋壳年龄或水量上追加随机残差。

常数是零离散度的退化分布，因此后期扩展不需要平行的“常数算法”和“噪声算法”。
参数场只向原算法提供局部 `f64` 参数值；P2/P3/P5 仍分别拥有自己的方程、单位、
状态更新、守恒账本和失败语义。统一部分仅限现有
`generators/natural/morphology::noise`、`FieldRecipe`/`GaborKernel` 和
`LabeledSubstreams` 已承担的相关场采样、随机流正交与确定性规则，不新增第二个
球面噪声入口或跨模块状态写入器。

每个真实参数消费者在实施前必须冻结以下 resolved 输入；没有直接出处的项必须
标为类比和开放问题：

- 参数身份、所属模块、所进入的方程项、物理单位与科学支持域；
- 边际分布族及其位置/尺度/形状参数；分布的支持域必须按构造满足物理域，禁止
  先采样再 clamp；
- 空间相关结构：物理或角相关尺度、频带权重，以及确有消费者时的各向异性和
  方向来源；
- 采样坐标系：世界球面、物质随体坐标或由过程所有者提供的局部切向/法向；
- 因果条件场和作用域，例如边界类别、岩性、洋脊方向或局部铺展状态；不得接受
  任意绘图 mask；
- 多参数确有直接依据时的联合分布、交叉协方差或共享潜在场；不得默认相关，
  也不得用一个全局噪声场把无关参数暗中绑定；
- 独立、版本化的标签子流，以及分布/采样算法版本；不新增第二个用户 seed；
- 必须精确保持的总体矩、对称性或质量约束，以及不满足约束时的 typed failure；
- `SurfaceRef`、世界半径、真实单元面积和可解析最短波长。分辨率过滤由工作域
  推导，不暴露 octave、lacunarity 或格元尺度等 UI 算法旋钮。

运行时输入只来自已验证的 resolved formation/profile、权威球面、具名随机子流
和该算法已经拥有的因果状态。参数场是私有中间输入，不进入 bundle 历史；其
配方、版本和标签进入输入/产物指纹。增加更高频带不得改变其他参数子流、板块
owner、事件序列或既有低频结果。

未来 P2 去规则化应优先让已有的板块阻力、裂谷倾向或局部铺展参数按有出处的
分布变化，使边界和年龄条带通过原过程自然变得不规则；不得直接执行
`final_boundary += noise` 或 `final_age += noise`。这些具体参数采用何种边际
分布、相关尺度和条件关系尚无本规格可直接移植的统一出处，故仍是后期任务的
开放问题，不能由本修订预钉数值。纯显示微细节若未来存在，则属于 `view/gpu`，
不得回流科学 artifact。

## 3. 兼容视图隔离

### 3.1 权威消费边界

`EvolvedTectonicSnapshot` 中板块几何、地壳种类/年龄/线理、物质和 forcing
仍是 P3 真实消费者所需事实。实现应提供一个最小的借用型权威输入视图，使 P3
能读取这些事实，但该视图**不暴露** `tectonic_elevation_m`。

该视图不是新的序列化 snapshot，不复制数组，不构成第二事实源。现有
`compatibility()` 仅供旧 V3 呈现、冻结回归和迁移诊断；生产 P3/P4/P5 不得
调用它取得高程响应。

### 3.2 必须成立的不变式

在保持板块、地壳、物质、forcing、seed 和输入规格不变时，任意改变兼容
`tectonic_elevation_m`：

- 不得改变 P3 权威基础地形；
- 不得改变 P4 forcing；
- 不得改变 P5 当前科学状态；
- 只允许刷新兼容视图自身的字节和专属诊断。

该不变式以生产构造器测试，而不是源码字符串扫描，作为模块所有权门禁。

### 3.3 粗松弛的处置

当前 `relax_current_crust` 必须按职责拆分：

- 保留 P2 所需的地壳/事件年龄推进和固体物质状态更新；
- 大陆侵蚀与海沟沉积从新权威路径移除，由 P5 生产算子唯一承担；
- 旧兼容高度需要保留的 Cortial 粗响应只能进入显式 legacy/compatibility 路径，
  不得影响权威 forcing、P3 或 P5；
- 洋壳热沉降高程由 P3 现有年龄—深度事实源唯一导出。

兼容路径若失去真实消费者，应按 YAGNI 删除；是否删除由调用点审计决定，不
预先保留投机性迁移 API。

## 4. 当前态的因果时间语义

### 4.1 “当前”是私有历史的终点

生产当前态定义为：

```text
S_current = evolve_causally(S_initial, resolved_formation_timeline)
```

内部 `S(t)`、接受/拒绝步、P4 中间平衡和构造控制状态都是一次构建的私有工作
内存。artifact 只包含 `S_current`、当前速率/通量、守恒证据和输入身份；不包含
时间序列、可恢复历史或演化播放器数据。

因此，“当前态产品”与“内部有物理时间”并不冲突。R3 §14.3 中“没有发布
历史，所以只能求固定强迫稳态”的推论由本修订替代。

### 4.2 形成时间的唯一事实源

现有 P2 参考实现使用 `2 Myr` 离散步和 `128` 步有界演化。Cortial et al.
(2019) 为球面程序板块过程和 `2 Myr` 离散步提供直接依据，但不为 `128` 步
提供依据；`128` 是 Sekai 当前已批准、需要保持产品身份连续性的参考形成时域
参数。实施时把这两个来源不同的执行事实从 `spherical_tectonics::runner`
私有常量提升为 `world` 层的 resolved formation timeline；P2 与因果协调器
只能消费这一份 resolved 事实源。

P2 是有地质过程依据的程序形成模型，不是预测性地球动力学模拟。该时间表是
当前参考形成模型的产品参数，不宣称为地球年龄、所有世界的真实年龄或论文
校准常量。它进入输入身份和构建指纹，但本修订不增加用户年龄旋钮，也不把
内部逐步历史发布到 UI。未来若用户可编辑形成时长，必须另做规格、性能包络
和 UI 设计。

### 4.3 生产单次分裂与终点闭合

一次生产构建遵循：

```text
advance P2 solid-earth state across the resolved timeline
derive one final authoritative P3 terrain/SurfaceWaterGeometry
initialize the retained f64 formation state
rebuild forcing and solve start P4
advance P5 once across its resolved 100,000 yr horizon with stable physical substeps
rebuild forcing and solve endpoint P4 from final terrain/SurfaceWaterGeometry
recompute terminal hydrology and current process rates under endpoint P4
validate solid, sediment, water, component and endpoint-forcing identities
publish the complete bundle atomically
```

这是对同一组 P2/P5 演化算子的一次 Lie-style 顺序分裂。P2 仍按自己的 `128`
个 accepted steps 完成 `256 Myr` 程序形成时域，P5 仍完整消费其上位 P5 规格
冻结的 `100,000 yr` coarse-grained horizon 并使用已有稳定子步；两个时间参数
来源不同，禁止合并。删除的是两者在每个 P2 宏步上的高频互调，不是物理过程、
各自时域或账本。P3 是最终固体状态的确定性投影，不再需要逐宏步把整张基础
地形重置进 P5。

start P4 驱动这一次 P5 生产推进；endpoint P4 只在最终地形与最终水面几何上
重新闭合发布 forcing。终点诊断重算不增加物理时间，也不重复沉积、侵蚀或水量
积分。一次完整生产构建的外层 P4 调用数因此固定为两次；P4 自己的快平衡内部
迭代不计入该数字。本规格不新增 cadence、收敛容差或按性能调节的用户旋钮。

### 4.4 有界校正候选与离线参考

若生产单次分裂未满足**既有**最终态质量包络，可以在显式修订后改用一次固定
predictor-corrector：

1. 从 P3 初态用 start P4 完成一次 P5 predictor；
2. 在预测终态重建 forcing 并求 predicted endpoint P4；
3. 丢弃 predictor 的 P5 累计状态，从同一个 P3/P5 初态在 predicted endpoint
   P4 下完整重跑一次 P5 corrector；
4. 从校正终态重建 forcing，求 final endpoint P4，并零时间重算终点诊断。

该候选固定一轮校正，即三次外层 P4、两次 P5；不迭代到容差，也不同时保留为
用户可选生产模式。只有离线证据证明单次分裂不能满足既有最终态要求时才实施，
避免为尚未出现的误差预付第二次 P5 成本。

上一版 start/midpoint/endpoint 每宏步耦合 cadence 只存在于一个私有
ignored/release 参考探针。探针在 `Standard`/seed `42` 上运行完整 P2 timeline，
并把同一个 `100,000 yr` P5 horizon 精确分配到全部 P2 窗口，避免用不同 P5
物理时域比较算法；它记录参考与生产候选
的最终地形组成、`SurfaceWaterGeometry`、climate fields、沉积/水库、既有质量
指标、守恒残差、wall time、peak RSS 和全部相关指纹。它不保存中间轨迹，不做
`2/1/0.5 Myr` 细化，不产生新误差包络，也不进入常规测试、每次构建或发布门。

生产单次分裂若破坏守恒、有限性、支持域、最终身份或既有质量门禁，必须更换
求解策略；不得 clamp 或后处理结果。若全部硬不变式、既有质量与性能门禁通过，
离线参考中的非零差值只作为适用范围记录，不自动把地图生成器升级为轨迹收敛
模拟器。

### 4.5 稳态与 PTC 的降级

动态平衡残差保留为当前状态诊断，但不再是所有世界的充分或必要发布条件。
活动构造区允许发布非零 `dh/dt`，前提是当前速率有单位、各过程有归因、库存
守恒且有限时间积分通过数值误差门禁。

Pseudo-transient continuation 只允许用于已定义且预期存在根的内部快平衡子
问题，例如适用的 P4/水库 spin-up；不得再作为整个绝对地貌的外层求根器。

## 5. 完整数值状态与高程边界

1. `formation_elevation_from_components` 继续是唯一高程恒等式。
2. 任何会跨步累计或参与验收的高程组成保留 `f64`；`f32` 只用于最终已验证的
   wire/GPU 发布转换，不能回流成为下一步科学状态。
3. Airy 响应直接加到完整组成状态，再以完整和校验；不得先量化工作高程再反推
   响应。
4. 当前 `ELEVATION_MIN_M`/`ELEVATION_MAX_M` 只表示现有 artifact 支持域。完整
   候选越界返回包含格元和未裁剪 `f64` 值的 typed failure。
5. 科学状态不得 clamp。色标、GPU 位移或相机裁剪可以使用独立显示范围，但不
   得改写 artifact。

未来若增加用户高程范围，必须区分“显示范围”“artifact 支持域”和“物理机制
产生的高程”。只有前两者可以是直接配置；物理峰值不能通过无过程的饱和函数
实现。

## 6. 原子 artifact 与运行时图

### 6.1 领域 bundle

生产使用一个当前态形成 bundle 原子承载最终：

- evolved tectonics；
- geologic substrate；
- primary relief/current elevation components；
- formation-consistent P4 climate；
- P5 hydrology、sediment、water reservoirs 与当前过程通量；
- 各子域质量/水量/数值误差报告。

所有权布局固定为：

```text
NaturalFormationBundle
├── timeline
├── tectonics
├── substrate
├── primary_relief
├── climate: GlobalCirculationSnapshot
├── surface_formation: NaturalSurfaceFormationSnapshot
└── quality reports
```

`NaturalSurfaceFormationSnapshot` 不再包含 `formation_climate`。
`SurfaceFormationUpstreamFingerprints` 只保留
`formation_climate_checkpoint_fingerprint`，用于声明终点 P5 诊断由哪个
sibling P4 checkpoint 驱动。world 层 bundle 结构 validator 必须验证共同
`SurfaceRef`/timeline，以及
`surface_formation.checkpoint().upstream()` 的 climate checkpoint 指纹等于
sibling P4 checkpoint 指纹。需要 climate spec/work domain 的
terrain→forcing 关系由 generators 层唯一 artifact factory 使用协调器保留的
同一个 final forcing 做 contextual validation：sibling P4 的
`forcing_fingerprint` 必须等于该 forcing 指纹，而该 forcing 必须由最终
formation terrain 及其 water geometry 直接构造。world 不得越层调用 forcing
builder，也不得为了无上下文反序列化复制一份 forcing 方程。P5 质量评估器显式
接收 sibling climate 或完整 bundle，不从 P5 内部反向取得 P4。

bundle 是一次耦合构建的事务边界，不是把各领域合并成一个算法模块。其内部
payload 继续使用既有领域 snapshot 与验证器；下游只能通过具名只读访问器消费
最终子域事实。

### 6.2 迁移原则

- 不修改通用 `Stage` 为多输出或循环接口；领域协调 stage 仍只有一个输出。
- 旧独立 P2/P3/P4/P5 stage 在迁移期只服务隔离测试和旧缓存拒绝；生产图切换
  后，无真实消费者的适配器删除。
- 任一 schema/stage/equation 变化按现有版本规则刷新身份；不得双写新旧科学
  artifact 来假装兼容。
- bundle 完整验证成功前不进入 `BuildArtifacts`；取消、预算失败、越界和守恒
  失败均保留上一个完整世界。

## 7. 错误与诊断语义

生产失败至少区分：

- 无效输入或时间表；
- 数值稳定性/时间误差未通过；
- 固体、沉积或水量账本不闭合；
- 完整 `f64` 状态非有限或超出 artifact 支持域；
- P4 快平衡子问题不收敛；
- 最终 P4 forcing 与最终地形/`SurfaceWaterGeometry` 身份不一致；
- 生产近似破坏既有最终态质量门禁或任一硬不变式；
- 资源上限或用户取消。

“当前局部速率非零”不再是错误。“固定强迫绝对地貌无稳态根”只作为被退役
外层方程的诊断证据，不再触发无限 continuation。错误报告不得携带可发布的
“最佳中间态”。离线参考与生产结果存在非零差值不是运行时错误；只有该差异已经
表现为上述硬不变式或既有质量门禁失败时，才要求调整求解策略。

## 8. 实施阶段与验收

### 阶段 A：冻结契约与基线

- 把本规格批准为 P2–P5 后续工作的唯一修订依据；
- 固定当前 `f64` 边界回归和 seed `42` 无根探针；
- 记录生产图中所有 `compatibility()` 消费者及其真实用途；
- 记录 Draft/Standard 的逐算子耗时和峰值内存，不改算法。

交付物：架构消费清单、数值基线和可复核探针；无科学行为变化。

### 阶段 B：隔离兼容视图

- 建立不暴露兼容高程的借用型 P2→P3 权威输入；
- 以失败测试证明改变兼容高程当前会污染 P3，再完成隔离；
- 移除大陆兼容高程继承，复测 L2 物质造山是否承担既有地形包络；
- 若包络失败，只报告物质/过程缺口，不恢复兼容高程或增加修形系数。

交付物：P3/P5 对兼容高程的生产不变性和刷新后的 P3 证据。

### 阶段 C：恢复过程唯一所有权

- 把 P2 粗松弛拆为固体年龄/状态推进与 legacy 高程响应；
- 从新权威路径删除大陆侵蚀、海沟沉积和重复洋壳热沉降高程；
- P5 继续通过现有正交算子唯一处理侵蚀、沉积、海岸和 Airy；
- 增加逐过程质量与高程组成测试。

交付物：每个过程只有一个生产所有者，P2 物质与 forcing 门禁保持可解释。

### 阶段 D：引入私有因果形成协调器

- 保持 P2 one-shot 生产循环，仅为离线参考提供 `#[cfg(test)]` accepted-step
  observer；
- 完成 P2 resolved timeline 后以最终 P3 投影建立 `f64` formation state；
- 接入 start P4、一次完整 P5 稳定推进和 endpoint P4 的生产 Lie-style 分裂；
- 终点 P4 后只重算水文/过程率诊断，并强制校验终点 forcing 身份；
- 完成守恒、确定性、取消、性能和代表性离线参考探针；
- 删除外层绝对地貌 PTC 和普遍稳态发布门禁。

交付物：只返回最终当前态、内部有物理时间且不要求不存在的稳态根的生产生成器。

### 阶段 D→E：最终态与性能门

进入公开 bundle、生产图和 UI 迁移前，生产 Lie-style 路径必须同时满足：

- 最终 P4 与最终完整地形/`SurfaceWaterGeometry` forcing 身份一致；
- solid/sediment/water/component、有限性、支持域和现有数值稳定性门禁通过；
- 既有 P2/P3/P4/P5 最终态质量包络通过；
- 现有 profile 的时间、内存和取消门禁通过。

耦合策略改变时另运行一次 `Standard`/seed `42` 高成本参考探针，并把机器、
编译档、最终态原始差值、守恒、质量指标、逐域耗时、峰值内存和指纹记录为研发
证据。该探针不在每次构建或常规发布门中执行，也不要求先新造一个误差包络。

若生产路径因耦合离散误差未通过前三项，必须记录成因，再显式修订为一次有界
predictor-corrector 或其他有出处的耦合策略；若仅性能门失败，则先定位 P2/P4/
P5 成本，再选择有出处的近似线性解、多分辨率工作域或内部有界求解，不能用会
增加成本的 predictor-corrector 冒充性能修复。两类失败都不得跳过 endpoint P4、
缩短形成时域、放宽守恒/稳定性门禁或以 clamp 修补。若已有门禁全部通过，不因
离线参考存在非零轨迹/终态差异而停止阶段 E；最终产品仍由 UI 中的当前态验收。

### 阶段 E：原子 artifact 与生产图迁移

- 发布当前态形成 bundle 并完成严格反序列化/版本拒绝；
- 下游 field registry、T1、最终气候/水文和呈现切换到 bundle 的具名终态访问器；
- 删除失去消费者的旧 stage、wire、伪时间报告和缓存恢复路径；
- 刷新受影响指纹和完成记录。

交付物：单一生产图、单一身份事实和无双写兼容层的当前世界。

### 阶段 F：UI、证据与最终验收

- UI 只显示最终当前态和当前有单位过程场，不提供内部历史播放器；
- 构建进度按领域/当前工作项报告，不把内部时间冒充世界可编辑年龄；
- 报告明确区分科学高程、artifact 支持域和显示范围；
- 完成 Draft/Standard 多 seed、原生/WASM、完整门禁与用户上手验收。

交付物：用户可在应用中触发生成、检查构造—地貌因果场并验证不再因无根稳态
或 `f32` 身份丢失失败。

## 9. 测试与验收矩阵

### 9.1 架构与身份

- 兼容高程扰动不改变权威 P3/P5；
- P2/P3/P4/P5 每个事实只有一个序列化所有者；
- P4 climate 与 P5 surface formation 是 bundle sibling，P5 不嵌套气候；
- P5 记录的终点 climate checkpoint 指纹等于 sibling P4 checkpoint 指纹；
- 最终 bundle 不含中间状态序列、接受/拒绝步或伪时间；
- 同 seed/spec 原生重复构建字节一致；WASM 量化事实一致。

### 9.2 数值与物理

- `f64` 子 ULP 真越界被拒绝，`f32` 假越界不再出现；
- 单一无补偿沉降率在有限物理时间内产生解析位移，而不是被要求收敛到零；
- 高程组成恒等式、Airy 加载/卸载和全部库存闭合；
- 一次完整生产构建只执行 start/endpoint 两次外层 P4 和一次完整 P5 推进；
- 最终 P4 forcing 指纹等于从最终地形/`SurfaceWaterGeometry` 重建的 forcing
  指纹；
- 终点诊断重算不推进 P5 时间或再次改变质量/水量库存；
- 活动构造格元允许非零当前速率，过程归因必须完整。
- `Standard`/seed `42` ignored/release 探针记录生产路径与高成本参考的原始最终态
  差值、既有质量、守恒和耗时，但不把差值变成新发布阈值。

### 9.3 产品与 UI

- seed `3/7/42` 及既有 17-seed 语料重跑；所有变化按因果链记录；
- P3/P5 地形包络只作为测量或既有门禁使用，不以输出重映射追值；
- UI 可触发构建、选择当前构造/侵蚀/沉积/高程组成字段并查看失败诊断；
- 用户验证：默认 Draft 构建完成、海沟不被科学裁剪、活动区保留非零当前速率，
  2D/3D 与字段检查器读取同一终态。

## 10. 非目标

- 不在本修订中新增深海压实、化学沉淀、俯冲沉积 sink 或经验盆地填充系数；
- 不新增用户年龄、高程上限或稳态容差旋钮；
- 不在本修订实现参数异质性、分布编辑 UI 或新的通用参数场 schema；§2.6 只冻结
  后期真实消费者必须满足的输入/所有权边界；
- 不发布地质历史、时间序列或恢复 checkpoint；
- 不把通用 engine 改造成循环数据流框架；
- 不保留无人消费的 trait、adapter 或双写 wire；
- 不通过 clamp、直方图重映射、放宽残差或 seed 特判制造通过结果。

## 11. 被否决方案

1. **继续固定今日强迫求绝对高程稳态。** 已由 `CellId(19366)` 的单一净沉降项
   证明并非普遍有根。
2. **只扩大高程域或在科学状态钳制。** 只能延后/隐藏持续净率，且引入未记账
   源汇。
3. **按“地质阶段”和“地貌阶段”各保留一份侵蚀/沉积。** 违反过程唯一所有权，
   使结果无法归因；时间尺度不同不构成重复方程的授权。
4. **把全部 128 个稠密历史状态发布给 P5。** 违反当前态产品裁定并显著扩大
   schema/缓存；私有协调器已经足够。
5. **修改通用 Stage 支持循环或多输出。** 当前只有自然形成链一个真实消费者，
   属于投机性通用化。
6. **生产中每个 `2 Myr` 宏步都执行 start/midpoint/endpoint P4。** 这是预测
   模拟器级的轨迹细化成本；最终态地图没有证据需要为 128 个宏步支付该成本。
7. **整次生成只求一个 P4，或以 start P4 直接发布。** 最后一次 P5 推进后会使
   发布气候 forcing 滞后于最终地形/水面几何；最小生产路径仍必须 endpoint
   closure。
8. **把 `2/1/0.5 Myr` step-doubling 和新造误差包络设为每次发布门。** 高成本
   参考应服务离线算法选择，不应把完整轨迹收敛升级为地图产品契约。
9. **增加独立“噪声阶段”改写最终高程、边界或年龄。** 参数异质性只能让原
   算法读取有出处的局部分布参数；中央事后抖动会破坏过程所有权与因果归属。

## 12. 每项承重技术的出处

- 球面程序板块与 `2 Myr` 离散步：Cortial, Peytavie, Galin & Guérin
  (2019), *Procedural Tectonic Planets*, Computer Graphics Forum 38(2),
  DOI `10.1111/cgf.13614`，§3 与 Appendix A 的 `δt = 2 My`。论文支持
  “有地质过程依据的程序近似”，不支持把 P2 宣称为预测性地球动力学，也未给出
  Sekai 的 `128` 步。
- `128` 步参考形成时域：Sekai 已批准的现有产品参数
  `EVOLUTION_STEP_COUNT`；本修订只将其提升为 resolved model identity，
  不伪装成文献常量。未来改变该值必须先测量并走显式规格/UI 修订。
- P5 `100,000 yr` coarse-grained formation horizon：沿用已批准的
  `2026-08-18-coupled-geomorphic-formation-p5-design.md` §5 产品参数，实施唯一
  事实源为 `SURFACE_FORMATION_HORIZON_YEARS`。它不是地球地貌达到稳态的文献
  常量，也不等于 P2 的 `256 Myr`；本修订只减少外层互调次数，不改变该时域。
- 生产 Lie-style 顺序分裂的数值依据：Trotter (1959), *On the Product of Semi-Groups of
  Operators*, DOI `10.1090/S0002-9939-1959-0108732-6`。它支持用顺序算子积近似
  同一演化问题，不为 Sekai 的总时域、误差或“两次外层 P4”提供现成常量。
- 过程耦合频率会改变离散结果，因而应离线测量而非假定轨迹已收敛：Santos,
  Caldwell & Bretherton (2021), *Cloud Process Coupling and Time Integration in
  the E3SM Atmosphere Model*, DOI `10.1029/2020MS002359`。该论文是顺序分裂
  与耦合频率敏感性的工业级气候模型证据，不直接给出 Sekai cadence。
- 地形—气候反馈存在但可按相异时间尺度异步耦合：Paik & Kim (2021),
  *Simulating the evolution of the topography–climate coupled system*, DOI
  `10.5194/hess-25-2459-2021`；Shen, Lynch, Poulsen & Yanites (2021),
  *A modeling framework (WRF-Landlab) for simulating orogen-scale
  climate-erosion coupling*, DOI `10.1016/j.cageo.2020.104625`。两者支持保留
  feedback 机制和离线敏感性检查，不要求地图产品逐宏步高频互调。
- 固定次数 predictor-corrector 与高成本迭代参考的类比：Schüller, Lemarié,
  Birken & Blayo (2025), *Quantifying coupling errors in atmosphere-ocean-sea
  ice models*, DOI `10.5194/gmd-18-9167-2025`，比较非迭代与迭代耦合并指出
  非光滑参数化可能妨碍迭代收敛；Strang (1968), DOI `10.1137/0705041`，只为
  离线 start/midpoint/endpoint 对称排序提供数值背景。本规格不据此声称 Sekai
  全系统二阶或必须迭代收敛。
- 球面相关参数场的数学依据：Lang & Schwab (2015), *Isotropic Gaussian Random
  Fields on the Sphere*, DOI `10.1214/14-AAP1067`；一般流形/三角网格上的 Matérn
  场与稀疏表示：Lindgren, Rue & Lindström (2011), DOI
  `10.1111/j.1467-9868.2011.00777.x`；有方向和频谱控制的程序场：Lagae et al.
  (2009), *Procedural Noise using Sparse Gabor Convolution*, ACM TOG 28(3)。
- 参数采样的确定性与子流正交沿用现有可复核实现
  `generators/natural/random.rs::LabeledSubstreams`：一次捕获 32-byte 根材料，
  以长度分帧的 BLAKE3 标签派生 `rand_chacha::ChaCha8Rng`。这只定义重放和模块
  隔离，不为任何物理参数的分布提供科学出处。
- 海床空间异质性的直接地学类比：Goff & Jordan (1988), *Stochastic Modeling of
  Seafloor Morphology*, DOI `10.1029/JB093iB11p13589`，使用带振幅、方向、特征
  波数和分形维的各向异性协方差。它不证明任意 P2/P3/P5 物理参数应采用同一
  分布；每个参数的边际/联合分布、相关尺度和因果映射仍须另找直接出处，否则
  作为开放问题交用户裁定。
- 基岩侵蚀、沉积输运与其他表面过程的组件化耦合：Shobe, Tucker & Barnhart
  (2017), *The SPACE 1.0 model*, GMD 10, DOI `10.5194/gmd-10-4577-2017`。
- 地质时间上的构造、陆地—海岸—海洋沉积演化：Salles, Ding & Brocard
  (2018), *pyBadlands*, PLOS ONE 13, DOI `10.1371/journal.pone.0195557`。
- generalized Exner 质量守恒与显式源汇边界：Paola & Voller (2005),
  DOI `10.1029/2004JF000274`。
- PTC 只用于求已经定义的稳态根：Kelley & Keyes (1998),
  *Convergence Analysis of Pseudo-Transient Continuation*, DOI
  `10.1137/S0036142996304796`。
- 浮点表示、舍入和误差身份：Goldberg (1991), *What Every Computer Scientist
  Should Know About Floating-Point Arithmetic*, DOI `10.1145/103162.103163`；
  Higham (2002), *Accuracy and Stability of Numerical Algorithms*, second
  edition。

上述来源支持机制和数值方法，不为 Sekai 新增任何未测量的门禁阈值、具体耦合
次数或参数分布。后续若需从生产 Lie-style 路径升级求解策略，或为某个物理参数引入
空间分布，必须先用生产算子与代表性离线参考测量，再以具名规格修订冻结其直接
出处、单位、支持域、适用范围和 UI 交付；不得用一个通用“自然感”系数代替。
