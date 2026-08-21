# Sekai 程序化球面构造与初步高度图设计

状态：已批准

日期：2026-08-10

范围：替换球面新世界的板块、地壳演化与初步高度图算法；不改变闭合球面事实源、二维/三维呈现、单位球渲染或旧平面兼容域。

本设计取代《2026-08-10-field-driven-spherical-terrain-design》中的最终板块分区、大陆区域生长和对应高度组合。该文档关于闭合球面、稳定实体、无投影物理、无高度顶点位移、确定性、原子发布和模块正交的约束继续有效。

## 1. 决策摘要

Sekai 采用 Cortial、Peytavie、Galin 与 Guérin 在 *Procedural Tectonic Planets*（Computer Graphics Forum 2019，DOI `10.1111/cgf.13614`）中提出的球面程序化构造模型作为板块与粗地壳的权威算法依据。

核心决策如下：

1. 现有 `SphericalSurfaceSnapshot` 继续是几何、拓扑、面积、邻接和稳定实体身份的唯一事实源。
2. 球面 Voronoi–Delaunay 继续作为数值骨架，并只承担初始板块分区；最终板块和大陆形态来自板块刚体运动、重采样、俯冲、碰撞、地块增生、海底扩张和裂谷。
3. 生成期允许有限离散时间迭代，但只保存当前/下一双缓冲；不发布、序列化或保留历史切片。
4. 构造阶段发布唯一的最终当前地壳状态；初步高度图由该状态正向生成。
5. 噪声只扰动成熟模型的初始条件、参数和定向细节，不取代构造因果。
6. 三维球体始终为单位球；高程只作为字段和着色数据，不用于顶点位移。
7. 旧 field-weighted shortest-arrival、面积校准 power/Laguerre 分区和大陆亲和区域生长不再作为球面生产算法，也不保留为静默回退。
8. 完整地貌细化后续采用 Cordonnier 等人的 uplift + stream-power 方法；本阶段完成可信的构造粗高程。

## 2. 问题与根因

现有实现虽使用多个球面场，但最终板块仍由共享正度量上的多源最短到达时间决定，并以源偏置校准面积。其宏观结果仍是加权球面 Voronoi／power diagram：

- 近似均匀的种子形成规则蜂窝骨架；
- 共享 metric 只能局部扭曲平分线，不能产生板块独立运动和碰撞形变；
- 大陆区域生长使用径向核心、强凝聚和精确面积预算，形成紧凑圆团；
- 大陆壳与洋壳的基础高差远大于区域扰动，海岸线近似复制离散地壳掩码；
- 既有周长、长宽比和 `owners != baseline` 测试允许“宏观六边形、局部锯齿”的假阳性。

问题不在球面化本身。闭合球面对于全球板块运动、气候、季风、洋流、水文、面积守恒以及二维/三维共用事实仍是必要前提。错误是把数值网格兼任了最终地质形态。

## 3. 研究依据与方案比较

### 3.1 采用：Procedural Tectonic Planets

该方法原生工作在球面上，以低分辨率带属性地壳样本和球面三角网表示板块，使用刚体测地运动及现象学传递函数近似：

- 洋壳／大陆壳与洋壳／洋壳俯冲；
- 大陆碰撞、地块拼贴和褶皱抬升；
- 张裂边界的新洋壳与洋中脊；
- 板块裂谷；
- 大陆侵蚀、洋壳年龄沉降和海沟沉积；
- 基于构造方向的程序化地貌放大。

论文面向虚拟世界创作而非完整地球物理预测，能在交互速度与视觉可信度之间取舍，且经过用户研究和地质专家评估，符合 Sekai 的地图设计定位。

### 3.2 不采用：PlaTec 直接移植

PlaTec／WorldEngine 是成熟的开源高度图路线，但核心基于周期平面栅格，运动、包围盒和碰撞均依赖环面坐标；直接移植会重写关键语义。此外 PlaTec 为 LGPL-3.0。它仅作为结果与行为对照，不复制代码，不作为依赖。

