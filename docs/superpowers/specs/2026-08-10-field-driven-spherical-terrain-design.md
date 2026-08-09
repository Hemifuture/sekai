# Sekai 场驱动球面地形设计

日期：2026-08-10
状态：待书面评审
范围：修正球面新世界的板块、陆海与宏观高程形态；不改变球面呈现架构

## 1. 决策摘要

Sekai 保留现有权威球面 Voronoi–Delaunay 地表，把它严格限定为离散计算骨架：

- Voronoi 单元继续提供稳定 CellId、面积和有限体积控制体；
- Delaunay／邻接边继续提供 EdgeId、相邻关系、弧长、局部法向和输运通道；
- 二维地图与三维球体继续只呈现同一份球面自然事实；
- 单位球体继续完全不按高程变形。

板块、大陆与宏观地形不再由均匀最近种子距离直接决定。新的球面构造使用：

1. 分辨率无关的多尺度相关标量场；
2. 由结构势场隐式产生的方向偏好；
3. 带空间代价的多源到达时间；
4. 有界面积校准的广义 Voronoi 分区；
5. 独立的大陆亲和场和面积约束区域生长；
6. 现有板块运动、地幔、边界运动、侵蚀和水文对当前结果的组合。

所有过程在一次构建中直接生成唯一的当前状态。系统不保存历史切片，不逐时间步演化板块，也不通过隐藏的历史状态影响结果。

## 2. 问题与根因

当前球面板块通过均匀分散种子和共同测地距离的多源最近点分配生成。当前球面大陆通过全局核、到核距离和按典型单元尺度缩放的平滑噪声生成。

这套方法满足闭合球面、连通性、确定性和性能要求，但在规则、准均匀的球面网格上会显露出三个问题：

- 板块面积趋同，边界接近规则测地平分线；
- 大陆趋向若干圆形或椭圆形距离团；
- 形态扰动随单元尺度缩小，20k 网格上的宏观轮廓几乎不受影响。

旧平面版本的部分自然感实际来自不规则底层网格，而不是一个分辨率无关的形态模型。球面迁移保留 Voronoi 作为数值网格是正确的；把同一个均匀距离分区同时用作最终地质形态则职责过载。

## 3. 目标与非目标

### 3.1 目标

- 默认 12 板块具有明显但有界的面积差异、弯曲边界和不同长宽比；
- 大陆轮廓具有海湾、半岛、狭部、复合地块和尺度分层，不呈规则距离圆；
- 板块与大陆相关但不重合：大陆偏好稳定板块内部，也允许跨板块和被边界切割；
- 同一根种子、规格和地表得到逐位稳定的权威结果；
- 形态尺度以球面角尺度或真实距离定义，不随 cell count 改变；
- 20k 默认世界仍适合交互式地图设计工具；
- 保留现有输出类型、字段目录、球面展示、缓存身份和原子发布边界。

### 3.2 非目标

- 不做地质历史、板块重建、时间步进、历史切片或多时期浏览；
- 不做完整地幔对流、有限元地球动力学或高成本相场演化；
- 不为本次修改增加用户可见的高级形态滑杆；
- 不新增可序列化的形态场 Artifact；
- 不改变旧平面 LegacyPlanarV1 的算法、随机流或金图；
- 不按高程移动单位球体顶点；
- 不把二维噪声纹理包裹到球面。

## 4. 权威数据与职责分层

系统分成四层，每层只有一个职责：

1. **球面地表层**
   SphericalSurfaceSnapshot 是几何、拓扑、面积和实体身份的唯一事实源。

2. **形态构建层**
   多尺度场、边代价、到达时间、面积目标和区域生长是一次构建中的内部工作数据。它们可以丢弃，不序列化，不进入 UI，也不是第二份地表。

3. **自然事实层**
   SphericalTectonicSnapshot 继续发布板块、地壳、厚度、当前运动和边界事件。下游只依赖这个稳定契约，不读取形态构建内部。

