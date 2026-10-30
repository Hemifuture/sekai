# 湖泊 R7 下一阶段验证计划

> 执行者：批准本计划后按 superpowers:executing-plans 或受监督的
> superpowers:subagent-driven-development 逐任务执行；当前只完成研究/设计，不自动实施产品。

**目标：** 用明确的物理长度校准、同场源域/汇水和受控根/FSM 夹具，把 R7 收敛到可批准
的 P5→T1→UI 产品实现；不得把三湖旧 C 诊断或 d0/d1/d2 当成最终模型验收。

**架构：** 选择全球统一 native 固定连续场，不做旧 Shepard patch blend。最终公开地形
投影是数值 SSOT；权威 H/裁剪用 f64，源域用真实 median-dual/气候求交，年度 B1 用同场
图与分片常 Jacobian 度量。显示只查询/裁剪固定场，物理尺度不随缩放增长。

**技术栈：** 当前 Rust/cargo 与生产几何、P1、气候助手；不预加依赖。

**规格：** `docs/superpowers/specs/2026-10-30-lake-p5-t1-ui-r7-design.md`（R7 草案）；
既有湖稳态设计 R1 的年度通量物理沿用，旧单调根端点扫描与旧几何口径显式替代。

## 全局约束

- 本树起点 `cee56d2`；不冻结、不 push/merge，不修改其他工作树或用户原有文件。
- 所有世界生成、cargo、计时与 UI 窗先由协调者错峰；本树独占 target，offline/-j6，测试单线程。
- 不创建网络/UDP 压测，不操作远程服务；只清理本任务创建的进程。
- 不持久化世界快照，不添加产品试验开关，不通过换 seed/挑新坑/抬地形/噪声系数凑通过。
- 产品实现须升真正身份/schema；本轮 cfg(test) 不改变生产产物，不虚升版本。
- 只有同场物理与 UI 接入、用户本人验收都完成，才可称产品闭环交付。

## 本轮已完成的最小证据

- [x] B1 三湖：真实 H_spill、原格多边形 P/E/R 求交、旧 C 差与成本；两档各一 fixture。
- [x] 三湖 outside-C 均非零，湖 55 存在负 E−P+R 系数；未假求物理根。
- [x] Native：单份 Draft d0/d1/d2，完整图重算真实鞍点/盆地/岸段/成本，未保旧 H。
- [x] 已知最终片内部查询小单元与淡水零通量/生产几何直接消费者通过；完整 fmt/clippy/wasm。
- [x] 新设计终审后保存一个可独立编译的 B1/native/cfg(test) 共同证据提交，另存 R7 文档任务；仍不实施产品。

## 审查重点

- 固定同一 level 的 native 边长不等于旧 fan 的物理波长；校准不能只重跑原 d 档。
- 旧 C 与新湿域跨源格时，源项必须按实际控制多边形重分配，不能静默扣旧 Q0。
- F 的混合符号可以在同号端点之间产生两个根；必须检查导数内极值与稳定符号。
- 兄弟湖合并不能把 child 已吸收的 Q_up/P/E/R 又作为 parent 新输入。
- 海洋边界只是保留粗终端近似；公开场上高于海平面的旧海终端必须拒绝，不能吞掉误差。
- 全球基面改变影响原海岸/内陆均值/细海体积，不限于三个湖；UI 不得只给湖图。

## 任务 1：物理长度与全球基面的一次校准

**文件/接口：** 新增 test-only `lake_physical_scale_probe.rs`，由既有 `lake_tin_probe` 挂接；
复用 native mesh、production `Conditions`、同场 minimax 与已有三湖 fixture 构建。
入口 `physical_scale_calibration_draft42()`，输出规范化的原始长度/谱/几何/成本记录。

- [ ] 一份 Draft fixture 中测生产旧 fan 的参考边长与第一次 split 对应的长度、native 实际边长
      和 T0 相邻节点二阶增量。唯一参考长度取生产 `cell_spacing_m()`（平均格面积平方根）；
      不把它写成实际邻距均值。对待平分边的规范中点 m，取
      `ell_mid=radius×central_angle(a,m)`，按 R7 §4 的唯一候选
      `A_edge=A0_edge(ell_mid/ell_ref)^H_edge`，其中 A0/H 复用 `Conditions::mean`。
- [ ] 只构造这一按真实长度回锚的 native 候选，与已归档 d2 作为对照；不再无目的扫原 d0/d1/d2。
      不新增幅度拟合参数，不为湖 33 另找“更好”的种子；识别所有真实洼地并记录旧镜头位置结果。
- [ ] 在同一 fixture 测：粗聚合高程均值、旧海终端的 `z−H_sea`、新海平面严格低连通、
      细场海体积相对现 P3 离散库存的偏差、海岸几何变化；这些只是下尺度误差诊断。
- [ ] 测三湖位置的新盆地/出口、岸段与长度、A*/V*、真球面 A_ref 与求积细分差、CPU/内存。
      不先钉 d/容差；非有限、越域、未封闭或超资源只诚实标不支持，不夹状态。
- [ ] Draft 候选不能满足支持尺度的质量/账本/工作量时淘汰，转已列有来源的连续插值对照，
      不增加 seed/d 试到好看。通过后才补 Standard 55 的一个 fixture 与原同镜头。
