# 跨岸重建与细岸一致性 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 先证伪候选A，再在保留原质量门与科学账的条件下实现同源细岸、河口、拾取和局部细节作者闭环，并独立校准CPU账。

**Architecture:** 复用P5中心节点与共享锚，T1派生分片床面；一个已验证几何对象供填色、岸线、河口与hit-query使用，Map/Globe只投影。作者输入进入版本化recipe，科学bundle按内容身份复用；异步候选连同GPU准备整体交换。性能只据可信测量选择一项等价工程改动。

**Tech Stack:** Rust 2021（rust-version 1.85）、egui/eframe 0.31、wgpu现有路径、serde/serde_json、blake3、Rayon；不新增几何依赖。

**Spec:** `docs/superpowers/specs/2026-10-01-coast-reconstruction-and-boundary-design.md` P1-A；执行者须同时读规格、AGENTS.md与本计划。

## Global Constraints

- 用户已批准P1全部推荐及逐任务实施；计划独立审查通过后自动继续，无需再次请用户确认。工作树`sekai-coast`、分支`codex/coast-consistency`、基线`7143d1e4a8a74cbe5c5bc13c87dd43d315d64747`；所有作者/执行者/独立审查为gpt-6.1-sol/high，不push/merge。
- 原构造16,384方向、L0/L6高程≥原海面，drift≤0.01不变；L0逐位恒等、有限/支持域、共享边、种子、P5河床/流量与原ocean/lake/forcing校验不放宽。L1精确面积mean明确改pre-carve观测。
- 同源fine几何不是strict fine-volume；不改变原P5水位/湖积分/物理账，不引入科学clamp、补直方图或失败c′候选。A红停止依赖其的任务2–5，不自动B/C；任务6独立可继续。
- geometry误差/字节包络尚未冻结：先测、记录来源、提交审阅；没有新容差授权。未冻结前只能验证研发候选，不进入正式默认、不称算法交付；需要新科学裁定才停在具体证据上。
- `MAX_PROJECT_FILE_BYTES`（65,536）、recipe-only、compatible reopen自动一次、explicit rebuild、旧world仅留内存、no snapshots保持。当前graph仍为ClimateWorkDomainStage + bundled P2–P5，不承诺空间science局部重算。
- 保持`HIERARCHICAL_PATH_DEPTH_MAX`、`VIEW_LEAF_BUDGET`（5,000,000）、`CACHE_LEAF_CAPACITY`（4,000,000）、`MIN_PRIMITIVE_EDGE_M`（10）；裁剪后成本另测，不把源叶上限冒充新片/字节上限。
- 新功能/bugfix任务红→最小实现→绿；纯等价性能优化保留正确性绿色基线、先记录真实成本证据再改动/配对复测。每任务随后独立spec review→独立quality review→本地commit；发现问题修复后复审。审查者不参与该任务实现；任务代码/证据一起审。文档任务不用写镜像测试。
- 测量串行，不与构建、UI或其他基准竞争；不调查/停止其他agent进程，不改Windows policy，不设RUSTFLAGS/target-cpu=native。自己的构建/runner须记录PID、UTC、参数、exit、产物hash。
- 常规代码提交门：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --all-features -- -D warnings`；`cargo check --target wasm32-unknown-unknown --all-features --lib`。按影响面跑最小Release测试；共享schema/身份/重采样完成后扩到Release全量，离线ignored探针显式运行且不并发。

## Review Focus

1. 水位恰等锚/整条边、触边未穿越与方向反转：严格`h<H`且canonical退化归属，任务2/3测试。
2. 两个同水位独立LakeId支持环重叠：不得靠排序合湖；owner冲突/湿外缘须拒绝，任务2测试。
3. 异深度共享边和离屏窄连接：验证最小连接闭包，无法证明或预算不足保留旧完整版本，任务2/4测试。
4. edit/undo/open/cancel后旧结果晚到，或GPU上传失败：不混用填色、河口与query版本，任务4/5测试。
5. 合法旧recipe、未知枚举、过长/非法路径与P1才暴露的绑定不匹配：解析不panic、不静默应用、不自动重试，任务5测试。

---

## 文件职责与共同接口

优先改现模块；只为实际几何消费者新增两个文件，禁止先造通用约束/边界框架。

| 文件 | 职责 |
| --- | --- |
| `src/generators/natural/surface_water_geometry.rs` | 提取现P5锚、父扇权重窄助手，保持原生产算子逐位顺序 |
| `src/generators/natural/hierarchical_derivation.rs` | T1锚、树/新息、节点几何与面值分离；测试内旧算术锚对照 |
| `src/world/natural/fine_water_geometry.rs`（新增） | geometry身份、owner、节点/片、观测与错误的领域数据；无投影/颜色 |
| `src/generators/natural/fine_water_geometry.rs`（新增） | 共形切分、解析裁剪、连接/支持验证、同域账与局部终段 |
| `src/generators/natural/hierarchical_rivers.rs` | 原路径/床高/宽度来源；受约束新息后完整关联path/carve重求 |
| `src/app/amplified_mesh.rs`、`src/app.rs` | selection请求、缓存身份、预算与候选原子安装 |
| `src/view/spherical_mesh.rs`、`src/ui/spherical.rs`、`src/gpu/mod.rs`、`src/gpu/spherical/renderer.rs` | 消费细片投影/上传与同revision点击；色板继续`src/view/palette.rs` |
| `src/app/authoring.rs`、`project_file.rs`、`project_ui.rs`、`spherical_presentation.rs` | 约束输入、历史、codec、兼容性与P1后/graph前绑定检查；project_io沿用原子保存 |

新类型默认`pub(crate)`，实现只向具体消费者放宽：`WaterBodyId = Ocean | Lake(LakeId)`（复用已发布`Lake::id()`，不取临时排序序号）；`FineGeometryIdentity`含P5内容hash、SurfaceRef、T1/water/约束语义版本、约束内容hash与selection身份。session/revision/request serial在app请求token中，不能代替内容hash。

`FineNode`含canonical节点键、`UnitVector3`、`elevation_m: f64`；`FinePiece`含根`CellId`、`LocatedPrimitive`、节点索引、`Option<WaterBodyId>`与参考域权重；`FineWaterGeometry`拥有节点/片/邻接、身份、连接验证与`FineGeometryObservation`。`FineHit`含上述primitive、owner、`water_level_m: Option<f64>`、`elevation_m: f64`与geometry身份。观测登记固定D/H/owner、实际深度、baseline谓词、分阶段面积/体积、缺参考及资源量，无自拟pass阈值；`FineGeometryError`至少区分取消、资源、owner冲突、湿支持外缘、连接未知/丢失、参考权重失败、river不接合，不合并为成功空图。

精度契约：P5 f32输入无损转f64，P5已有f64工作路径直接复用；锚、交点参数、参考权重和积分均f64，canonical键固定顺序；f32只在既有public面值cast及最终GPU坐标/颜色边界，GPU不反算owner。每父扇共形/river切分以同参考域参数权重分配且回收父权重；此映射须验证，失败即停，不能改用等权叶、投影像素或另一球面面积冒充账。

## Task 1：候选A最小可证伪验证（后续细岸硬前置）

**Files:** Modify `src/generators/natural/surface_water_geometry.rs`、`src/generators/natural/hierarchical_derivation.rs`、`src/generators/natural/terrain_amplification.rs`（仅错误承接）、`tests/hierarchical_product_gates.rs`；新增测试仅在现模块`#[cfg(test)]`与现产品gate内。证据写`../sekai-coast-evidence/a/`，不保存bundle/world快照。