### 3.3 不采用：Power Watershed 作为板块核心

Power Watershed 是成熟的图分割方法，可用于静态区域分区，但不产生俯冲、碰撞、海底扩张、地块增生或洋壳年龄。把这些现象拼接到分割结果上会重新发明板块模型，因此不采用。

### 3.4 后续采用：构造抬升与河流侵蚀

初步构造高度图稳定后，后续 relief 阶段采用 Cordonnier 等人 2016 年的 *Large Scale Terrain Generation from Tectonic Uplift and Fluvial Erosion*，以当前 uplift forcing 和 stream-power 河网生成更细的山脊、流域与河谷。本设计不提前实现该后续阶段。

## 4. 目标与非目标

### 4.1 目标

- 默认世界不再显露规则球面 Voronoi 板块；
- 板块边界具有碰撞、张裂、走滑和裂谷造成的不同形态；
- 大陆由可移动和可增生的大陆地块形成，而不是按板块或径向核心阈值切出；
- 海沟、岛弧、沿海山系、碰撞山系和洋中脊具有正确的相对位置；
- 洋壳年龄与深度负相关；大陆碰撞与抬升正相关；
- 同 seed/spec 原生与 WASM 产生相同量化事实；
- 默认约 20k 单元在地图设计可接受的等待时间内完成；
- 保留现有下游快照边界和二维/三维同源原则。

### 4.2 非目标

- 不预测真实地球历史；
- 不实现地幔对流 PDE、ASPECT 类求解器或完整岩石圈流变；
- 不保存时间轴、关键帧、历史 plate geometry 或多时期 UI；
- 不在第一阶段实现完整河流侵蚀、沉积循环、冰川或生态；
- 不以高程改变球体几何；
- 不改变 LegacyPlanarV1 的输出、随机流或存档兼容性；
- 不复制 Cortial 私有代码，也不引入 PlaTec 源码。

## 5. 权威状态与生命周期

### 5.1 输入

生产入口继续只接收：

- 已验证的 `SphericalSurfaceSnapshot`；
- 已解析的 `TectonicSpec`；
- 已解析的 `ResolvedWorldFormation`；
- `StageRng` 派生的稳定标签随机流。

`TectonicSpec::plate_count` 明确表示初始板块数。裂谷、板块消亡和地块转移可使最终活动板块数改变，但最终必须保持在 `2..=MAX_SPHERICAL_PLATES`。UI 文案改为“初始板块数”。最终字段目录使用快照中的实际活动板块数，不使用输入数猜测。

`ResolvedWorldFormation` 是初始条件与过程强度的作者意图，不是最终拓扑合同。它只能选择同一套 PlaTec 式相干噪声初始化的尺度谱、既有目标大陆比例，以及 Cortial 过程常数的有界倍率：

| Formation | 初始大陆性尺度谱 | 既有目标大陆比例 | 允许的过程偏置 |
| --- | --- | --- | --- |
| Continents | 低频与中频平衡 | 0.38 | 论文基准 |
| Supercontinent | 最低频占主导 | 0.42 | 裂谷率下调、碰撞保持基准 |
| Archipelago | 中频占主导并保留少量低频 | 0.26 | 裂谷率与俯冲岛弧响应上调 |
| GreatIsland | 低频占主导但带弱中频 | 0.28 | 裂谷率下调 |
| VolcanicIslands | 海洋占主导的中频谱 | 0.16 | 洋壳俯冲岛弧与既有热点响应上调 |

所有倍率都在对应论文／既有产品参数的显式有限区间内冻结，不能改变方程、事件方向、处理顺序或守恒关系。预设不要求最终恰有 1、3 或 5 个大陆连通分量，不做连通块裁剪、径向核生长、面积回填或事后海岸塑形。测试验证不同预设在多 seed 分布上的尺度、面积和构造响应排序，而不是强迫每个 seed 命中固定大陆数。

### 5.2 生成期状态

`TectonicWorkspace` 是 crate-private、不可序列化的临时状态，只包含：