4. **派生过程与呈现层**
   地幔、地形、地质、气候、水文、字段文档、二维投影和单位球渲染继续消费现有权威 Artifact。

形态字段是“生成自然事实的方法”，不是需要长期保留的产品事实。这样既能使用场驱动算法，也不会制造新的缓存、序列化或来源身份体系。

## 5. 多尺度球面场

### 5.1 连续采样

每个字段直接在权威单元中心的单位向量 p 上采样现有确定性三维 OpenSimplex 基函数。输入只使用 p 的三个分量、固定频带和独立标签种子，因此：

- 经线接缝和极点没有分支；
- 不依赖经纬投影；
- 同一球面位置在不同网格分辨率上具有相同连续值；
- 不引入新依赖。

每个 FieldRecipe 由少量频带组成。频带按球面角相关尺度定义，而不是按平滑遍数或单元数量定义：

| 频带 | 目标相关尺度 | 用途 |
|---|---:|---|
| macro | 70°–120° | 大板块趋势、主要大陆骨架 |
| meso | 25°–55° | 板块拉长、地块复合、海湾与半岛 |
| detail | 8°–20° | 海岸次级弯曲、小板块与岛链 |

detail 的最短有效尺度不得小于当前网格中位单元角直径的四倍。低分辨率世界自动省略无法采样的高频带，而不是产生混叠。

FieldBand 保存角尺度 theta、权重和独立 seed；采样坐标为 p / theta_rad 加一个由 seed 决定的三维偏移。第一版固定 recipe 为：

| 字段 | 角尺度与归一化权重 |
|---|---|
| plate resistance | 100°／0.55，42°／0.30，16°／0.15 |
| plate fabric | 75°／0.65，28°／0.35 |
| crust thickness | 36°／0.70，14°／0.30 |

大陆基础场按 formation preset 使用：

| 预设 | macro 105° | meso 38° | detail 13° |
|---|---:|---:|---:|
| Continents | 0.50 | 0.32 | 0.18 |
| Supercontinent | 0.65 | 0.25 | 0.10 |
| Archipelago | 0.25 | 0.45 | 0.30 |
| GreatIsland | 0.55 | 0.30 | 0.15 |
| VolcanicIslands | 0.10 | 0.40 | 0.50 |

### 5.2 统一归一化

所有标量场经过同一个 crate-private 流程：

1. 按单元真实面积计算加权均值；
2. 按面积计算加权方差；
3. 去均值并除以稳定尺度；
4. 截断到固定有限区间；
5. 量化为有序定点值。

板块阻力、结构势、大陆亲和和厚度变化只通过不同 FieldRecipe 配置这一个采样器。实现中禁止为每个消费者复制噪声循环、归一化或量化逻辑。

### 5.3 随机流正交

球面 V2 使用独立、版本化的标签流：

- plate-target-area-v2
- plate-seed-placement-v2
- plate-resistance-field-v2
- plate-fabric-field-v2
- crust-anchor-layout-v2
- crust-affinity-field-v2
- crust-thickness-field-v2

增加大陆细节频带不得改变板块目标面积、种子、运动或地幔流。旧平面标签保持原样。

## 6. 场驱动板块分区

### 6.1 板块面积目标

对 k 个板块生成 k 个有界正权重并归一化为全球面积：

- 单个原始权重限制在平均值的 0.45–2.40 倍；
- k 大于等于 8 时先生成一个从 0.55 到 1.90 的稳定秩序轮廓，再叠加不超过正负 20% 的标签随机扰动；归一化后仍保持最大／最小目标比不低于 2.75；
- 默认 12 板块因此具有明确大小差异，但禁止接近零面积板块；
- 权重只依赖 plate count 和 plate-target-area-v2；
- tectonic activity 只控制运动强度，不偷偷改变板块大小。

面积目标是构建约束，不加入公开 Plate 类型。最终真实面积从 cell ownership 和球面面积计算。

### 6.2 非均匀种子

种子仍从权威 CellId 中选择，以保证稳定身份和连通传播。选择过程不再简单地反复取全局最远点：