**Interfaces:** 复用并最小放宽`reconstruct_vertex_elevations(surface: &SphericalSurfaceSnapshot, elevation_m: &[f64], cancellation: &BuildCancellation) -> Result<Vec<f64>, WaterVolumeSolveError>`为natural内部共享助手，保留签名/实现/顺序；T1私有`from_amplifier`改返`Result<Self, TerrainAmplificationError>`，该error透明承接`WaterVolumeSolveError`，现非取消构造使用新`BuildCancellation`并传播错误，不吞失败。L1质心仍P5值；L2+种子/幅度不改。不新增生产legacy mode；对照在测试内给另一evaluator设置由旧算术规则得到的锚，其余生产walk/face/carve相同。

- [ ] 写`p1_a_constant_step_shared_edge`：常值保持、500/−1800m阶跃基底在邻值范围内、两侧同节点bit一致、L0恒等；诊断基底/新息/carve/cast分开，均值只观测。先运行`cargo test --release --lib p1_a_constant_step_shared_edge -- --nocapture`，旧T1/P5锚不一致应红。
- [ ] 最小提取P5助手、替换T1算术锚；测试不得复制Shepard公式。只保留候选验证必需改动，不提前开发fine结构或UI。
- [ ] 在`hierarchical_derivation`私有`#[cfg(test)]`模块写ignored `p1_a_draft42_shared_anchor_probe`：按`tests/support/causal_formation.rs`的原recipe用`crate::`生产构造器装配一次Draft42，不直接include依赖extern `sekai`的support源码；两个evaluator借同bundle比较旧算术/P5共享锚，对照只在该测试模块设置私有旧锚，不新增生产public legacy接口。复用生产面值求值与私有阶段助手，记录L0/L1/L2/L6 fixed16,384方向、丢/增、pre-carve/cast、片内点值和pre-carve面积mean。记录真实bundle/build/surface身份，旧对照应复现4219→3991与drift0.013916；身份平台不同则注明，原科学门仍必测。
- [ ] 运行`cargo test --release --lib p1_a_ -- --include-ignored --nocapture --test-threads=1`；同一次fixture显式调用原gate逻辑：`fibonacci_probe(index, 16_384)`、L0/L6 `sample(...).elevation_m >= 原sea_level`、`abs(l0_land-l6_land)/16_384 <= 0.01`，统计与assert共用样本，保持原方向/口径/阈值，并核对`tests/hierarchical_product_gates.rs`原函数调用路径一致。只有无法证明同路径时才另运行`cargo test --release --test hierarchical_product_gates deep_land_fraction_stays_within_one_point_of_l0 -- --ignored --exact --nocapture`并记录必要性，避免默认第二进程重复fullsolve；不得将ignored当绿、不扩seed/Standard。
- [ ] 决策：原gate、共享边/数值域任一红即归档失败与归因、停止任务2–5；不默认移除ignore、更新科学金样、运行失败候选UI或转B/C。可保留经独立审查的助手/探针与可恢复验证改动；失败路径不得作为默认发布算法。全绿才新增单一`T1_DERIVATION_VERSION: u16 = 2`登记正确性变更（旧隐式版本1；现`DERIVATION_DOMAIN`保持不变以保留L2+种子），按审计平台重新测新派生指纹（P5科学hash不变），随后复审提交`test: validate shared P5 anchors against coast gates`。