- 当前与下一组地壳样本；
- 活动板块及其内部 lineage ID；
- 每板块刚体旋转；
- 当前接触、重叠、空隙和地块分量；
- 重采样、距离查询和事件处理的复用 scratch；
- 固定迭代计数和确定性事件序列。

每一步完成后交换 current/next；被覆盖的一步立即消失。不得存储 `Vec<TectonicState>`、历史 Arc、检查点或 UI 可见中间状态。

### 5.3 最终当前态

生成结束后先在权威 cell 邻接图上规范化活动域，再压缩和重编号活动板块，构造 `SphericalTectonicSnapshot` schema V3：

- 删除已经没有任何当前地壳样本的内部 lineage；
- 同一 lineage 若在最终 owner 图上形成多个不连通分量，则每个分量发布为独立活动板块；
- 分裂后的板块继承同一物理旋转与来源 lineage，但获得独立连续 `PlateId`；
- 每个最终板块先求单位方向的面积加权三维均值，再选择与该方向大圆距离最小的自有 cell 作为代表，并以最低 `CellId` 稳定打破平局；若均值方向退化则直接选择最低自有 `CellId`；
- 规范化后若活动板块少于 2 或超过 `MAX_SPHERICAL_PLATES`，候选构建以 typed error 失败，不合并遥远分量来凑容量。

该步骤只是把最终拓扑事实规范化为现有快照的“每个 PlateId 连通”合同，不改变地壳材料、海岸或高程，也不是第二套板块算法。快照除现有字段外，增加一个正交的密集 `SphericalCrustState`：

- `kind`；
- `thickness_km`；
- `age_myr`，大陆壳使用规范化哨兵语义；
- `tectonic_elevation_m`；
- `lineation_east` 与 `lineation_north`，表示洋脊或褶皱方向；
- `orogeny_kind`：None、Andean、Himalayan；
- `orogeny_age_myr`。

这些是最终当前地壳的材料属性，不是历史切片。公开 getter 只读；序列化严格、定长、有限值且绑定 `SurfaceRef`。

## 6. 算法映射

### 6.1 初始采样与板块

1. 权威球面始终是唯一发布的地表。权威表面不高于 5,000 cells 时直接作为演化采样；更大表面使用目标 5,000、当前实际 4,842 cells 的瞬态 geodesic control surface。该控制球面只服务一次构造，既不发布、序列化、缓存，也不形成历史切片。
2. 按 `plate_count` 在演化球面选择初始球面质心并构造球面 Voronoi 初始板块。Voronoi 只提供初始材料分区，不是最终板块形状。
3. 按论文使用低频球面连续噪声有界扰动到质心的测地距离，使初始边界不完全规则。
4. 初始化大陆地块和洋壳。这里采用 PlaTec 已验证的 coherent-noise 初始岩石圈与 sea-level quantile 配方，把原平面周期坐标采样改为单位球方向上的 3D OpenSimplex 采样；`ResolvedWorldFormation` 只选择上一节定义的频谱、quantile 和有界过程倍率。该场只决定初始大陆性，不直接定义最终陆地，也不按连通块数量修剪结果。
5. 为每个板块生成一个球心旋转轴和角速度；速度满足 `s(p) = omega * (w × p)`。

初始 Voronoi 仅存在于 workspace。最终快照不得从初始 owner 表直接构造。

### 6.2 刚体运动与重采样

每个离散步使用论文默认 `delta_t = 2 My`：

