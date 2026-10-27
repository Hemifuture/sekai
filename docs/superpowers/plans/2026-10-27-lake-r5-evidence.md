# 湖 R5 证据与草案计划

> 执行者：当前受派工作者自行执行；不得再派代理。采用 superpowers:executing-plans
> 的逐任务核验方式；这里的研究探针已获授权，后续产品实现须另获 R5 设计批准。

**目标：** 形成可审阅的 R5 草案及最小可证伪证据，不把未获批准的堤岸机制投入产品。

**架构：** 从 P5 已发布事实区分面积语义；复用生产派生值、图元邻接和 P1 积分，
以 test-only 闭包检验单独填满格内低地的作用及代价。堤线位置、面积分配与层级契约
未定前不改变生产地形。显示、点击和河口裁剪必须最终使用同一已验证判定。

**技术栈：** 现有 Rust、cargo；不增加依赖。外部证据只放
`D:/hemifuture-probes/lake-r5-20261027/`。

**规格：** `docs/superpowers/specs/2026-10-25-lod1-fine-geometry-coherence-design.md` R5
（草案）；P5 湖泊水量平衡设计 §2.1 仅作为尚未实现的设计依赖。

## 全局约束

- HEAD 起点 `f660af4`，R2 对照 `a362843`；不 push、merge、冻结规格或修改其他工作树。
- 只保存日志、图像、源码与报告；不保存世界快照或磁盘缓存，不增加产品开关。
- 构建、计时、GUI 须先取得协调者明确时段；GPU 单线程，rustc access violation 用 `-j6`。
- 修改产物时升所属阶段版本；本次 test-only 探针与文档不改变产物，不虚升版本。
- 不按外观调阈值，不平滑岸线，不新增噪声，不推定未知堤线。

## 审查重点

- 湖 55 的 P1 满湿不等于所有支持层级必须逐格满湿；不得据此前提判整个 R5 不可行。
- 格内闭包可能只把直岸推到格边，须同时报告格内前沿、格边低岸和面积。
- 逐层全连通不自动保证层级嵌套；不得用单层图代替相邻层证据。
- 单点查询若需要全格枚举，深层复杂度指数增长；不得把离线 BFS 直接作为产品路径。
- 出口与相邻不同水位湖不可筑成闭墙；研究未涉及的行为不得宣称已验证。

## 任务 1：核对语义与出处

- [x] 读取交接、根仓库/当前工作树 AGENTS、A10 后续用户修订；核对干净起点。
- [x] 核查 `hydrology.rs`、`world/natural/hydrology.rs` 面积构造与校验消费者。
- [x] 查 Barnes 2014、USACE Build Walls、isoAdvector 一手依据并记录适用边界。
- [x] 在 R5 追加三层面积语义、备选修订、模型先验与待裁定问题。

## 任务 2：最小可证伪探针

文件：`src/generators/natural/hierarchical_derivation/lakes.rs` 仅挂接 `cfg(test)`；
新建同目录 `lake_r5_probe.rs`。消费 `LakeWalk::frame`、`frame_uncarved_m`、
`edge_neighbour`、`is_lake_water` 与生产 P1 积分；不扩大生产可见性。

- [x] 只用 Draft seed 42 湖 39（格内前沿）、湖 33（正常细化对照）、Standard seed 42 湖 55（格边硬截断）。
- [x] 对 L1–L6 枚举同一湖成员与两环，按生产分类播种，仅在同根格的低于 H 叶片间 BFS。
- [x] 断言闭包保留原湿叶、闭包湿叶低于 H、没有可继续扩展的同格湿/低干邻边。
- [x] 输出原分类/闭包面积、P5 `Lake.area_km2`、成员 P1 参照、两种岸边比例及相邻层变化。
- [x] 独立报告取样/邻接/闭包时间与叶向量大小（不是完整工作集）；不将其冒充产品端到端提速。
- [x] 在获准时段运行 Release ignored 探针；两侧各 2 通过，fmt/clippy/wasm 通过；直接湖模块首跑因共享 target 身份混用而失效，已仅重编当前 crate、核验 557 项与 R4 独有测试，定向补跑 5＋1 通过。
- [x] R2 同探针重跑使用证据目录下 `git archive a362843` 独立源码快照，只移植 test-only 挂接和同一探针，记录差异与 SHA256，不覆盖当前生产源码。