构建优先本worktree自有target，Phase A target只读。小验证可用`../sekai-diagnostics-evidence/cpu/compile.ps1`的精确dependency fingerprint/direct-rustc流程构建自有test/runner，参数和代码独立保存，不导入诊断hooks、不覆盖旧脚本/target；必须实跑同一断言与生产路径，记录代替Cargo的理由，不重复40分钟无关baseline。

## Task 2：共形床面、唯一湿干片与可证伪几何账

**Files:** Create 上表两份`fine_water_geometry.rs`；Modify相应`mod.rs`、`hierarchical_derivation.rs`、`app/amplified_mesh.rs`；测试在generator模块。

**Interfaces:** `HierarchicalEvaluator::triangle_nodes(cell: CellId, sector: u8, path: &HierarchicalPath) -> [FineNode; 3]`输出原内部锚的派生几何，不改变`PrimitiveValue/sample`语义。`derive_fine_water_geometry(input: &FineGeometryInput<'_>, cancellation: &BuildCancellation) -> Result<FineWaterGeometry, FineGeometryError>`；input只借evaluator、验证bundle/surface、selection的canonical图元列表与保护连接包络，不深克隆P5。`FineWaterGeometry::query(direction: UnitVector3) -> Option<FineHit>`返回primitive、owner/H、派生h及geometry身份。

- [ ] 写`fine_water_`测试：同深度/相邻深度共享切分并集与过渡三角无T-junction；h=H的点/整边严格干归属；单水位裁剪解析面积/深度积分与P5生产助手一致，父权重回收；海岸与填片公共边同一索引。运行`cargo test --release --lib fine_water_ -- --nocapture`应红（接口不存在）。
- [ ] 实现按canonical edge+W+geometry版本唯一交点、h<H裁剪与邻接；海面/各湖面从bundle读取。候选成员+一环，枚举为所属优先、水位、类型/稳定ID全序；排序不证明物理分区。
- [ ] 加失败测试：异水位双湖竞争、同水位独立湖合并、封闭干洼地新孤立湿片、湿支持外缘、小岛/窄海峡与lake主体/outlet断连。组合P5粗摘要与已导出细片，在selection+共享边+相关已知连接包络证明，记录实际深度并成对比较LOD；域不足最小扩域，预算内加深仍无法证明即`FineGeometryError`，不按粗摘要猜fine连接或改符号。query自身不得全域扫描。
- [ ] 运行同一测试命令绿，复用任务1唯一真实fixture做同D/H/owner离线观察；ocean对同D的原P1部分湿积分，lake对同D逐格depth账，缺数据只报本地量/缺参考。分账P5、重建、新息、carve、cast/裁剪与owner覆盖；测片放大、scratch/候选/旧版/staging/GPU峰值。
- [ ] 将测量定义、适用域与拟定包络交审查；未冻结不能默认上线，无授权不得自填新数字。硬失败停止细岸安装；通过局部验证的代码以`feat: derive shared fine water pieces with explicit observations`提交，状态明确为研发候选，未称UI交付。