1. 用板块四元数旋转其带属性样本；
2. 检测移动三角／样本对当前演化球面控制体的覆盖；
3. 将多重覆盖记录为接触候选，将未覆盖记录为张裂空隙；
4. 按论文以最大板块位移决定全局重采样间隔，限制在 10–60 步；
5. 重采样只由 `resample` 模块执行，使用球面重心插值和稳定最近实体 tie-break；
6. 每次中间重采样后使用 volume-preserving graph MBO：固定三次有界 Jacobi graph-heat，再按原占据球面面积阈值化材料类型。它只清除网格尺度混叠，不保留材料历史，也不把 owner 恢复成新 Voronoi；
7. owner 使用移动样本证据项加 marker-controlled watershed／Potts 正则重建连通域；张裂填隙按 incident divergence 和当前 anchor 建 O(cells + events) 索引，正常路径不做 gap × world 全局扫描；
8. 若使用瞬态控制球面，只在 128 步全部结束后把最终当前状态一次投影到权威球面：owner、crust kind、orogeny 使用稳定最近样本，厚度、年龄、高程和 lineation 等连续量仅在兼容 owner/kind/orogeny 内做球面三角面积重心插值；
9. 最终在权威邻接图上规范化 plate lineage，并用多源 graph distance 生成被动大陆边缘的 shelf/slope 剖面；
10. 所有位置重新归一化到单位球面，不携带高度位移。

默认运行 128 步，即 256 My 的生成期演化。该常量是产品默认，不进入存档；未来如需质量档位必须另行设计，第一版只有一个正式路径。

### 6.3 接触分类

`contacts` 只根据球面局部切向基、相邻板块相对速度、地壳类型、年龄和覆盖深度生成事件：

- convergence；
- divergence；
- transform；
- oceanic–continental subduction；
- oceanic–oceanic subduction；
- continental collision；
- rift candidate。

接触检测不写高度，不改变 owner，不生成噪声。

### 6.4 俯冲

遵循论文 4.1：

- 洋壳遇大陆壳时洋壳俯冲；
- 两侧均为洋壳时较老洋壳俯冲；
- 小型大陆地块可形成 forced subduction，随后增生；
- 海沟位于俯冲侧，抬升位于上覆侧；
- 抬升按论文的距离、相对速度和已有高程传递函数计算；
- slab pull 只修改对应板块的当前旋转状态；
- 噪声只能有界调制基准强度，不能翻转俯冲侧或事件类别。

### 6.5 大陆碰撞与地块增生

遵循论文 4.2：

- 大陆连通分量作为 terrane；
- 达到论文定义的交叠条件后，地块从原板块转移到接收板块；
- 碰撞区抬升取决于地块面积、接触深度和相对速度；
- 保存当前褶皱方向、Himalayan orogeny 类型与年龄；
- 小地块增生可形成半岛、岛弧拼贴和不规则大陆边缘。

地块转移是当前状态所有权变化，不保留转移历史。

### 6.6 海底扩张

遵循论文 4.3：

- 张裂空隙生成新的年轻洋壳；
- 高程由相邻板块插值与模板洋脊剖面混合；
- 记录洋脊切向方向和年龄零点；
- 洋壳随年龄按论文阻尼下沉；
- transform fault 细节只在最终放大阶段使用定向 Gabor noise。

### 6.7 裂谷

遵循论文 4.4 的 Poisson 事件模型和大陆比例／面积影响：

- 只对满足面积和大陆比例条件的活动板块触发；
- 分裂为 2–4 个子板块；
- 裂谷线使用论文的扰动 Voronoi fracture；
- 子板块获得发散旋转；
- 达到 `MAX_SPHERICAL_PLATES` 时停止触发新裂谷，不覆盖或复用活动 ID。

裂谷发生后的大陆伸展使用 McKenzie (1978) 均匀纯剪切模型的有界工程近似，而不是在最终高度图上直接绘制低地：

- 每个 2 Myr 步长只记录每个当前地壳样本最强的发散法向速度；
- 以 400 km 有效裂谷带宽把本步伸展位移换算为 `β = 1 + extension / width`，并把单步 `β` 限制在 `1.0..=1.2`；
- 大陆地壳厚度更新为 `thickness / β`，再受公开的最小大陆地壳厚度约束；
- 高程只写回当前地壳构造状态，变化量由共享 Airy 均衡函数对新旧厚度求差；
- 重复张裂可以继续变薄和沉降，但不保存每步历史，也不在 relief 阶段追加裂谷形状。