- [ ] 保存该研究任务提交，结果只决定后续支持尺度/模型选择，不宣称全地形可上线。

## 任务 2：固定源域度量与最低稳定根

**文件/接口：** 经设计批准后在 generators 的湖几何单元实现
`LakeMeasurePiece { physical_piece, source_cell, hydro_node, original_barycentric_corners, area_m2 }`；
`evaluate_measure(pieces, H) -> (A_star, V_star, source_terms)` 直接复用生产正深度积分。
根接口消费排序事件和分片二次项，返回最小稳定根/满溢/干边界或明确数值失败。

- [ ] 先写受控源域分割夹具：完整子片面积闭合；同一原坐标 H 轮廓；`dV*/dH=A*`；
      A* 与 A_ref 的差是声明的度量近似，不能误称精确球面一致。
- [ ] 实现原坐标交叠/稳定三角化与真实整片球面积常权重；不将交叠片方向重新连成新 P1。
- [ ] 写“同号端点之间两根”夹具及事件点/切触根夹具；按区间导数极值分单调段，
      对真实 F* 括号二分至 f64 不可再分，按高度/稳定符号选最小值；不加 epsilon。
- [ ] 近退化/非有限必须明确失败而不是误判无根；数值残差预算依据生产算子实测后修订。
- [ ] 只跑这些纯函数/直接消费者测试并保存该任务提交，不为局部求根扩世界语料。

## 任务 3：同场汇水与年度 FSM 账本

**文件/接口：** 固定物理 mesh 节点/边、实际 median-dual 凸部分和 P5 气候格求交；
`build_drainage(mesh, source_parts, ocean_terminals)` 生成同场最陡边 DAG/洼地树；
年度 solver 消费树、原始 source-part IDs 和外部边 IDs，产出 active bodies 与 Q_out。

- [ ] 最小坡面/两坑一鞍点夹具先验证控制多边形覆盖、实际球面面积及源项一次归属，
      不能用 A/3 代替非 Delaunay 球面真实分区。
- [ ] 同场重建 DAG/平台/洼地层级；海洋严格子水平集分类与 `=` 临界接触分开；
      旧 Ocean root 高于公开 H_sea 时返回具 ID/差值的边界错误。
- [ ] 用两坑夹具覆盖单侧满溢→兄弟部分充水→合并→parent 外流；合并前禁内部回流投递，
      parent 按原始片与外部边重建账本，去掉内部 spill/Q_up，验证没有重复 P/E/R/Q。
- [ ] active body ID 由规范坑 ID 生成，出口与河口来自最终图，不继承旧 Lake ordinal。
      对最终三个镜头只查询真实新水体，不能选择新坑来补数。
- [ ] 验证 dry R＋wet P−wet E−外流 的局部/全局年度账本，旧粗 Q0 诊断不进入产品。
      保存该任务提交；不加入季节储量或 P4 双向湖反馈。

## 任务 4：真实发布消费者与 UI 预览

**文件/接口：** P5 最终投影/终点组装、world spherical hydrology/schema、quality/recipe/version；
T1 的 `query(direction)` 与多片 `clip_display_triangle`；app `amplified_mesh` 批次/拾取；
共享 direction-domain mesh 的二维/三维与河口消费者。

- [ ] 更新权威 f64 H/几何引用、partial-support 与 L0 类别/点深度语义；旧整格 A/V 验证替换为
      同一几何/源项校验，Lake 出口/河流/质量计数不能仍读旧几何。
- [ ] 清除权威比较里的 LakeWire/WaterCandidate/uncarved/centreline f32 回路，GPU 不重判水类。
      升真正 P5/T1/schema/配方与缓存身份，检查旧配方确实不再被判完全兼容。
- [ ] 批次按物理面和岸线求交返回多片＋物理片 ID/body/f64 H；pick 从实际提交片索引查询
      同一连续场；不能只替换旧扇锚值和整叶 flat color。
- [ ] 不添加常驻产品开关；用本地实际应用预览 Standard 55、Draft 39/33，附原同镜头、
      新海岸/内陆均值影响、2D/3D、当前高程/地表水、点击与入湖河线。
- [ ] 报完整端到端时间/内存和适用尺度。共享 schema/身份改动后跑所需 Release 回归；
      最终用户本人 UI 验收前不标交付。

## 每项承重技术的出处

- 物理谱律与中点新息：现 T1 v2/LOD-1 已有来源与生产 `Conditions`；本任务只测物理长度
  映射，不新增岸线专用谱或经验拟合系数。
- 同 TIN 最陡边路由：Ivanov et al. (2004) §2.5.1，DOI `10.1029/2004WR003218`。
- median-dual 与分片常度量：FESOM2 官方 Geometry §§The placement of variables、Earth sphericity；
  本项目常 Jacobian 是声明的近似，仍需 A_ref 实测。
- 年度水量：既有 10-08 R1；Matsubara & Howard (2009)，DOI `10.1029/2007WR005953`。
- 洼地树/FSM：Barnes et al. (2020, 2021)，DOI `10.5194/esurf-8-431-2020`、
  `10.5194/esurf-9-105-2021`；年度通量适配须由两坑夹具证明。
- 分片多项式/括号根与重根风险：GSL 2.8 Polynomials、One Dimensional Root-Finding；
  不添加 GSL 依赖，所有科学积分继续复用生产助手。