## Task 3：窄河口与湖入出流接合同一片集合

**Files:** Modify `hierarchical_rivers.rs`、generator `fine_water_geometry.rs`、`app/amplified_mesh.rs`，测试在generator模块。

**Interfaces:** `bind_river_terminals(geometry: &FineWaterGeometry, paths: &[RiverGeometryPath]) -> Result<Vec<FineRiverTerminal>, FineGeometryError>`；`RiverGeometryPath`是原path的借用几何节点、reach身份、方向/床高/宽度/portal与源/目标owner，`FineRiverTerminal`携带相同geometry身份及有向穿越位置；二者仅服务现river绘制消费者。

- [ ] 写`fine_river_`测试并运行`cargo test --release --lib fine_river_ -- --nocapture`见红：入口已湿、触边未穿越、多交点、缺目标owner、portal落干、窄于网格的槽；LakeOutlet必须源湖湿→干起点，保留下游干路径，再进入接收owner才截断。
- [ ] 河口局部将既有path/宽度/床高作为约束边/节点切分床面，共享节点nodal min-only；方向/床高/槽连通另验，不能用min证明全部。接收格内仅显示终段延到首次合格干→湿交点，P5reach/discharge不变；无合格交点/竞争/预算失败拒绝候选。
- [ ] 同命令绿并运行`cargo test --release --lib hierarchical_rivers -- --nocapture`；图元引用指向任务2片，不新增独立blue mask裁口。独立双审后提交`feat: join river terminals to shared fine water geometry`；UI验收随任务4完成。

## Task 4：两视图、真实点击与取消竞态原子安装

**Files:** Modify `src/app.rs`、`src/app/amplified_mesh.rs`、`src/view/spherical_mesh.rs`、`src/gpu/mod.rs`、`src/gpu/spherical/renderer.rs`、`src/ui/spherical.rs`；Test现模块及`tests/spherical_presentation_integration.rs`/`tests/spherical_presentation_gpu.rs`的直接消费者。

**Interfaces:** 扩展现`AmplifiedDetailPayload`携带`Arc<FineWaterGeometry>`、同身份fill/river/query和GPU准备；`DetailRebuildRequest/Result`携带session、recipe revision、P5 hash、T1/water/约束内容与版本、selection serial。保留现selection调度，不用现u64 selection hash替代语义内容身份。Map/Globe各自selection/query对象同包；不同selection不得混query。

- [ ] 写`fine_detail_`测试，`cargo test --release --lib fine_detail_ -- --nocapture`见红：旧camera答案/AlreadyInstalled晚到、取消后计算完成、edit/undo/open使请求陈旧、GPU上传中失败/超预算，均旧完整包不变且不自动重试；只完成GPU准备才更新installed身份。
- [ ] build detail先共形/水片/终段/query验证，再投影和上传；fill直接取片类别，岸线取湿干公共边，科学payload不改。Inspector保留P5科学高程/kind并并列“派生几何高程”“显示水体/水位”，值来自当前已安装query。地图接缝、球体边缘、退化边都查同片身份。
- [ ] 同命令绿，运行`cargo test --release --test spherical_presentation_integration`与`cargo test --release --test spherical_presentation_gpu`（GPU金样只在审计适配器）；记录帧代理/上传/取消/峰值，不能称真实present latency。
- [ ] 在包络允许的研发候选中实开native窗口，拍同海岸/湖岸/河口Map与Globe及Inspector，记录地理点、LOD、构建hash/UTC与Library ID；不同zoom数字不当同尺度。A红或包络未定不得以失败算法截图宣称交付。双审后提交`feat: install fine coast fill and picking atomically`；最终用户上手仍待任务7。

## Task 5：局部L2+变体、recipe版本与一次重开

**Files:** Modify `hierarchical_derivation.rs`、world `fine_water_geometry.rs`、`app/authoring.rs`、`project_file.rs`、`project_ui.rs`、`spherical_presentation.rs`、`src/app.rs`、`ui/spherical.rs`；测试优先各模块现`#[cfg(test)]`。