400 km 和单步上限是面向地图设计工具的稳定、高效近似参数：它保留“张裂 → 变薄 → 均衡沉降”的成熟因果链，同时避免短时间步把大陆壳一次耗尽。

内部 lineage ID 可稀疏；发布前按稳定来源顺序和最终连通分量的代表 cell 重编号为连续 `PlateId`。最终连通化只在候选装配阶段执行一次，不能在每个时间步反复拆分 identity。

### 6.8 大陆侵蚀、洋壳阻尼与沉积

遵循论文 4.5 和 Appendix A：

- 大陆粗高程应用论文的线性侵蚀项；
- 洋壳随年龄／当前高程应用阻尼；
- 海沟应用沉积填充；
- 海平面为 0 m；
- 首版常数以论文 Appendix A 为基准，仅允许形成预设做已记录的有界倍率。

这一步是粗地壳松弛，不替代后续 stream-power relief。

## 7. 噪声契约

### 7.1 唯一实现

通用 `morphology::noise` 是唯一球面噪声入口，包装仓库已有 `noise` crate 的成熟 3D coherent-noise 实现，并实现论文引用的 sparse Gabor convolution。它只知道球面方向、物理／角频带、种子标签和输出范围。

### 7.2 使用位置

允许噪声作用于：

- 初始质心距离 warp；
- 初始大陆性、厚度与年龄；
- 板块旋转轴、角速度与裂谷倾向；
- 俯冲、碰撞、海沟、洋脊和阻尼参数的有界空间变化；
- 沿褶皱与洋脊方向的最终细节。

### 7.3 禁止位置

- 不直接生成最终板块 owner；
- 不直接生成最终陆海 mask；
- 不在每一步重新抽样随机值；
- 不使用逐 cell 白噪声；
- 不改变事件方向、地壳守恒或数值不变量；
- 不改变球体顶点位置。

### 7.4 确定性与正交随机流

每个用途使用独立标签流：initial-plates、initial-crust、plate-motion、rift-events、process-variation、orogenic-detail、oceanic-detail。增加高频高度细节不得改变板块、裂谷事件或地壳 owner。

## 8. 初步高度图

`heightmap` 在最终构造迭代后一次执行：

1. 从 `tectonic_elevation_m` 读取论文粗地壳高程；
2. 以地壳厚度提供有界等静力修正；
3. 保留俯冲海沟、上覆侧抬升、碰撞山系和洋中脊的符号与相对位置；
4. 使用 `lineation` 对 Gabor／梯度噪声定向；
5. 按 orogeny age 衰减年轻与古老山系细节；
6. 以统一 0 m 海平面派生陆海；
7. 量化到现有高度字段的确定性单位并构造 `SphericalReliefSnapshot`。

地壳类型是高度的一个因子，不保证大陆壳必然高于海平面，也不保证洋壳绝不形成岛屿。最终海岸因此不会复制 `CrustKindField`。

## 9. 模块与依赖方向

```text
generators/natural/
  morphology/
    noise.rs              published coherent/Gabor noise primitives

  spherical_tectonics/
    model.rs              pure transient attributed crust/plate state types
    workspace.rs          current/next plus contact/process reusable scratch assembly
    initial_state.rs      initial spherical Voronoi and crust conditions
    kinematics.rs         rigid geodetic rotations and relative velocity
    contacts.rs           overlap/gap/contact classification only
    resample.rs           moving samples -> authoritative sphere
    control_surface.rs    transient coarse evolution -> one final projection
    passive_margin.rs     final graph-distance shelf/slope profile
    processes/
      subduction.rs
      collision.rs
      spreading.rs
      rifting.rs
      relaxation.rs
    runner.rs             bounded iteration and atomic candidate assembly

  spherical_relief/
    tectonic_heightmap.rs final current crust -> coarse height components
    directed_noise.rs     fold/ridge-aligned Gabor and gradient detail
```

依赖方向固定为：

```text
world/spatial + morphology/noise
              ↓
model ← initial_state / kinematics / contacts / resample
              ↓
          processes
              ↓
          workspace / runner / transient control_surface
              ↓
world/natural SphericalTectonicSnapshot V3
              ↓
spherical_relief::tectonic_heightmap
              ↓
world/natural SphericalReliefSnapshot
```

