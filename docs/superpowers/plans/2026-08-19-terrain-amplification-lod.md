# 地形放大与 LOD 计划（P6，2026-08-19）

## 背景

2026-08-19 的审计与形成链 UI 交付确立了用户批准的三层方向：

- **T0——全局物理真相**（既有 P1→P5 链）：大陆、山带、气候、河网、
  按水量反解的海平面。草稿档求解 20,252 格（≈159 km）；标准（8 万格）
  与高（20 万格）分辨率已在质量档位设计中。低于约 10 km 的全局均匀模拟
  既存不下也无意义（1 m ≈ 5.1×10¹⁴ 格 ≈ 10² PB 级），因此精细细节永远
  不能靠调大 T0 的 N。
- **T1——条件化放大**（新增）：确定性、局部、逐点的纯函数
  `sample(单位向量, lod) → 高程/材质`，插值 T0 场并叠加多倍频、域扭曲
  噪声，其振幅、粗糙度、各向异性由 T0 物理量（造山年龄、可蚀性、降水、
  坡度）调制，另沿 P5 已发布河段雕刻河谷。这是 Gleba 的手法，但条件化
  来自物理场而非手工摆点。无全局求解、按构造无缝（球面三维噪声，无
  经纬度接缝）、种子确定。
- **T2——LOD 运行时**（后续里程碑 M2）：四叉树分块 + 视距相关采样密度
  （镜头附近至 ~1 m）、GPU 位移、分块缓存。LOD 是交付机制；T1 的局部
  确定性才是使能者。

两个审计遗留项自然并入：海洋中过直的结构（板块边界几何被过薄的细节
噪声暴露）先由 T1 的域扭曲在视觉上攻克，结构性的 T0 修正设决策门；
山体阴影随烘焙放大视图一并到来。

按 `AGENTS.md`：以下每个算法任务只有当其产出在 UI 上可见、可操作时才算
交付，最终验收归用户。

## 里程碑 M1 任务

- [x] Task 1 —— UI 质量档位选择器：档位（草稿/标准；高档标注
      实验性·离线级）驱动形成链构建；档位表面缓存按（档位, 半径）为键；
      面板显示当前档位与预计构建时长（标准档在工作线程约 3–6 分钟）；
      形成链激活时灰置无效的目标陆地面积滑杆并附悬停说明。
      验证（UI）：切到标准档重建，不冻结，格数升到约 8 万且世界细节
      可见增加。（已完成：同种子草稿 20,252 → 标准 79,212 格。）
- [x] Task 2 —— 放大设计规格：在 docs/superpowers/specs/ 冻结 T1 契约——
      采样域（三维单位向量，永不用经纬度）、T0 场插值方案及其连续性
      等级、条件表（哪个 T0 场调制哪个噪声参数及其单调方向）、每 LOD 的
      倍频/频率预算、种子派生（世界种子 + 既有带标签子流纪律）、以及
      确定性指纹（冻结探针集的哈希）。
      验证：规格已提交；探针指纹测试在任何实现落地之前已在规格中列举。
      （已完成：`2026-08-19-terrain-amplification-t1-design.md`，含冻结前
      科学审查记录。）
- [x] Task 3 —— T1 核心 `sample()` 模块：在既有库内机器
      （`SphericalNoise3d`、`FractalProfile`、带标签子流）上严格按冻结
      规格实现——测地格上的 T0 重心插值、条件化多倍频细节、域扭曲的
      海岸与洋脊打散。单元测试：确定性指纹、跨经线与极区无缝、条件化
      单调性、格元边界连续性。按 AGENTS.md，本任务在 Task 4 上屏之前
      **不算交付**——此处勾选只跟踪提交。
      验证：测试全绿（交付延至 Task 4）。