**Interfaces:** `LocalDetailConstraint`含ID、科学context hash（不含约束）、SurfaceRef/topology指纹、root CellId/sector/HierarchicalPath、variant seed、地址/T1新息/成员/边界语义版本；`AuthorInputs.local_detail_constraints: Vec<LocalDetailConstraint>`。`AuthorInputs::scientific_context_id() -> Result<[u8; 32], AuthorInputError>`编码原科学输入；`canonical_bytes/RecipeId`再编码canonical排序的完整约束，避免自引用。`HierarchicalEvaluator::with_local_detail_constraints(self, constraints: &[LocalDetailConstraint], context_id: [u8; 32], surface_ref: SurfaceRef) -> Result<Self, TerrainAmplificationError>`验证成员/绑定，只分流L2+内部新息，L0/L1/区界与相邻约束共同边保持未编辑值。

- [ ] 写`local_detail_`测试，`cargo test --release --lib local_detail_ -- --nocapture`见红：同canonical域/祖先嵌套拒绝、相邻约束边同值、遍历/线程不改结果、seed重做不重随机、L0/L1无覆盖、非法外部路径在调用`HierarchicalPath::new`前返回错误。先证明成员/边界分流，再冻结对应版本。
- [ ] 实现真实图选择稳定地址与范围、变体确认/删除/恢复/显式重绑定；一次确认一条before/after事务，Escape丢提案、no-op不入栈、新命令清redo；不做科学预览，仍显式重建，旧图标“草稿约束已改，地图仍为上次结果”。重绑定须在新发布图重新选地址、验冲突，不能只换fingerprint。
- [ ] 首版局部输入更改保留P5身份命中复用，整体失效派生selection与river path/carve缓存（尚无局部依赖闭包证明）；完整关联路径重求后重新构造片/终段/query，不能只改选区或河口。科学输入仍走bundled完整graph。
- [ ] 明确`PROJECT_FORMAT_VERSION` 1→2、author canonical编码域版本1→2；复用任务1的T1版本，water/address/constraint新息/成员/边界语义各从显式版本1开始，由唯一常量引用，冻结后语义变更才递增。旧v1读为无约束但独立判断算法兼容，T1变更须显式升级副本。GeneratorIdentity登记显示语义版本，未知类型/枚举失败，旧原件不覆盖；65,536上限不扩大。world/节点/片/undo历史不进文件。
- [ ] 写并运行`cargo test --release --lib project_ -- --nocapture`和`cargo test --release --lib authoring -- --nocapture`：恰上限/超限、未知版本、checksum、原子保存外部冲突；compatible reopen一次、静态context不合不启动、P1完成/昂贵graph前SurfaceRef不合终止不重试、解析失败保留会话、打开构建失败按Phase A保留且不把旧项目当新结果；冷重开无undo历史。覆盖任务4晚到竞态。
- [ ] 实开UI完成变体→重建→undo/redo→重建→保存关闭→兼容重开现算；记录2D/3D真实图与失败保留。schema/身份最终改动运行`cargo test --release`，离线ignored仅前述指定项。双审后提交`feat: persist bounded local detail variants as recipe inputs`。

## Task 6：独立CPU账校准与证据驱动单项优化

**Files:** 自有`../sekai-coast-evidence/cpu/`探针/runner/收据；只有归因可信后才修改实际被证据指向的一个生产模块及其测试，任务实现记录明确该路径，不提前预建优化框架。

**Interfaces:** native小探针同时读取`Instant` wall、GetProcessTimes与GetThreadTimes返回FILETIME kernel+user（100ns），记录PID/thread生命周期/成功返回/单位；受限cycle只报cycle。现依赖windows-sys dev features够用则复用；诊断scope不是公开产品API，不将旧hooks整包加入产品。