约束：

- `model` 不依赖任何 process；
- `workspace` 是唯一同时依赖 model、contacts 与 process scratch 的装配层；
- process 之间不得互相调用，只通过 runner 定序；
- `contacts` 不写地壳；
- `spherical_relief` 不反写 tectonic state；
- `noise` 不依赖 tectonics 或 relief；
- `runner` 是唯一构造迭代入口；
- `control_surface` 只组合既有 runner、球面 locator 和 `resample` 的共享插值原语，不复制过程公式；
- 瞬态控制表不得越过 facade，也不得进入 artifact、stage cache、UI 或 GPU；
- Stage、UI、GPU 不访问 workspace；
- 旧 `arrival.rs`／`area.rs` 若无其他生产调用则删除，否则保留为其实际消费者的通用原语，但球面 tectonics 不得引用。

## 10. 构建与发布数据流

```text
surface + resolved tectonic spec + formation + labeled RNG
    ↓ validate
initial_state
    ↓
for step in 0..128
    kinematics
    → contacts
    → subduction/collision/spreading/rifting
    → relaxation
    → conditional resample
    → swap current/next
    ↓
compact active plates + rebuild final boundaries
    ↓
validate SphericalTectonicSnapshot V3 candidate
    ↓
heightmap(candidate)
    ↓
validate SphericalReliefSnapshot candidate
    ↓
Stage returns artifacts; graph publishes atomically
```

若任何步骤失败，丢弃 workspace 和候选；不得发布 tectonic 成功但 relief 失败的混合自然世界。既有图级原子发布边界继续负责整套 Artifact 的最终交换。

## 11. 错误语义

新增 crate-private typed errors，并在 facade 映射为稳定产品错误：

- invalid initial crust/plate sample；
- non-finite rotation or material value；
- unresolved coverage gap after spreading；
- illegal overlap/event combination；
- plate capacity exhausted；
- empty or orphaned terrane；
- resampling failed to bind every authoritative cell；
- event/output cardinality mismatch；
- final active plate count outside bounds；
- current crust or elevation outside documented range；
- iteration/workspace budget exceeded。

不以 panic、NaN clamp、默认零值、旧算法回退或重新随机 seed 隐藏失败。相同输入必须返回相同结果或相同 typed error。

## 12. 性能与内存

默认 20k 产品门槛：

- 目标：native Release tectonic + initial heightmap 不高于 2 s；
- 硬上限：5 s；
- WASM 目标不高于 5 s，硬上限 10 s；
- 相对现有完整球面自然图的峰值工作集新增不超过 256 MiB；
- workspace 只允许两份 dense crust buffer、活动 plate metadata、边界事件和复用 scratch；
- 不允许每步分配 cell-count Vec；
- 不允许 `plate_count × cell_count × step_count` 状态；
- 接触处理应与活动边界／实际重叠成比例；
- Release benchmark 必须使用默认 seed 42、约 20,252 cells 和完整正式路径。

若直接 128 步超过目标，优化顺序固定为：

1. 复用缓冲和压缩属性布局；
2. 并行每样本的独立过程；
3. 稀疏追踪活动边界；
4. 使用论文允许的较低粗地壳采样，再保守重采样到权威表面。

不得先减少构造现象、恢复 Voronoi 最短路或降低验收质量。

正式 20k 路径已经采用第 3、4 项：spreading 以 cell 索引替代逐 gap 全局重扫；128 步 Cortial-style 演化运行在有界瞬态控制球面，最终当前状态只投影一次。Release 验收分别限制纯 tectonic construction 不高于 300 ms、包含 artifact validation 和确定性 semantic hash 的正式 tectonic stage 不高于 1 s；完整正式图仍必须满足上述 2 s 目标／5 s 硬上限及冻结基线 1.25 倍相对门槛。分项计时的目的仅是区分算法成本与发布验证成本，不允许跳过正式 stage。