- [x] Task 4 —— 烘焙放大显示层：工作线程构建完成后，从 `sample()` 以
      等高色带 + 日照山体阴影烘焙一张等距圆柱颜色纹理（初始预算
      4096×2048，草稿档工作线程 ≤3 秒），并在 2D 地图与球体上加
      显示模式切换（格元视图 / 放大视图）。本里程碑实体检查仍留在
      格元视图。
      验证（UI）：切换放大视图——海岸线平滑无六边形锯齿、海洋中的直线
      构造被扭曲可见打散、山体有光影；切回格元视图检查工作流不变。
      （已完成：代理侧已在草稿档 20,252 格与高档 198,812 格实测——球体
      与 2D 地图（Equal Earth）均渲染烘焙纹理，山影与海洋纹理可见；
      放大模式下画布点击被禁用且有提示；切回格元视图点击选中
      Cell 155190，检查工作流不变。实现要点：颜色与格元视图共用
      Hypsometric 色带与海平面锚定显示半径；地图顶点携带权威单位方向
      （接缝分割点沿弧插值），修复了 Equal Earth 数值逆变换在投影域边缘
      裁剪点上的失败（曾致草稿档 replacement 报
      "cell CellId(16215) produced invalid projected geometry"，旧发布
      原子保留）。已知表现：放大视图沿用格元网格作画布，草稿档下地图
      外轮廓有格元级切角；替换安装路径的纹理替换分支已有测试与初装
      同构覆盖，用户下次档位切换重建时即得 UI 实证。最终验收归用户。）
      **用户验收结论（2026-08-20）：烘焙贴图这一呈现形式被否决**——
      用户在放大视图中仍能看到规则六边形拼块。根因分析（记录于
      Task 4R 与 Task 6）：其一为烘焙固定 4096×2048 的奈奎斯特限制；
      其二为 T1 对 T0 的 smoothstep 重心插值在格元中心形成常值平台、
      边界形成陡带，山体阴影进一步勾勒平台轮廓——该根因与贴图形式
      无关，任何呈现路线都需配套处理。处置：全球烘焙贴图管线退役
      （删除），T1 采样引擎、色带与海平面锚定一致性、显示模式 UI、
      worker 烘焙管道全部由 Task 4R 续用。
- [ ] Task 4R —— 格元内测地细分放大视图（替代烘焙贴图；用户批准的
      LOD 路线第一步）：worker 构建完成后，把每个格元的扇形三角形
      （质心 + 相邻边界顶点对）在方向域递归四分细分 k 层，k 按全球
      三角形预算（≤8×10⁶）与档位自动选取（草稿 k=3 ≈ 20 km 顶点距、
      标准 k=2、高档 k=1）；子顶点方向归一化回单位球，从 T1
      `sample()` 取高程，按与格元视图一致的等高色带与海平面锚定范围
      着色，并用东/北向差分采样的光照法线预乘山体阴影；边中点仅由
      两端方向对称求出，保证跨格元边界的细分顶点逐位一致、无裂缝。
      三维球体直接以方向为顶点渲染；二维地图在 UI 侧按当前投影正向
      投影全部顶点（rayon 并行），跨接缝三角形复用既有接缝二分切割
      管线，投影或中央经线切换时重投影。放大视图下实体拾取恢复可用
      （子图元属于母格元，locator 照常反查）。
      已知取舍：全局均匀细分的最细可表示波长约为顶点距两倍
      （草稿 ≈40 km），比烘焙贴图少一层中频细节；视距相关加深属
      M2。烘焙贴图模块（`amplified_view.rs`、渲染器纹理绑定 4/5 与
      等距圆柱采样路径）删除。
      验证（UI）：放大视图中格元内部可见更小的三角形图元且边缘
      锐利不糊；跨格元与跨接缝无裂缝断色；球体与地图一致；投影/
      经线切换后放大视图正常；放大视图下点击可选中实体；切回格元
      视图工作流不变。
      **交互修正（2026-08-20，用户反馈）**：不设"显示模式"开关——
      画布只有一个视图，滚轮缩放即所见。细分层在填色字段为
      "地表高程"时自动替代格元填色（渲染器在网格缺失时自动回落
      格元填色）；选择其他字段（降水、湖泊深度等数据检查）时保持
      权威格元渲染。已知表现：细分层激活时点击选中的格元高亮不
      可见（高亮属于格元填色着色器）；缩放驱动的细分深度自适应
      属 M2。
      **视觉语言修正（2026-08-20，用户反馈）**：自始至终只有一种
      图——格元马赛克，细分只是单元变小。去掉顶点渐变与山体阴影：
      每个子三角形是一块纯色（在其球面质心处经 T1 采样、用与格元
      视图相同的等高色带与海平面锚定范围取色），由专属首顶点携带、
      GPU 平直插值整块着色；接缝切割后的碎片沿用母三角形的纯色。
      三角形预算相应降为 5×10⁶（草稿 k=2 ≈ 40 km、标准 k=1 ≈
      44 km、高档 k=1 ≈ 25 km 子单元）。光影与更平滑的地形呈现
      留待用户明确需要时另行开启。
- [ ] Task 5 —— 河流雕刻与河道：把 P5 已发布河段以解析河谷剖面写入
      `sample()`，并在放大视图中绘制河段折线（线宽按斯特拉勒河级）。
      验证（UI）：河流走向与检查器报告的汇流一致，河谷在放大地形上
      可见下切。