- [ ] 先写并运行小自检（计算、等待、一个worker及线程结束取账）：每个OS失败必须显式错误，单调/非负、线程覆盖与nested排他和验证；不强令CPU≈wall。命令与源码固定到`../sekai-coast-evidence/cpu/calibrate.ps1`；红先修采集，不跑世界、不猜热点。
- [ ] WPR已知0xc5585011策略失败不重复路线、不改策略；仅权限实际可用才采自有process CPU/scheduling。受限替代覆盖实际参与线程、粗算子排他CPU/cycle和观测开销；不能解释计算/非运行区间或缺覆盖就明确“归因受阻”，完成独立证据任务且不假做优化。
- [ ] 前置可信后至多一个Draft42冷观察，独立自有runner、同原graph/输入/线程/缓存；固定观察`run-observation.ps1`，记录wall/CPU、调度/非运行证据、所有样本/开销与产物hash。只细分有证据残项，分清requested bytes、实际copy、live/peak RSS与GPU；不用28个wall桶自称CPU profile。A红不阻止此独立路径。
- [ ] 依据归因挑一个冗余/布局/缓冲/cache/调度候选：先保留已有科学/身份回归的绿色基线，再记录真实cost冗余或重复工作证据，做最小工程改动与paired复测；无可信赢家则记录阻碍结束。纯等价优化不得为了红绿形式制造numerical mismatch或自拟cost阈值；若实际包含新功能/bugfix，其对应最小行为回归仍须先实见红再修复。不得无新证据重试撤回parallel/hoist，不改方程、CFL、profile或门。
- [ ] 对当轮正确性基线做串行配对A/B，保存次序/原值/条件，完整产物身份相同；收益近噪声只补相关重复。科学/身份及真实UI回归通过且收益稳定才保留，否则撤回；旧版→细岸版耗时差不能归因优化。Draft10–20s/Standard≤60s目标原样，未达单列；不为校准先扩Standard/seed。双审后以实际结果提交`perf: ...`或仅`docs: record calibrated CPU accounting and blockers`。

## Task 7：全分支证据核对与用户验收交接

**Files:** 更新本计划checkbox/设计修订与证据索引，成果用原生Library artifact存档，不把world/运行历史写进recipe。

- [ ] 独立整分支spec/quality review核对P1-A、所有任务日志、身份/精度接口、停止原因；修复真实问题，只有新变化/失败才重复相应测试。文档commit先`git diff --check`，代码提交执行Global Constraints门；记录平台/资源限制，不能把历史成功写成新执行。
- [ ] 交付每张真实窗口图的Library ID、exe/build hash与日期，并给用户步骤：启动本worktree构建的native app→同位置Map/Globe海/湖/河口→点击科学与派生值→变体/显式重建→undo/redo→保存/冷重开→取消与版本失败保留。用户本人验收前状态明确为“代理验证完成，待用户UI验收”。
- [ ] 若A红、包络未冻结或CPU归因受阻，交接具体失败证据与允许保留的独立成果，任务2–5/优化按实际标阻止；不请求泛化确认、不放宽门、不称未实施能力完成。全部条件满足再报告交付，性能缺口仍独列；本地commit，不push/merge。

## 每项承重技术的出处

- P5中心节点、Shepard p=2与原P1海洋积分：规格§13、`surface_water_geometry.rs:476`及冻结2026-08-24 transient §4/R1；Shepard(1968)出版记录，确切公式以现生产助手为SSOT。
- 点值/均值与不相容约束：Sweldens & Schröder(1996) §1.6.3/§2.4/§2.10及P1-A§2数学反例；本轮不实施未证明球面average interpolation。
- T1中点、种子、幅度/层级LOD：冻结T1v2 §2/T1v2.1、Fournier等(1982)/Lewis(1987)，现`hierarchical_derivation`/`amplified_mesh`；不另创系数或保证相机无关逐点LOD值。
- 湿干裁剪、共形与局部约束：P1 production解析积分、VTK vtkTriangle/vtkContourFilter 9.7.20260930、CGAL 6.2.1 Constrained Triangulations（原链接见规格§13）；方法依据不担保球面参考权重、连接或fine-volume，本计划以任务2/3证伪。
- 唯一显示几何：规格核对的Azgaar提交574861c583177ebfd078733d6eb43d67a9e4fb65共享coastline/landmass消费者；只借事实源分工，不照搬噪声为科学结果。
- CPU单位/线程覆盖：Microsoft GetProcessTimes/GetThreadTimes/QueryThreadCycleTime；直接出处与限制见`../sekai-diagnostics-evidence/cpu/cpu-report.md`，cycle不换秒、wall不当CPU。
- 作者事务/原子安装/recipe-only：已批准Phase A R2及基线7143d1e4、现authoring/project_file/project_io与detail worker；新显示版本显式登记，既有文件上限与质量门不改。
- 最小真实语料与失败基线：`../sekai-next-design-evidence/contracts-note.md`、`../sekai-diagnostics-evidence/t1/results.md`、`../sekai-diagnostics-evidence/cpu/cpu-report.md`；仅Draft42，旧观察不代新候选验收，新包络必须先测后钉。