## 13. 验证与测试

### 13.1 论文公式单元测试

- 球面刚体速度与解析 `omega * (w × p)` 一致；
- 洋壳／大陆壳和洋壳／洋壳俯冲侧正确；
- subduction distance/speed/elevation transfer 曲线命中端点和峰值；
- collision uplift 随地块面积和相对速度单调；
- divergent gap 生成 age=0 的洋壳并记录脊向；
- oceanic elevation 随年龄非增；
- erosion、damping 和 sediment 项与论文 Appendix A 一致；
- rifting 的 Poisson 决策、2–4 子板块与容量上限确定性。
- 大陆张裂的 McKenzie `β` 纯剪切变薄与共享 Airy 均衡沉降单调且有界。

### 13.2 球面与状态不变量

- 所有位置单位长度；
- 无接缝、极点或旋转后的统计异常；
- 每个权威 cell 恰有一个当前 crust owner；
- 无未处理空隙或多重 owner；
- 活动 `PlateId` 连续且范围合法；
- 每个最终 `PlateId` 在权威邻接图上恰有一个连通分量，其代表 cell 严格遵守面积加权方向／最低 `CellId` 规则；
- 人工构造的空 lineage、断裂 lineage 和规范化后容量溢出分别命中删除、确定性拆分和 typed error；
- 所有密集字段 cardinality 与 `SurfaceRef` 相同；
- 无 NaN/Inf，厚度、年龄和高程在范围内；
- 同 seed/spec bitwise deterministic；
- native/WASM 的量化事实相同；
- workspace drop 后无历史 Arc 或大缓存存活。

### 13.3 构造因果 oracle

- 收敛大陆边界的上覆／碰撞侧高于邻近内部基线；
- 俯冲侧形成负海沟，上覆侧形成正抬升；
- 洋中脊高于同板块古老洋壳；
- 洋壳年龄与深度具有显著负相关；
- transform 边界不产生与收敛边界等量的系统性抬升；
- Andean 与 Himalayan orogeny 的空间侧别正确；
- 关闭某一 process 的 mutation 必须只消失对应地貌因果。

### 13.4 反 Voronoi／形态验收

测试不能只断言最终 owners 与初始 owners 不相等。默认 seed 42 加至少 16 个固定 seeds 必须同时检查：

- 最终板块边界在 500–1,000 km 宏观简化尺度上不存在占主导的等距直边；
- 球面等周紧致度、凸度和边界转角分布不集中于规则 Voronoi 基线；
- 三岔点角度不集中在规则约 120 度窄峰；
- 至少半数非微型板块具有明显非圆、非正多边形宏观轮廓；
- 最终大陆／主要岛弧包含半岛、狭部、凹湾或地块拼贴证据；
- 最终 land mask 与 `CrustKind::Continental` mask 不得近似恒等；
- 海岸与当前板块边界允许相关但不能大面积完全重合；
- 5k/20k 比较使用物理尺度统计，不要求 CellId 一致。

阈值在实施时先由当前 Voronoi 失败基线和论文路线的多 seed 分布共同冻结；不得为单 seed 调低门槛。冻结顺序必须是语义 oracle 先通过、视觉矩阵确认后再记录 hashes。

### 13.5 Formation 作者意图

五种 formation 在相同 seed 矩阵上只验证统计排序与因果，不验证固定最终连通块数：

- 初始 continental area 在一个 cell 面积误差内命中各自既有目标比例；
- Supercontinent 与 GreatIsland 的初始大陆性相关长度显著大于 Archipelago 与 VolcanicIslands；
- Archipelago 的初始主要频谱显著高于 Continents，且不存在单元级白噪声峰；
- VolcanicIslands 的最终洋壳占比最高，岛弧／热点高程响应强于其大陆内部基线；
- Supercontinent 与 GreatIsland 的裂谷事件频率不高于 Continents，Archipelago 不低于 Continents；
- 删除 formation 频谱选择或把所有预设映射为同一 recipe 的 mutation 必须被上述多 seed 分布捕获；
- 任何实现都不得为通过 formation 测试而对最终 land mask 做连通块删选、填充或径向重塑。