1. 按目标面积从大到小安置板块；
2. 用目标面积的平方根估计等效角半径；
3. 候选评分使用与已选种子的“距离／两者等效半径和”；
4. 加入小幅、独立的宏观种子偏好场；
5. 从仍满足分离约束的最高 5% 候选中，按稳定 CellId 排序后使用标签随机索引选择，而不是永远取唯一最大值。

该策略保留必要的分离度，但不制造规则、近等距的种子格局。

### 6.3 空间代价

为每条权威邻接边 e 构造一个共同的正代价：

    cost(e) = length(e) × clamp(
        1 + 0.45 × resistance(e) + 0.35 × fabric_crossing(e),
        0.45,
        2.20
    )

其中：

- length 是球面弧长的既有量化值；
- resistance 是边两端板块阻力场的均值，范围为 -1..1；
- fabric_crossing 是结构势场沿边的绝对方向导数，再用全局面积／长度加权 RMS 归一化到 0..1；
- 最终代价量化为严格正 u64。

低阻力区域传播更快；跨越结构势陡坡更贵，所以传播倾向沿结构走向展开，分区边界倾向停在势垒附近。结构方向由同一标量势场的球面梯度隐式给出，不再存一份容易漂移的重复向量场。

当两个场系数为零、所有源偏置相同时，该算法严格退化为当前共同测地距离 Voronoi。这为回归和故障定位提供清晰基线。

### 6.4 到达时间与面积校准

使用共同边代价执行带源初始势的多源 Dijkstra：

    owner(x) = argmin_i [bias_i + distance_metric(seed_i, x)]

所有 bias 统一平移到非负范围，不改变归属。稳定比较顺序为：

1. 总到达代价；
2. PlateId；
3. CellId。

令 S 为种子间最近 metric 距离的中位数。面积校准最多进行六次：

1. 计算当前每板块真实球面面积；
2. 令相对误差 e_i = (实际面积－目标面积)／目标面积；
3. 使用 delta_bias_i = clamp(0.35 × S × e_i, -0.12 × S, 0.12 × S) 更新，并把总 bias 限制在 -0.60 × S..0.60 × S；
4. 若任何种子不再属于自身，拒绝本轮 bias；
5. 保存最大相对误差最小的有效分区；
6. 连续两轮的最大相对误差改善小于 0.005 时提前结束。

这是球面 power/Laguerre 分区的高效图近似。它只要求面积接近目标，不为追求数学精确而运行无界优化。

共同正度量下，加性到达时间区域沿至少一条到种子的最短路连通；实现仍必须显式验证所有板块非空、种子归属和图连通。校准未达到审美目标时使用“最佳有效轮次”，不得静默回退到旧均匀 Voronoi。

## 7. 独立大陆亲和场

### 7.1 与板块解耦

大陆不是板块的子集，也不等于一个或多个板块。大陆亲和分数由以下正交分量组合：

    affinity =
        preset_field_recipe
      + 0.35 × clustered_anchor_influence
      + 0.15 × soft_plate_interior_preference

plate interior preference 是到最近板块边界的 metric 距离除以该板块目标等效半径，并截断到 0..1。它只占较小权重，降低大量海岸恰好贴着板块边界的概率，同时仍允许大陆跨越多个板块。大陆生成不读取板块速度、地幔或最终边界分类，避免循环依赖。

### 7.2 当前态地块簇

为得到复合大陆而不模拟历史，每个主要大陆由若干静态“形态叶片”组成。叶片只是一次构建中的影响核，不是历史 terrane 实体。

| 形成预设 | 主要簇 | 每簇叶片 | 岛屿面积预算 |
|---|---:|---:|---:|
| Continents | 4 | 2–4 | 8%–18% |
| Supercontinent | 1 | 6–9 | 5%–10% |
| Archipelago | 3 | 2–3 | 35%–55% |
| GreatIsland | 1 | 3–5 | 15%–25% |
| VolcanicIslands | 0 | 0 | 100% |

