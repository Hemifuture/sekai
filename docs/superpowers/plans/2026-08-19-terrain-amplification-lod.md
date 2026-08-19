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
- [ ] Task 3 —— T1 核心 `sample()` 模块：在既有库内机器
      （`SphericalNoise3d`、`FractalProfile`、带标签子流）上严格按冻结
      规格实现——测地格上的 T0 重心插值、条件化多倍频细节、域扭曲的
      海岸与洋脊打散。单元测试：确定性指纹、跨经线与极区无缝、条件化
      单调性、格元边界连续性。按 AGENTS.md，本任务在 Task 4 上屏之前
      **不算交付**——此处勾选只跟踪提交。
      验证：测试全绿（交付延至 Task 4）。
- [ ] Task 4 —— 烘焙放大显示层：工作线程构建完成后，从 `sample()` 以
      等高色带 + 日照山体阴影烘焙一张等距圆柱颜色纹理（初始预算
      4096×2048，草稿档工作线程 ≤3 秒），并在 2D 地图与球体上加
      显示模式切换（格元视图 / 放大视图）。本里程碑实体检查仍留在
      格元视图。
      验证（UI）：切换放大视图——海岸线平滑无六边形锯齿、海洋中的直线
      构造被扭曲可见打散、山体有光影；切回格元视图检查工作流不变。
- [ ] Task 5 —— 河流雕刻与河道：把 P5 已发布河段以解析河谷剖面写入
      `sample()`，并在放大视图中绘制河段折线（线宽按斯特拉勒河级）。
      验证（UI）：河流走向与检查器报告的汇流一致，河谷在放大地形上
      可见下切。
- [ ] Task 6 —— 结构性 T0 去规则化决策门：Task 4–5 上屏后与用户共同
      判断是否仍需在 P2v5 内做板块边界域扭曲粗糙化与小尺度洋壳年龄
      扰动。若需要，则该工作另立规格修订并刷新演化/P5 证据（它改变
      产物指纹）；若不需要，在此记录豁免。
      验证：用户决定被显式记录。
- [ ] Task 7 —— 门禁与验收：fmt/clippy/wasm/全量回归、计划勾选核对、
      撰写用户验收步骤、审计报告 artifact 补充前后对照截图。
      验证：门禁全绿；用户亲自走完验收步骤。

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
- **T0 物理链（不变）**——已在 P5 完成文档中引用：Barnes, Lehman &
  Mulla 2014（priority-flood）；Braun & Willett 2013（O(N) 隐式流功）；
  Roering, Kirchner & Dietrich 1999（非线性坡面）；Davy & Lague 2009 /
  Yuan et al. 2019（泥沙输运）。

## 非目标（M1）

- 不改任何 T0 产物、指纹或质量证据（除非 Task 6 决策门另有结论）。
- 实体拾取与任何模拟都不读放大数据——T1 仅呈现；P5 产品仍是唯一权威。
- 本里程碑不做实时 LOD，也不做 1 m 级采样。