### 13.6 视觉矩阵

必须生成可审查图册，至少覆盖 seed 42 和 16-seed 矩阵的：

- plate owner；
- crust kind；
- crust age；
- tectonic elevation；
- final initial heightmap；
- boundary kind／strength；
- lineation；
- map Equal Earth 与 globe 两种视图。

人工拒绝条件包括：蜂窝板块、大片直线、圆形大陆、板块即大陆、均匀环状山系、接缝、极点聚集、单元级棋盘或球体高程变形。

## 14. 迁移与兼容

- `SphericalTectonicStage::version` 从 2 提升到 3；
- `SphericalTectonicSnapshot` schema 从 V2 提升到 V3；
- 所有球面 tectonic 及下游 relief/geology/climate/hydro artifact 正确失效重建；
- 当前应用状态只持有作者 specs，不持久化生成结果，因此无需把旧 V2 事实自动升级为 V3；
- LegacyPlanarV1 完全冻结；
- 旧 field-driven 球面 goldens 由新语义 oracle 通过后一次性刷新，并在报告中记录为何变化；
- 二维/三维 renderer、picking、palette、GPU packet 和单位球 mesh 不改变数据所有权。

## 15. DRY 与源码溯源规则

每个算法模块顶部必须注明：

- 对应论文章节；
- 使用的方程／常量表；
- 与论文不同的工程适配；
- 适配为何不改变现象语义。

禁止出现两个实现分别计算：

- 球面刚体速度；
- 接触相对速度；
- 俯冲侧选择；
- 洋壳年龄沉降；
- 球面噪声；
- 移动样本到权威 cell 的重采样；
- 最终构造高程。

测试 helper 必须调用生产纯函数或独立数学 oracle，不复制生产分支成为第二算法。

## 16. 完成定义

本阶段只有在以下条件全部满足后完成：

1. 球面生产板块不再由旧 shortest-arrival／power partition 决定；
2. Cortial 的刚体运动、俯冲、碰撞、扩张、裂谷和粗松弛均有生产实现和论文公式测试；
3. 生成期只有双缓冲，发布物只有最终当前态；
4. V3 当前地壳属性完整、严格验证并绑定唯一 `SurfaceRef`；
5. 初步高度图由当前地壳状态正向生成，噪声只作有界扰动和定向细节；
6. 单位球 mesh 无任何高程位移；
7. 多 seed 语义、因果、formation 排序、反 Voronoi、双视图视觉矩阵通过；
8. native、WASM、Clippy、格式、完整测试和性能／内存门槛通过；
9. 旧平面兼容域不漂移；
10. 工作树干净，设计、计划、实现、验收分别提交并留有可审计证据。

## 17. 参考资料

- Y. Cortial, A. Peytavie, E. Galin, E. Guérin. *Procedural Tectonic Planets*. Computer Graphics Forum 38(2), 2019. DOI: <https://doi.org/10.1111/cgf.13614>
- D. McKenzie. *Some Remarks on the Development of Sedimentary Basins*. Earth and Planetary Science Letters 40(1), 1978. DOI: <https://doi.org/10.1016/0012-821X(78)90071-7>
- G. Cordonnier et al. *Large Scale Terrain Generation from Tectonic Uplift and Fluvial Erosion*. Computer Graphics Forum 35(2), 2016. DOI: <https://doi.org/10.1111/cgf.12820>
- A. Lagae et al. *Procedural Noise using Sparse Gabor Convolution*. ACM Transactions on Graphics 28(3), 2009. DOI: <https://doi.org/10.1145/1531326.1531360>
- L. Viitanen. *Physically Based Terrain Generation: Procedural Heightmap Generation Using Plate Tectonics*, 2012. <https://urn.fi/URN:NBN:fi:amk-201204023993>
- Mindwerks. *plate-tectonics* reference implementation. <https://github.com/Mindwerks/plate-tectonics>