叶片中心根据 crust-anchor-layout-v2 在球面上保持物理间距，并沿局部结构势变化最小的邻接方向偏移。每个叶片使用 q = distance／support_radius 和紧支撑 Wendland C2 核：

    kernel(q) = (1 - q)^4 × (4q + 1),  0 <= q < 1
    kernel(q) = 0,                     q >= 1

support_radius 由该簇面积预算的等面积球面圆半径确定，并限制为该半径的 0.55–0.90 倍。多个叶片与连续场叠加后产生海湾、半岛、狭部和复合轮廓，而不是一个核对应一个圆形大陆。

### 7.3 面积约束区域选择

陆地比例继续严格服从 TectonicSpec.continental_crust_fraction。统一 AreaConstrainedMaskBuilder 负责：

1. 从主要簇种子按 affinity 最大优先队列向邻接单元生长；
2. 为每个主要簇保留预设面积预算和至少一个连通分量；
3. 从分离的局部高点生长岛屿预算；
4. 删除小于该预设最小岛屿面积的噪点，除非该分量属于 VolcanicIslands；
5. 填补小于物理面积上限的内陆孔洞；
6. 沿当前海岸按 affinity 顺序增删单元，恢复最接近目标的面积前缀；
7. 海岸收缩前按批次计算受保护分量的割点，只从非割点海岸叶片中删除；每批删除后重新计算，不做逐 cell 全图搜索。

所有阈值以全球面积比例、平方米或球面角尺度表示，不以“几个 cell”表示。最终误差不得超过一个权威单元面积。

第一版最小岛屿面积分别取全球面积的 Continents 0.05%、Supercontinent 0.05%、Archipelago 0.015%、GreatIsland 0.025%；VolcanicIslands 允许一个单元。实际阈值取该比例和当前最小单元面积的较大者。可填补孔洞的面积上限为相应最小岛屿面积的两倍。

### 7.4 地壳厚度

CrustKind 确定后，厚度继续遵守现有大陆／海洋物理范围。变化项改为独立的 crust-thickness-field-v2：

- 大陆厚度受低频相关场和到海岸的归一化距离调制；
- 海洋厚度使用独立中尺度场；
- 场振幅被现有最小／最大厚度常量夹紧；
- 不复用大陆亲和字段，避免“海岸形状”和“厚度纹理”成为同一随机图的重复表现。

## 8. 当前态高程组合

本次不重写完整 relief、hydrology 或 erosion 流水线。新的地壳和板块输出进入现有当前态组合：

    elevation =
        crustal_freeboard
      + boundary_kinematic_response
      + mantle_and_hotspot_response
      + regional_relief
      + hydro_erosion_adjustment

只做两类必要校准：

- 把仍按典型单元尺度定义的球面区域起伏改为角尺度／真实距离尺度；
- 确保 plate boundary influence、regional relief 和 island relief 调用同一通用距离传播原语，不各自实现近似 Dijkstra。

地形结果仍是每单元高度标量。二维地图按字段着色；三维球只在单位球面上着色，不使用高度顶点位移。

## 9. 代码模块设计

### 9.1 通用形态原语

新增 crate-private 目录：

    src/generators/natural/morphology/
        mod.rs
        field.rs
        metric.rs
        arrival.rs
        area.rs

职责如下：

- field.rs
  定义 FieldRecipe、FieldBand、QuantizedScalarField 和唯一的球面采样／面积归一化／量化实现。它不知道板块、大陆、地形或 UI。

- metric.rs
  把 NaturalTopologyIndex、阻力场和结构势场变成 PositiveEdgeMetric。它只保证长度对齐、严格正代价和确定性，不知道 owner 或 PlateId。

- arrival.rs
  提供单源距离、多源到达时间、源偏置和稳定 tie-break。旧的共同距离函数可委托给这里的统一核心；LegacyPlanarV1 的输出由无偏置、原边长兼容入口冻结。