- [ ] Task 6 —— 结构性 T0 去规则化决策门：Task 4R–5 上屏后与用户
      共同判断是否仍需在 P2v5 内做板块边界域扭曲粗糙化与小尺度洋壳
      年龄扰动。若需要，则该工作另立规格修订并刷新演化/P5 证据（它
      改变产物指纹）；若不需要，在此记录豁免。
      追加评估项（2026-08-20，源自 Task 4 验收）：T1 规格 §3 的
      smoothstep 权重重映射在格元中心产生常值平台，是"圆化六边形"
      观感的主要根因之一，细分路线不会自动消除。候选对策：细节振幅
      与局部起伏配比复核、对 T0 场先做一次球面平滑再插值、或更换
      插值核（需修订冻结规格 §3 并刷新 T1 指纹）。与去规则化一并
      决策。
      验证：用户决定被显式记录。
- [ ] Task 7 —— 门禁与验收：fmt/clippy/wasm/全量回归、计划勾选核对、
      撰写用户验收步骤、审计报告 artifact 补充前后对照截图。
      验证：门禁全绿；用户亲自走完验收步骤。

## 结构决策记录（2026-08-20 会话，与用户讨论得出）

1. 层级图元结构：顶层为 Goldberg 六边形格元（L0，
   物理/战略语义层）；自 L1 起为逐格元扇形三角形的
   递归四分，三角形内嵌三角形、精确嵌套、递归同构
   （rep-tile 性质）。层级 ID 采 HTM 式编码：
   （格元 ID, 扇区 0–5, 每层 2 bit 四分路径）；子三角形
   几何由格元顶点递归中点纯函数重算，免存储、确定性。
   此结构同时作为 M2 分块 LOD 与未来 gameplay 细节层（地块/
   所有权/资源聚合，精确向上求和）的统一骨架。
   出处：Szalay & Kunszt 等，Hierarchical Triangular Mesh
   （SDSS 天文巡天索引）；Google S2（四边形同构思想）。
   （替代方案 f×2 子 Goldberg 显示网格因六边形不可自嵌套、
   只能近似归属而被否决。）
2. 质量档位语义澄清：档位 = T0 全局物理解算的分辨率
   （创世参数，改变地理事实本身，正式世界锁定档位）；
   LOD 下钻 = 纯显示参数（锁定档位后 T1 逐点解算，不改
   任何地理数据）。两者正交：前者回答“世界有多真”，
   后者回答“看得多细”。M2 可将下钻深度暴露为显示控制。
3. 寻路定位：连续移动语义以三角形层级为 navmesh（出处：
   Demyen & Buro, *Efficient Triangulation-Based Pathfinding*,
   AAAI 2006；Recast/Detour 工业标准；漏斗算法拉直路径），
   并天然支持分层规划；若需战棋式离散格移动，发生在 L0
   六边形层。层级 ID 编码的实现推迟到语义需求（gameplay
   或 M2 分块）落地时再做。

## 工程债（随显示层重构处置）

- 替换构建的显示状态合并：worker 构建期间用户的任何显示操作（切换
  显示模式、填色、投影、中央经线）都会使完成的候选在
  `validate_current` 处被判为过期（"world candidate was prepared
  from a stale spherical publication"），整次构建被安全丢弃。保护
  语义正确，但体验是白等一次构建。正确修法：安装时把用户当前显示
  状态合并到新候选上（uniform 级状态直接合并；几何级状态重投影），
  而非拒绝。

## 里程碑 M2（M1 验收后另立计划）

立方球四叉树分块、视距相关的 `sample()` 密度（镜头附近至 ~1 m）、GPU
位移与分块缓存、基于环流场的群系/材质着色、以及低于 T0 分辨率的每块
受约束精细水文合成。

## 每项承重技术的出处

以下技术无一为本项目发明；每项都是确立的研究结果或已量产的生产系统。
由我们**原创**的是组合方式——尤其是 Task 2 中把 P5 物理场映射到噪声
参数的条件表——这正是规格冻结与 Task 6 用户决策门存在的原因。

- **T0 测地二十面体网格（既有球面）**——全球大气建模的标准做法，始于
  Sadourny, Arakawa & Mintz, *Integration of the nondivergent barotropic
  vorticity equation with an icosahedral-hexagonal grid for the sphere*,
  Monthly Weather Review 96 (1968)；今日在业务运行中的有 DWD/MPI 的
  ICON（Zängl et al., QJRMS 141, 2015）与 NCAR 的 MPAS（Skamarock
  et al., MWR 140, 2012——球面质心 Voronoi 网格）。12 个五边形是
  Goldberg 多面体性质（Goldberg, 1937）。
- **Task 3/4 条件化多倍频噪声**——fBm 与程序化噪声：Perlin, *An Image
  Synthesizer*, SIGGRAPH 1985；Perlin, *Improving Noise*, SIGGRAPH 2002；
  空间变化的粗糙度（"异质地形"）是 Musgrave 的多重分形路线：Musgrave,
  Kolb & Mace, *The Synthesis and Rendering of Eroded Fractal Terrains*,
  SIGGRAPH 1989，收录于 Ebert et al., *Texturing & Modeling: A
  Procedural Approach*（第 3 版，2002）。