## 任务 3：基线证据与草案交接

- [x] 整理已有 R2 同镜头图、日志和镜头参数，标明为历史证据；没有 R5 原型不伪造修后图。
- [x] 中文报告列实际完成/未完成、文件与提交、来源、最小充分测试、开放问题。
- [x] 规格追加 R5，保持“草案，待用户批准”；任务完成记录只报告研究结果。

## 任务 4：用户裁定后接入 UI（本轮不执行）

- [ ] 先确定 P5 面积比较口径、允许的跨格重分配、堤线信息来源及层级连续契约。
- [ ] 经最小离线对照后，地形与水体统一接入 `HierarchicalEvaluator`；提升 T1 身份，
  如修改 P5 输入/面积账本则另升 P5 所属身份，不只重钉金样。
- [ ] 在实际应用里验证二维/三维、地形着色、水体着色、点击与河口裁剪同源。
- [ ] 以 R2 同镜头取得标准湖 B 连续缩放、草稿湖 39/33 ×16/×72/×288、三维及地形图；
  等细分完成再截图，附代理自检与用户本人 UI 验收步骤。
- [ ] 用户本人验收之前不得标为算法已交付。

## 每项承重技术的出处

- Priority-Flood：Barnes, Lehman & Mulla (2014), DOI `10.1016/j.cageo.2013.04.024`，
  [作者全文](https://rbarnes.org/sci/2014_depressions.pdf)。用于给定 DEM 的洼地/连通分析，
  不从粗格心推断未知堤形。
- 已知边界的 DEM 条件化：USACE HEC-HMS，
  [Applying the Terrain Reconditioning Tools](https://www.hec.usace.army.mil/confluence/hmsdocs/hmsguides/gis-tools-and-terrain-data/gis-tutorials-and-guides/applying-the-terrain-reconditioning-tools)。
  需要输入边界及河道穿墙位置；只能作为 R5 类比，不足以授权新堤线。
- 守恒界面重建：Roenby, Bredmose & Jasak (2016), DOI `10.1098/rsos.160405`，
  [作者论文](https://pmc.ncbi.nlm.nih.gov/articles/PMC5180117/)，§2.1、§3.1。
  给定分数的界面重建不是物理地形重建；跨格分数重分配和固定 H 仍未确定。
- 本次闭包：有限图 BFS 的最小可达集合，使用现有生产三角邻接及实际未雕刻高程，
  不引入任何新科学系数；目的仅为反证“一步补水就足够”。
- 水文约束插值：Hutchinson, Xu & Stein (2011), *Recent Progress in the ANUDEM
  Elevation Gridding Procedure*，Geomorphometry 2011，19–22，
  [作者论文](https://www.geomorphometry.org/uploads/pdf/pdf2011/HutchinsonXu2011geomorphometry.pdf)。
  其湖泊重建消费已给湖边界，不能直接为当前未知堤线提供形状或参数。
- P1 参照与球面面积：复用 `surface_water_geometry` 的生产积分和
  `spherical_triangle_area_unit`，不复制 R4 仓外探针的积分公式。
- 待审地形先验（本轮不实施）：Esri ArcGIS 10.2 (2014)，
  [TIN 线性插值](https://resources.arcgis.com/en/help/main/10.2/00q9/00q900000078000000.htm)
  与 [TIN 等高线](https://resources.arcgis.com/en/help/main/10.2/00q9/00q900000047000000.htm)。
  只证明既有插值/取轮廓方法；完整盆地阻水、面积及 LOD 须按 R5.4 三例最小验证另测。