- area.rs
  提供真实面积统计、最接近面积前缀、连通分量、受保护区域生长、孔洞清理和海岸再平衡。它只处理 CellId、邻接、分数和面积权重，不知道 CrustKind。

这些类型不导出到 crate 外，不实现 serde，不进入 Artifact。通用模块不得依赖 spherical_tectonics、relief、stage、app、view 或 gpu。

### 9.2 球面板块领域模块

保留 src/generators/natural/spherical_tectonics.rs 作为薄 facade，并增加：

    src/generators/natural/spherical_tectonics/
        plates.rs
        crust.rs
        motion.rs
        boundaries.rs

- plates.rs
  只生成目标面积、种子和 PlateIdField。输入为已验证球面 domain、TectonicSpec 和板块专用随机流。

- crust.rs
  只生成 CrustKindField 和厚度。输入为 domain、PlateIdField、formation preset、目标陆地比例和地壳专用随机流。

- motion.rs
  保留每板块 Euler 极刚性运动和相邻板块相对速度选择。它不读取大陆亲和场或厚度噪声。

- boundaries.rs
  组合 owner、Euler 运动和最终地壳类型，生成现有 BoundaryRecord 与 SphericalBoundarySegment。

- spherical_tectonics.rs
  只做输入验证、按顺序调用四个模块、组装并验证 SphericalTectonicSnapshot。

任何模块都不得调用 Stage、写缓存或发布 Artifact。SphericalTectonicStage 仍是唯一的引擎适配器。

### 9.3 依赖方向

允许的依赖方向只有：

    world ids / spatial contracts
              ↓
        topology + morphology primitives
              ↓
        spherical tectonic domain modules
              ↓
        snapshot assembly
              ↓
        typed stage / downstream artifacts

禁止：

- morphology 反向依赖 tectonics；
- crust 读取 motion 的随机流；
- relief 读取内部 FieldRecipe；
- stage 内复制科学算法；
- app 或 renderer 直接读取形态构建数据；
- 为测试公开原本私有的重组入口。

### 9.4 DRY 的精确定义

本设计中的 DRY 是“一个稳定概念只有一个实现”，不是“不同领域必须共用同一策略”：

- 球面场采样、归一化和量化只有一套；
- 图距离／到达时间堆算法只有一套；
- 面积前缀、分量和区域生长只有一套；
- 板块策略与大陆策略分别存在，因为语义不同；
- LegacyPlanarV1 与球面 V2 可以有不同策略，但复用不会改变旧输出的纯原语；
- 不建立包含 planar/spherical 分支的万能生成器。

## 10. 数据流、生命周期与缓存

单次球面 tectonic 构建顺序：

1. 验证 spec、formation、surface identity；
2. 借用 surface 构建一个 NaturalTopologyIndex；
3. 构建板块字段、metric 和 partition；
4. 丢弃板块字段和 metric 工作缓冲；
5. 独立生成 Euler motion；
6. 构建大陆亲和、mask 和 thickness；
7. 丢弃大陆字段和区域生长工作缓冲；
8. 组合 boundary records／segments；
9. 构造并完整验证一个 SphericalTectonicSnapshot；
10. Stage 成功后按现有引擎语义原子发布。

多轮到达时间复用一个 ArrivalWorkspace 的 distances、owners、heap 和面积统计缓冲，不为每轮保留快照。内存峰值只包含当前阶段需要的场。

SphericalTectonicStage 的版本从 1 递增到 2，以使 tectonic 及所有下游球面自然缓存正确失效。SphericalSurfaceArtifact、resolved specs 和 formation 在输入未变时继续命中。输出数据布局不变，因此 SphericalTectonicSnapshot 的 wire schema 不因算法升级而增加无意义版本。

## 11. 错误与失败语义

新增内部错误至少区分：

- 字段 recipe 非法或采样得到非有限值；
- edge metric 非正、溢出或 cardinality 不匹配；
- plate count 超过可用单元；
- 种子重复、丢失自身归属或板块为空；
- plate ownership 不连通；
- continental target 在约束下不可满足；
- 最终 crust 面积超过一个单元误差；
- 构造结果与 SurfaceRef／cardinality 不匹配。