- **Task 3/4 域扭曲**——Perlin & Hoffert, *Hypertexture*, SIGGRAPH
  1989；Quilez, *Domain Warping*（iquilezles.org）；2026-08-19 审计中
  读到的一手量产证据：Factorio Space Age 的 Gleba 表达式对每个群系坐标
  做扭曲（`gleba_wobble_x/y`，官方 wube/factorio-data 仓库）。
- **地形放大作为研究方向**——Paris, Galin, Peytavie, Guérin & Gain,
  *Terrain Amplification with Implicit 3D Features*, ACM TOG 38(5),
  SIGGRAPH Asia 2019（doi 10.1145/3342765）；后继 *Terrain Amplification
  using Multi Scale Erosion*, ACM TOG 2024（doi 10.1145/3658200）；综述
  见 Galin et al., *A Review of Digital Terrain Modeling*, Eurographics
  STAR 2019。
- **Task 5 解析河谷图元雕刻**——Génevaux, Galin, Guérin, Peytavie &
  Beneš, *Terrain Generation Using Procedural Models Based on Hydrology*,
  ACM TOG 32(4), SIGGRAPH 2013（doi 10.1145/2461912.2461996）：层级排水
  图 + 河流图块的混合/雕刻算子——我们的变体用 P5 发布的物理河网替换
  其合成图。
- **Task 3 球面插值**——Langer, Belyaev & Seidel, *Spherical Barycentric
  Coordinates*, Eurographics SGP 2006；守恒/重心重映射同样是气候再网格化
  文献的标准做法。
- **M2 分块 LOD 行星运行时**——Ulrich, *Rendering Massive Terrains Using
  Chunked Level of Detail Control*, SIGGRAPH 2002 course；Losasso &
  Hoppe, *Geometry Clipmaps*, SIGGRAPH 2004；Cignoni et al., *P-BDAM*,
  IEEE Vis 2003；Cozzi & Ring, *3D Engine Design for Virtual Globes*,
  2011（立方球四叉树；Cesium 谱系）；开源参考实现：Proland（INRIA，
  Bruneton & Neyret）。
- **M2 惰性求值的确定性分块（量产先例）**——Factorio 自家噪声管线
  （FFF-390，审计中已读）；Hello Games, *Building Worlds in No Man's Sky
  Using Math(s)*, GDC 2017；Outerra 公开管线（粗真实 DEM + GPU 上的
  分形细化）。
- **T0 全局耦合求解作为架构（2026-08-20 尽职调查补录）**——
  学术背书：全球地貌演化模型 goSPL，Salles et al.,
  *Hundred million years of landscape dynamics from catchment to
  global scale*, Science 379 (2023)；区域级 LEM 谱系 FastScape
  （Braun & Willett 2013）、LandLab（Hobley et al., Earth Surface
  Dynamics 2017）、CHILD（Tucker et al. 2001）、Badlands
  （Salles 2016–2018）；构造–气候–侵蚀耦合：Willett, JGR
  1999；Whipple, *Nature Geoscience* 2009 综述；简化全球环流
  对应 EMIC 谱系（PlaSim, Fraedrich et al. 2005；Budyko/Sellers
  1969 能量平衡模型）；板块层面 GPlates（Müller 等）。
  工程先例（游戏界少数派但有例可循）：Dwarf Fortress 世界
  生成（温度/降雨/排水/侵蚀全图耦合）；WorldEngine +
  PlaTec（Viitanen 2012，板块→气候→侵蚀单向链，与本链
  结构同构）；Cordonnier et al., Eurographics 2016（LEM 入
  内容生成）。Galin et al. STAR 2019 将 simulation-based 列为
  三大路线之一并指认 hybrid（模拟大尺度 + 程序化细节）为
  方向——即本项目 T0+T1 分层。已知简化：本链为单向
  （P1→P5 一次通过，仅单向均衡响应），学术完全体为
  构造–气候–侵蚀双向反馈迭代；游戏主流为噪声合成，
  选择模拟路线的理由是地理因果可信性产品目标。
- **T0 物理链（不变）**——已在 P5 完成文档中引用：Barnes, Lehman &
  Mulla 2014（priority-flood）；Braun & Willett 2013（O(N) 隐式流功）；
  Roering, Kirchner & Dietrich 1999（非线性坡面）；Davy & Lague 2009 /
  Yuan et al. 2019（泥沙输运）。

## 非目标（M1）

- 不改任何 T0 产物、指纹或质量证据（除非 Task 6 决策门另有结论）。
- 实体拾取与任何模拟都不读放大数据——T1 仅呈现；P5 产品仍是唯一权威。
- 本里程碑不做实时 LOD，也不做 1 m 级采样。