所有错误在 SphericalTectonicSnapshot 创建和 Stage 发布前返回。失败不产生半完成 snapshot，不复用旧世界的内部缓冲，不回退到平面算法，也不回退到旧均匀球面 Voronoi。

## 12. 正确性与形态验收

测试不能只冻结哈希。哈希之前必须通过语义和形态 oracle。

形态指标统一定义如下：

- 等面积圆归一化周长：实际共享边弧长总和，除以球面上具有相同面积的圆形帽周长；
- 主轴长宽比：把 component 的面积权重点投影到其面积质心切平面，取二维协方差两个特征值平方根之比；
- 边界径向变异：component 边界点到其球面面积质心的大地距离之变异系数；
- 主要陆块：面积至少占全部大陆面积 10% 的连通分量；
- 板块／海岸重合率：处于一个中位 cell 角直径缓冲区内的海岸弧长比例。

### 12.1 基础不变量

- 每个 cell 恰有一个有效 PlateId 和 CrustKind；
- 每个板块非空、包含自己的 seed、图连通；
- 每条 plate boundary 的两个 owner 不同；
- continental fraction 与目标之差不超过一个 cell 面积；
- 经线接缝、两极和旋转后的统计无特殊异常；
- 同 seed/spec/surface 逐位相同，不同 seed 产生实质不同形态；
- LegacyPlanarV1 金图逐位不变。

### 12.2 板块形态矩阵

默认 12 板块、至少 16 个根种子上检查：

- 最大／最小板块面积比不低于 2.5，不高于 8；
- 无板块小于全球面积的 1%；
- 面积变异系数位于 0.30–0.75；
- 每板块实际面积相对其目标面积的最大误差不超过 35%；
- 等面积圆归一化周长的中位数大于 1.15、小于 2.60；
- 至少半数板块的主轴长宽比大于 1.25；
- 去除 field influence 的 deliberate mutation 必须让至少一项形态 oracle 变红。

这些区间约束“明显不规则但不碎裂”，不要求模拟地球的精确统计。

### 12.3 大陆形态矩阵

每个 formation preset、至少 16 个根种子上检查：

- 主要陆块数分别为 Continents 3–5、Supercontinent 1、Archipelago 2–6、GreatIsland 1、VolcanicIslands 0–2；
- 非 VolcanicIslands 不出现低于物理面积下限的噪点；
- Continents 和 Supercontinent 的归一化海岸周长中位数大于 1.35、小于 3.50；
- 默认 Continents 至少一个主要陆块的边界径向变异大于 0.18；
- 默认 Continents 的海岸与板块边界重合率保持在 10%–55%，既不锁死为板块形状，也不完全失去构造关联；
- 把大陆亲和替换为纯到核距离的 mutation 必须被形态 oracle 捕获。

### 12.4 分辨率不变量

对同一 seed/spec 的约 5k 与 20k 球面比较面积加权统计：

- plate area distribution、主要大陆数和 land fraction 保持；
- 归一化板块周长和海岸周长相差不超过 15%；
- 结构尺度不会随 cell count 增加而缩成单元级噪声；
- 把 20k 单元中心最近邻重采样到 5k 后，经最优 PlateId 匹配的 plate owner 面积加权一致率不低于 65%；
- 同样重采样后的大陆 mask 面积加权 Jaccard 不低于 75%。

不要求 CellId 一一相同，因为两个地表本来具有不同 SurfaceRef。

### 12.5 视觉验收

生成固定 seed 42 以及不少于 11 个额外种子的 20k 图册：

- plate ownership；
- crust kind；
- crust thickness；
- elevation；
- boundary kind；
- 单位球和 Equal Earth 地图各一份。

先通过数值 oracle，再由人工检查是否存在规则圆团、等距拼块、极点聚集、接缝、棋盘碎片或单元尺度锯齿。视觉图册是验收证据，不作为第二套算法或产品资源。

## 13. 性能与内存预算

设 V 为 cell 数、E 为邻接边数、I 为面积校准轮数，I 固定不超过 6：

- 场采样为 O(V)；
- metric 构造为 O(E)；
- 每轮多源传播为 O(E log V)；
- 区域生长和分量处理为 O(E log V)；
- 不存在与历史长度、板块对数或像素分辨率相乘的状态。

20,252 cell、默认 seed 42 的 Release 门槛：

- 新 spherical tectonics 阶段不超过 300 ms；
- 完整 spherical natural graph 不超过同机修改前基线的 1.25 倍，并继续满足既有 5 s 绝对门槛；
- 形态临时工作集不超过 64 MiB；
- 不增加常驻 presentation 内存；
- 相机、投影、字段切换和动画 phase 不触发任何形态重算。

若精确三角曲面 Fast Marching 超出预算，正式实现采用现有邻接图上的量化加权多源 Dijkstra。它是本产品明确接受的高效近似，不作为临时降级路径。

## 14. 实施边界

本次实现预期只修改：

- crate-private morphology primitives；
- spherical tectonics 的 partition、crust、motion／boundary 组织；
- 必要的 spherical regional relief 尺度校准；
- SphericalTectonicStage 版本和相关测试／性能／金图。

不修改：

- 球面 SurfaceRef 和网格生成；
- 2D/3D presenter 与 GPU shader；
- field catalog 的 36 个稳定字段；
- camera、picking、projection、dynamic arrow；
- legacy planar generator；
- app 的 publication lineage。

## 15. 研究依据与产品取舍

- Engwirda, JIGSAW-GEO：球面 Voronoi–Delaunay 网格适合大气、海洋和有限体积计算。Sekai 因此保留 Voronoi 作为数值骨架。
  https://gmd.copernicus.org/articles/10/2117/2017/index.html

- Cortial et al., Procedural Tectonic Planets：球面 Voronoi 只是初始板块，真实形态来自后续相互作用、地块、抬升与细化。Sekai 不复制历史演化，而直接近似其当前态输出。
  https://onlinelibrary.wiley.com/doi/10.1111/cgf.13614

- Kimmel and Sethian, Computing Geodesic Paths on Manifolds：三角曲面上的 Fast Marching 为到达时间场提供 O(M log M) 基础。Sekai 使用量化图传播作为更便宜、确定性更强的近似。
  https://www.cis.upenn.edu/~cis6100/Kimmel-Sethian-geodesics-98.pdf

- Cui et al., Spherical Optimal Transportation：球面 power diagram 可以通过源权重控制区域质量／面积。Sekai 使用六轮有界 bias 校准近似这个思想，不引入无界凸优化。
  https://www.sciencedirect.com/science/article/pii/S0010448519302003

- Lang and Schwab, Isotropic Gaussian Random Fields on the Sphere：球面相关场可用频谱和快速近似表达。Sekai 使用现有三维确定性噪声构造有限频带近似，不引入 SPDE 求解器。
  https://arxiv.org/abs/1305.1170

- Cordonnier et al., Large Scale Terrain Generation from Tectonic Uplift and Fluvial Erosion：抬升场与河网侵蚀能以较低成本生成可信大尺度地形。Sekai 保留当前态 uplift／hydrology 组合，不追求完整物理历史。
  https://onlinelibrary.wiley.com/doi/10.1111/cgf.12820

## 16. 完成定义

只有同时满足以下条件，才视为修复完成：

1. Voronoi 仅承担地表离散和广义传播的退化基线，不再直接决定最终板块／大陆形状；
2. 字段、metric、arrival 和 area 原语各自只有一个 crate-private 实现；
3. 板块、地壳、运动和边界模块依赖方向单向、随机流正交；
4. 旧平面结果不漂移；
5. 数值、形态、分辨率、性能、内存和视觉验收全部通过；
6. 单位球体无高程位移，动态向量仍只通过箭头／标注呈现；
7. 最终仍只发布一个当前球面自然结果，不产生历史或第二事实源。
