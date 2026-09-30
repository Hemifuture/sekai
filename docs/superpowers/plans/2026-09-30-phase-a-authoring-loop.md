# Sekai 首期 A 作者闭环 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**执行登记（2026-09-30）：用户已批准八项实施，T1–T7 已实施并独立审查；T8 执行中。原 design-only/待执行文字是批准前的历史记录，由本登记覆盖；R2 技术范围不变。用户本人 UI 验收仍 pending，不宣称整体交付。**

**Goal:** 交付不存世界快照的输入配方作者闭环：编辑/撤销→显式异步重建→验证后原子发布→安全保存→兼容项目重开自动生成一次，并完成证据驱动的性能迭代。

**Architecture:** 在现有 app 构建/发布路径上增加具体输入历史、请求身份和项目文件边界；领域校验和 engine artifact 指纹保持唯一事实源。文件与生成工作在线程执行，UI 只消费完整结果；复用现有双阶段 graph，不建立第二套调度或通用命令框架。

**Tech Stack:** Rust 2021/MSRV 1.85；现有 egui/eframe 0.31、wgpu、serde/serde_json(float_roundtrip)、blake3、thiserror、std thread/mpsc。Windows 原子文件替换需要将已有 windows-sys 0.61 增补为 windows 运行时依赖并启用实际使用的文件 API；不增加原生文件对话框依赖。

**Spec:** [已批准 R2](../specs/2026-09-30-phase-a-authoring-loop-design.md)，用户批准时间 2026-09-30 02:49 UTC。执行者必须同时阅读设计与 AGENTS.md。

## Global Constraints

- 不保存世界 bundle、阶段缓存、历史世界快照、显示网格或求解轨迹；运行中的旧有效世界允许留在内存。
- 兼容合法项目打开后自动重建一次；日常编辑显式重建；取消或失败不自动重试。
- 最多一个实际全链构建；请求必须带 epoch/build ID/revision；晚到结果不得发布。
- 撤销重做恢复输入，不保证即时恢复世界；历史限会话内。
- 原生 desktop 优先；保持 wasm lib 编译兼容，不承诺本期浏览器后台线程体验。
- 生产 graph 为 ClimateWorkDomainStage + bundled CausalNaturalFormationStage(P2–P5)；不承诺局部重算。
- Draft 10–20 s、Standard ≤60 s 是待验证目标；不承诺达标，不降低质量、不裁剪科学状态、不改冻结门槛。
- 不改 t1v2-2-stage1-candidate，不重做已撤回的细粒度并行，不纳入未裁定的模分裂、FAS/CFL/周期或科学模型变化。
- 未知 schema/算法/依赖明确失败；升级副本须显式选择，原件保留；跨平台不冒充逐位恢复。
- 已获准按下列任务实施并本地提交，一任务一审查/提交遵循仓库纪律；不 push/merge，不自动扩大到未裁定科学方向。

## Review Focus

以下五类是最容易漏掉的用户失败路径；已落实到任务测试，不能只依赖正常路径：

1. 同一 egui pass 既提交输入又收到旧候选：编辑先使请求失效，不能先发布再撤销（T2）。
2. 保存 A 期间已编辑到 B：A 可以保存成功，但 B 必须仍未保存；旧项目 I/O 结果不能标记新项目已保存（T1/T5）。
3. 打开新项目等待旧 worker 退出期间又编辑/取消：自动启动意图被消耗，不能后来悄悄生成（T2/T5）。
4. 未知生成版本/隐藏未知字段/超大或重复 JSON 键：不得默认补值后自动生成，原件和当前会话保持不变（T3）。
5. Windows 文件被占用或替换中断、外部修改：不得丢唯一旧配方或误报成功；报告真实恢复位置（T4）。

---

## 开工基线与文件职责

编写计划时 HEAD 为 a716138ad619ebc2cd335b7a7fdaa6b79c6876ab；main 比本地 origin/main 领先三个提交，工作树干净。只读核对发现下列接入点：

- src/app.rs：TemplateApp、PendingWorldBuild、WorldBuildCompletion、request_spherical_world_build、poll_world_build、settle_staged_world_build、install_initial_spherical_candidate、install_replacement_candidate；update 当前先画控件再 poll，再处理 new_seed，T2 必须调整事务顺序。
- src/app.rs：formation_surface 结算早于取消判断；放入候选事务或证明为严格键控的非发布缓存，不默认忽略。
- src/app.rs / src/app/spherical_presentation.rs / src/gpu/spherical/renderer.rs：当前替换和附属 amplified 资源安装边界；新准备类型若需跨模块使用，只在 src/gpu/spherical/mod.rs 以 pub(crate) 重导出，不扩大公共 API。
- src/app/frame_stats.rs、tests/generation_latency.rs、src/generators/natural/formation/causal.rs 的 timing_probe：现有观测入口。
- tests/support/causal_formation.rs：真实 Formation fixture，避免重复生成同一世界。

新增文件按具体职责收敛为：

| 文件 | 职责 |
| --- | --- |
| src/app/authoring.rs | AuthorInputs、输入历史、请求 token、dirty/save/revision；不含文件/GPU |
| src/app/project_file.rs | V1 配方 wire、严格解码、校验和、生成身份及兼容判定 |
| src/app/project_io.rs | 原生读写、备份/替换、文件冲突；cfg 隔离 native I/O |
| src/app/project_ui.rs | 文件/编辑动作、路径输入框、未保存/升级确认与 I/O 轮询装配 |
| src/app/build_metrics.rs | app 端请求/生成/发布耗时和交互样本摘要，不保存世界 |

现有 src/app.rs 保持装配，不全面搬迁。必要时把该任务的新测试放在上述文件私有 tests 模块；app 私有集成场景留在 src/app.rs 的 phase_a_tests 子模块。无需为测试把模块提升 public。

共用类型：本计划接口默认 pub(super)；除既有类型外，新类型都由下列任务定义。不创建项目公共 API。新增错误用 thiserror，错误分类保留而非只存无上下文字符串。

执行开始（仅在计划批准且方法选定后）先检查 git status/HEAD/候选分支；若用户工作变化，重读受影响代码并修订接入点，不覆盖。按 using-git-worktrees 创建/确认隔离执行位置；现在不创建。所有下列 shell 命令在该执行根目录串行运行；不修改 RUSTFLAGS。应用占用 release 输出时，用 AGENTS.md 规定的 target/probe 隔离构建。

## T1：输入事务与会话事实源

**Files:** Create src/app/authoring.rs；Modify src/app.rs、src/app/pipeline.rs（仅移动/复用已有默认值与 serde helper）；Test authoring.rs::tests。

**Interfaces:**

- AuthorInputs：字段沿现有名称 world_seed、spherical_space_spec、world_pipeline、formation_quality_profile、formation_spec、tectonic_spec、relief_spec、geologic_spec；不存结果。
- AuthorInputError；RecipeId([u8;32])（仅配方身份，不替代 engine 内容身份）。
- AuthorInputs::validate(&self) -> Result<(), AuthorInputError>。
- AuthorInputs::recipe_id(&self) -> Result<RecipeId, AuthorInputError>：固定字段顺序 wire、稳定数值编码，输入校验后计算；T3 复用同一编码。
- AuthoringSession::new(inputs: AuthorInputs) -> Result<Self, AuthorInputError>。
- commit(&mut self, label: String, after: AuthorInputs) -> Result<bool, AuthorInputError>；undo(&mut self) -> bool；redo(&mut self) -> bool。
- draft(&self) -> &AuthorInputs；revision(&self) -> u64；is_dirty(&self) -> bool。
- SaveStamp { epoch: u64, recipe_id: RecipeId }；capture_save(&self) -> SaveStamp；mark_saved(&mut self, stamp: SaveStamp) -> bool。只接受本 epoch，保存标记指向实际落盘配方，is_dirty 比较当前配方。
- BuildToken { epoch: u64, request_id: u64, revision: u64, recipe_id: RecipeId }；begin_build(&mut self) -> BuildToken；invalidate_build(&mut self)；accepts(&self, token: BuildToken) -> bool；mark_published(&mut self, token: BuildToken) -> bool。
- reset_project(&mut self, inputs: AuthorInputs, saved: bool) -> Result<(), AuthorInputError>：递增 epoch、清历史/发布身份/在途 token；saved=false 用于升级副本。

- [ ] 写 authoring tests：noop 不增 revision；无效 NaN/领域越界不改草稿；预设联动一条命令；undo 后新命令切断 redo；redo seed 相同；相同值撤回不能复活旧 token；保存 A 再编辑 B 后 mark_saved(A) 仍 dirty；reset 后旧 stamp 被拒绝。
  关键断言形式：`assert!(!session.accepts(old)); assert!(session.is_dirty()); assert_eq!(session.draft().world_seed, recorded_seed);`。不要为这些用真实地图 fixture。
- [ ] 先红：`cargo test --lib app::authoring::tests -- --nocapture`，应因缺失接口/断言不满足失败；修正测试编译问题后确认失败确为缺少行为。
- [ ] 实现具体快照历史与 token 逻辑；直接复用领域 validate。输入全等用规范 RecipeId，浮点 NaN/Inf 先拒绝；合法零值在规范编码中将 -0 归为 +0，不改变非零输入；字段默认值只保留一份。
- [ ] TemplateApp 改为一个运行时 AuthoringSession 事实源；把旧扁平自动持久化字段收敛为仅反序列化的 LegacyAppInputsWire，读取后立即转 AuthorInputs。新持久化 wire 只存配方和偏好，不让 runtime 与 legacy 两份字段同时可编辑。编辑中的临时 AuthorInputs 不算已提交历史。旧自动存储恢复需识别生成身份：缺身份的旧格式标为未保存导入，不伪造旧版本可复现性。
- [ ] 同命令转绿；补跑 `cargo test --lib app::tests::persisted -- --nocapture` 和 `cargo test --lib app::tests::opt_in_frame_scenario -- --nocapture`，检查实际选中了已有测试（不能把 0 tests 称为通过）。
- [ ] 任务审查输入字段是否遗漏、旧自动存储是否只导入一次。未来获准执行时提交本任务，不夹带其他改动。

## T2：请求竞态、取消及完整发布事务

**Files:** Modify src/app.rs、src/app/spherical_presentation.rs、src/gpu/spherical/renderer.rs（仅候选资源准备/提交边界）；Test src/app.rs::phase_a_tests，并保留 app::tests 的既有取消回归。

**Interfaces:**

- PendingWorldBuild 和 WorldBuildCompletion 均携带 T1 BuildToken；worker 捕获 AuthorInputs，不再读取变动的 app 字段。PendingWorldBuild 持有 native JoinHandle；收到 completion 不等同线程已退出，只在 is_finished() 后 join 回收，UI 不阻塞等待，再允许下一 worker 启动。
- request_spherical_world_build(&mut self) 延续现有签名，只在无实际 worker 时申请新 token。
- 新增 PendingOpenBuild { epoch: u64, recipe_id: RecipeId }，仅表示一次成功 open 的自动启动意图；运行期不序列化。
- try_start_open_build(&mut self) -> bool：只有意图仍匹配且旧 worker 已退出才消费并启动；编辑/取消/reset 清除意图。
- PreparedWorldPublication（app.rs 私有）：候选 presentation、已准备的 amplified/河线/投影资源、对应表面关联、报告及 token。
- prepare_world_publication(&self, completion: WorldBuildCompletion, render_state: &RenderState) -> Result<PreparedWorldPublication, AppRuntimeError>。
- commit_world_publication(&mut self, prepared: PreparedWorldPublication, render_state: &RenderState) -> Result<(), AppRuntimeError>。
- 保留既有 SphericalPresentationCandidate、SphericalRendererPreparer、try_replace 语义；GPU准备过程不能把候选 buffers 装到当前资源，最终提交再次检查 token/取消；提交部分不再做可失败的 CPU 构建。不重新设计渲染器。

- [ ] 写 phase_a_tests：同一 pass 编辑与 completion 相遇时旧图不变；cancel 后改回同配方仍拒绝；换 epoch 后旧 worker/LOD completion 拒绝；失败/断开/GPU 准备失败时世界、表面关联、缓存、河线/mesh、摘要来源均不变；首次失败无半发布。用现有 tiny candidate/GPU fixture 和人工 channel completion，不启动全链求解。
- [ ] 先红：`cargo test --lib app::phase_a_tests::publication -- --nocapture`。
- [ ] 把控件生成的输入事务/new seed/undo/redo 与取消处理放到 poll/settle 之前；保留“接收后至少下一 egui pass 结算”规则。worker 未退出时保留取消中状态，不并发；断开和 error 也要消耗 token/清自动意图。
- [ ] 实现 prepare/commit；formation_surface 只在成功发布时与其消费者一致提交，或以证据证明私有键控缓存独立安全。派生 detail 的既有 serial/hash 继续使用，并加世界发布身份防跨世界；视角取最终当前值，实体选择按有效身份调和/清空。
- [ ] 转绿并跑 `cargo test --lib app::tests -- --nocapture`、`cargo test --test spherical_presentation_integration --test spherical_presentation_gpu -- --nocapture`。至少保留既有 same_frame_cancel、pointer_down_in_the_settling_pass、held_cancel_button、formation_channel_disconnect 的实际执行证据。
- [ ] 审查副作用是否发生在 final guard 前、候选资源是否仍隐式写当前 renderer。未来获准执行时独立提交。

## T3：配方格式 V1 与生成版本兼容

**Files:** Create src/app/project_file.rs；Modify src/app/pipeline.rs（生成兼容版本）、src/app.rs（模块声明）、src/ui/spherical.rs（共享严格偏好 wire）、src/app/spherical_presentation.rs（共享隐式模型输入构造）；只在需要复用现有事实时访问 engine graph descriptors、rules、profile API，不复制其常量。Test project_file.rs::tests。

**Interfaces:**

- ProjectFile { format_version, inputs: AuthorInputs, generator: GeneratorIdentity, view: ProjectView, extensions: BTreeMap<String, serde_json::Value>, upgraded_from: Option<GeneratorIdentity> }；wire 另有 magic/checksum，字段全部显式。
- ProjectView：只保存当前视图模式、投影、globe/map camera、字段选择和减少动效；复用 ui/spherical.rs 已有 SphericalCanvasStateWire/CameraWire 的语义转换，抽取可共享的严格偏好 wire，旧 app 恢复层独立保留兼容默认值。项目编码时剔除 selected_entity、vector_phase 和派生数据，不能直接序列化整个 SphericalCanvasState，也不再复制相机转换公式。
- GeneratorIdentity：pipeline、product_recipe_version、stage descriptors 的 namespace/id/version、profile 解析计划、内建规则/依赖内容绑定哈希；另记录 float_platform_canary 作为环境标识。程序版本不参与语义兼容。
- current_generator_identity(inputs: &AuthorInputs) -> Result<GeneratorIdentity, ProjectFileError>：读现有 graph.descriptors、profile.resolve、内建 resolved 输入/规则身份，不运行 P1 或 P2–P5。
- spherical_presentation.rs 新增 pub(super) formation_implicit_models() -> Result<(ResolvedClimateInput, ResolvedHydroErosionInput), SphericalPresentationError>，将现有 build_spherical_formation_external_artifacts 内部默认模型构造移入；生成和身份计算共同调用，不能各写一份 ClimateSpec/HydroErosionSpec 默认绑定。其他模型 ID/输入按同样原则复用当前构造及其领域版本，不把可变作者值混成算法升级。
- compare_generator(saved: &GeneratorIdentity, current: &GeneratorIdentity) -> Compatibility；枚举 Exact、PlatformConfirmationRequired、UpgradeRequired(Vec<String>)。
- encode_project(file: &ProjectFile) -> Result<Vec<u8>, ProjectFileError>；decode_project(bytes: &[u8]) -> Result<ProjectFile, ProjectFileError>。
- upgrade_copy(file: &ProjectFile, current: GeneratorIdentity) -> ProjectFile：只适用于已识别合法输入，保留原身份到 upgraded_from，不能用来“修复”未知核心 schema。
- 错误分类 Format/Limit/Checksum/Input/UnsupportedSchema；算法差异用 Compatibility，不混成解码成功后的静默覆盖。

- [ ] 写测试：配方（含非活动值）往返；相同内容不同 JSON 空白/对象顺序得到相同配方身份；checksum 不包含自己；NaN/负零策略遵循 T1；未知核心版本/字段/枚举、重复核心键、截断/超限/超深结构失败；普通 extensions 往返；序列化键集合不含世界、缓存、历史或 dense arrays。`assert_eq!(decoded.inputs.recipe_id()?, original.inputs.recipe_id()?);`
- [ ] 写兼容测试：stage version/规则哈希/profile plan 变动各触发 UpgradeRequired；只环境 canary 不同触发确认；app 版本变化不擅自升级生成身份；升级副本保留源身份，原结构字节不改；默认 Formation profile=Standard 时应按 profile 的 authoritative_target_cell_count 解析实际计划，不误用 app 旧 target_cell_count=20,000 导致拒绝合法项目（Formation 的该字段不参与表面选择，原作者值仍保留）。
- [ ] 先红：`cargo test --lib app::project_file::tests -- --nocapture`。
- [ ] 实现显式 V1 wire、重复键拒绝和严格核心字段；固定顺序 typed payload 加 extensions 的 BTreeMap 编码做校验和，复用 T1 输入编码，不依赖用户 JSON 排序。沿用 serde_json 默认递归限制；文件安全上限依据实际最大全量参数/偏好/扩展 fixture 字节测量后登记单一常量与理由，作为资源限制，不伪称科学阈值。
- [ ] 身份覆盖审计：在 pipeline.rs 定义唯一 PRODUCT_RECIPE_GENERATOR_VERSION 兼容纪元，兜住 graph 外的 P1、RNG、profile 数值策略等语义变化；已有 stage version/规则内容必须直接读取，不能手抄第二份。文档注明这些语义变动必须更新所属既有版本或该纪元。不存在外部数据时不虚构数据包；未来依赖只有实际消费者才加入。
- [ ] 转绿同命令；运行 `cargo test --test world_formation_spec --test geologic_spec -- --nocapture` 加本任务单元测试。ReliefSpec 的严格校验用 `cargo test --lib relief_spec -- --nocapture`，确认选中既有模块测试，不把 0 tests 当通过。
- [ ] 审查清单覆盖全部实际生成输入、未知内容未被 serde(default) 吞掉。未来获准执行时独立提交。

## T4：原生文件事务与失败保全

**Files:** Create src/app/project_io.rs；Modify Cargo.toml/Cargo.lock（只为生产 Windows 文件 API 依赖，实施时才更新）；Test project_io.rs::tests，Windows 专属用 cfg(windows)。

**Interfaces:**

- DiskStamp([u8;32])：原始磁盘字节哈希，区别于配方身份。
- LoadedProject { file: ProjectFile, disk_stamp: DiskStamp }。
- SaveExpectation = NewFile | Unchanged(DiskStamp) | ConfirmedOverwrite(DiskStamp)；后者是用户确认时再次捕获的磁盘版本，不是“任意覆盖”。
- SaveReceipt { path: PathBuf, disk_stamp: DiskStamp, backup: Option<PathBuf> }。
- read_project(path: &Path) -> Result<LoadedProject, ProjectIoError>。
- save_project(path: &Path, file: &ProjectFile, expected: SaveExpectation) -> Result<SaveReceipt, ProjectIoError>。
- ProjectIoError 区分 Conflict、Read/Write/Sync/Replace、Format，并带确实存在的恢复路径；不输出成功 receipt 后再把关键错误丢弃。

- [ ] 写真实小临时目录测试：首次保存→读取；已存在但 NewFile 不覆盖；原件被外部修改时报 Conflict；损坏不切会话；写失败保留原件；替换失败保留可恢复配方并报告路径；保存成功后原文件内容等于编码且 backup 为旧配方。临时目录由测试专用 RAII 管理，删除只限已确认测试根。
- [ ] Windows 用真实被占用目标制造 sharing violation，验证返回的路径确实可读取对应旧/新配方。sync/replace 罕见错误可用局部 cfg(test) failpoint，不为测试引入公共 FS trait。
- [ ] 先红：`cargo test --lib app::project_io::tests -- --nocapture`。
- [ ] 实现同目录唯一 create_new 临时文件、write_all、sync_all、重新校验和、替换前再次比对 DiskStamp。Windows 使用 ReplaceFileW 的备份模式（已存在目标）和无覆盖重命名（首次文件），补 windows-sys 0.61 Win32_Storage_FileSystem/必需 Foundation features，unsafe 局限窄 wrapper 并写安全依据。非 Windows 原生保留备份再同目录替换；不声称绝对断电安全。
- [ ] 错误路径检查文件实际状态，保留至少可恢复旧配方；成功后才返回 receipt。外部修改检测是检查窗口的冲突保护，不声称任意第三方并发写的跨进程 CAS；检测到更改不得覆盖。打开路径是用户选择的，不执行其中代码。
- [ ] 同命令转绿；Windows 同时验证中文/空格路径、只读或被占用目标。native 模块 cfg 不污染 wasm。
- [ ] 未来获准执行时独立提交；提交说明记录为什么已有 windows-sys 从 dev 增为生产依赖，不安装未使用对话框库。

## T5：文件/编辑 UI 与重开自动一次生成

**Files:** Create src/app/project_ui.rs；Modify src/app.rs、src/ui/spherical.rs（仅提取/应用已有偏好，必要时）、src/app/authoring.rs（接入现有接口，不重复状态）；Test project_ui.rs::tests、src/app.rs::phase_a_tests。

**Interfaces:**

- ProjectAction 枚举：New、Open(PathBuf)、Save、SaveAs(PathBuf)、Undo、Redo、CancelBuild、RetryBuild、Exit。
- PendingProjectIo：I/O id、发起 epoch、SaveStamp（保存时）、单次 receiver；同一时刻一个项目 I/O，生成可独立继续。
- dispatch_project_action(&mut self, action: ProjectAction, ctx: &egui::Context)；poll_project_io(&mut self, ctx: &egui::Context)，均为 TemplateApp 方法。
- ProjectUiState 持有路径输入/待确认动作/当前 disk stamp/升级提示；配方脏状态仍只读 AuthoringSession。
- 同步 I/O worker 使用 T4 read_project/save_project；completion 先检查 io id/epoch，再决定更新路径、保存身份或切项目。关闭/打开的待执行动作也绑定 epoch。

- [ ] 写测试：文本编辑没有提交不能显示已保存；拖动多帧只一历史项，Escape 还原；undo/redo 快捷键在文本输入焦点中不夺走文本编辑 undo；预设全部联动；显式点击重建才启动（open 例外）。
- [ ] 写 open/save 流程测试：合法 open 只调用一次启动，取消/失败不自动重试；等待旧 worker 时 edit/cancel 清自动意图；save(A) 期间 edit(B) 不清 dirty；旧 epoch save 完成不改新项目；unsaved→Save→失败不继续 open/exit；拒绝升级不切会话；升级只生成无原路径绑定的新项目并要求另存为；平台确认前不生成。
- [ ] 先红：`cargo test --lib app::project_ui::tests -- --nocapture` 和 `cargo test --lib app::phase_a_tests::project_flow -- --nocapture`。
- [ ] 实现原生文件菜单 新建/打开/保存/另存为/退出，编辑菜单 Undo/Redo。使用 egui 小型路径输入对话框（支持粘贴绝对路径、中文/空格、清楚显示目标），不在 UI 中暴露 schema/hash 内部细节；无原生 picker 依赖。文件打开/编码写入在一个短 I/O worker，UI poll 不阻塞。
- [ ] 关窗拦截含系统关闭：若 dirty 显示 保存/放弃/取消；保存失败继续留在会话；用户确认退出才取消生成并退出。打开成功前保留旧会话；成功后清新项目 Published，旧 epoch worker 留到退出但不能再安装。
- [ ] 显示“项目已打开，正在按保存的配方生成”“参数已修改，地图仍为上次有效结果”“正在取消”“保存失败”等准确状态。列表/摘要来源是 Published，控件来源是 Draft。视图只影响表现；缩放不污染生成历史。
- [ ] 同命令转绿，跑 T1/T2 的回归过滤。T3/T4 测试覆盖文件细节，不在 UI 层重复所有损坏排列。
- [ ] 未来获准执行时独立提交。此任务通过只是 UI 自动化/状态证据，最终用户上手留到 T8。

## T6：可比较的性能观测与基线证据

**Files:** Create src/app/build_metrics.rs；Modify src/app.rs、src/app/frame_stats.rs；只有现有报告不足以定位时才 Modify src/generators/natural/formation/causal.rs 的观测位置，不改数值路径。Modify tests/generation_latency.rs 仅复用已有构建助手增加非门槛观察入口，不改两个既有预算测试。Create docs/superpowers/measurements/2026-09-30-phase-a-performance.md（未来实施测量时创建，不在本轮造数据）。

**Interfaces:**

- BuildMetrics { token: BuildToken, outcome: BuildOutcome, spans: Vec<TimingSpan> }；BuildOutcome = Published | Cancelled | Failed | Stale。
- TimingSpan { phase: BuildPhase, start: Duration, duration: Duration, parent: Option<usize> }。
- BuildPhase 为实际操作枚举：OpenOrRequest、WaitWorker、InputAndSurface、Graph、Validate（仅能单独观察时）、Presentation、GpuInstall、FirstSubmittedFrame。不把 app submitted frame 称真实 present。
- BuildMetrics::record(&mut self, span: TimingSpan)；finish(&mut self, outcome: BuildOutcome)；对 clock 只通过参数传 Duration 测试，不 sleep。
- frame_stats.rs 增加原始 interval 的 p50/p95/p99/max 摘要，继续标为 update 指标；不影响生成身份/项目编码。

- [ ] 先写 build_metrics 测试：父/子 span 不双计、取消/失败不是成功样本、观测不进项目 wire/recipe_id；frame_stats 测试用固定 interval 序列验证分位与 max。
- [ ] 先红：`cargo test --lib app::build_metrics::tests -- --nocapture`、`cargo test --lib app::frame_stats -- --nocapture`。
- [ ] 实现 app 时间点串联已有 BuildReport 阶段耗时；保留原 generation_latency 口径与另一个端到端口径，不能把旧门槛挪到更窄计时。细阶段使用既有 timing_probe，若需生产 span 仅在对应真实因果边界增加单调时钟，不拆 graph/不发布 solver 中间态。
- [ ] 转绿；加入 ignored `observe_phase_a_latency`（seed 42；环境变量 SEKAI_PHASE_A_PROFILE=draft/standard；SEKAI_PHASE_A_RUNS 控制顺序冷构建次数），复用原 measure 的构建路径与验证，输出每次 surface/graph/total、成功/失败及身份，再在同一运行中测一次同输入内存热缓存。不得更改原 20/60 秒断言。
- [ ] **证据点 E0：记录改动前与闭环完成后的同机基线。** 在执行隔离工作树保留原基线 revision；各 revision 编译结束后再计时，顺序运行，记录 CPU/GPU/OS、电源、线程、尺寸、profile、输入、commit、缓存和原始行。不要切回覆盖用户 main。
  命令（PowerShell，后续执行时）：
  `$env:SEKAI_PHASE_A_PROFILE='draft'`；`$env:SEKAI_PHASE_A_RUNS='3'`；
  `cargo test --release --test generation_latency observe_phase_a_latency -- --ignored --exact --nocapture --test-threads=1`。
  再 profile=standard、runs=1 执行同命令；最后 Remove-Item Env:SEKAI_PHASE_A_PROFILE/Env:SEKAI_PHASE_A_RUNS。旧 revision 没有观察入口时用原两个预算测试保存原始数据，不能因此修改旧分支代码；热缓存没有原版对照时明确标注。
- [ ] 使用既有 `cargo test --release --lib stage_timing_probe -- --ignored --nocapture --test-threads=1` 定位内部耗时，仅在 E0 指出 graph 占比需要它时运行。需 Standard 时使用既有 SEKAI_PROBE_PROFILE=standard，之后清理。
- [ ] UI 采样：`$env:SEKAI_FRAME_STATS='1'` 后 `cargo run --release --bin sekai`，分别记录冷 open、编辑重建、旋转/缩放/字段切换、取消和发布长帧；离开后清变量。原生真实界面验证不可被日志 FPS 代替。
- [ ] E0 数据和可复现命令写入 measurement 文档，区分无日志来源的历史 52–73 s 与本次结果；没有运行就保持“未测”。未来获准执行时提交观测与基线，不能顺带优化。

## T7：证据选定的一轮工程优化（条件任务）

**Files:** 消费 T6 measurement；候选只落到 E1 实测命中的既有文件，不预写未知优化代码。可选调查面为 app.rs/amplified_mesh.rs/field_document.rs、gpu/spherical/renderer.rs、engine/cache.rs/scheduler.rs、或剖面定位的具体生成热循环；每轮先把唯一实际文件及测试写入本任务的“候选记录”，再动代码。禁止以这张列表授权全部重构。

**Interfaces:** 优先保持 T1–T6 及 engine/generator 接口不变；确需接口变化，在候选记录写确切签名和调用者并复核不改 R2。不新增无人消费的并行/缓存 trait。

- [ ] **证据点 E1：填一条候选记录。** 写明测量位置/占比、唯一假设、具体文件与函数、最小回归测试名称、输出逐位不变要求、期望改变的工作次数或分配/上传行为、可能风险、撤回文件范围。只从数据支持的冗余计算→布局/分配→派生显示→内存缓存→调度中择一，顺序可据证据调整；不是按顺序全部做。
- [ ] 若无安全有效机会，记录证据并结束本轮，不强造变更。若主因只能靠待裁定物理/数值策略改写解决，停止该方向并给用户独立设计问题，不能顺带调整 FAS/CFL/周期、T1 或 profile 质量。
- [ ] 针对候选先写最小行为测试；用复用生产 helper 的输入输出/身份相等和明确减少的重复工作契约验证。以具体文件实际测试命令填入 E1 并先红。仅性能、无新可独立行为的布局改动，可用失败的配对性能假设作为红证据，沿用现有正确性测试；不为凑“红”造脆弱 wall-clock 单元阈值。
- [ ] 做一个受控实现，保持输出/运算语义；再跑 E1 的精确命令转绿及直接消费者套件。数值输出改变且无法证明既有契约等价时撤回/升级独立设计审阅，不自行刷新科学金样。
- [ ] **证据点 E2：同机同输入配对复测。** 用 T6 相同命令与缓存条件测改前/改后，保留原始值及噪声；收益接近噪声则补相关重复，不挑最快值。不并行基准，不加 RUSTFLAGS。
- [ ] **证据点 E3：质量与 UI 回归。** 原世界/科学硬门、取消、长帧、发布同步、2D/3D 逐一记录；所选优化不改变数值时比较审计平台指纹与普通领域校验。更大的科学语料只有影响面证据要求时增加。
- [ ] 可重复、超出噪声且无质量/UI 回退才保留；否则只撤回该轮优化，保留测量证据，不触碰用户原工作。A6 已证伪细粒度并行不是候选；无新上下文证据不重做。
- [ ] **证据点 E4：更新结论。** 报告收益、剩余瓶颈和 10–20/60 秒目标状态。需要下一轮时先写下一条同结构 E1 记录再做，不能无界循环。未来获准执行时一轮独立审查/提交；无收益轮只提交证据，不提交无效产品代码。

## T8：整链、发布质量和用户验收交接

**Files:** Test src/app.rs::phase_a_tests；复用 tests/support/causal_formation.rs 的思想/现有 Formation 构建入口，不导出私有 session；Update 本计划勾选和 T6 measurement；不修改 T1 测试阈值或忽略原因。

- [x] 新增 ignored `phase_a_recipe_reopen_formation`（位于 phase_a_tests）用一个 Draft seed 42：生成并校验→改活动目标陆比配方并显式重建/保存→销毁会话和全部内存缓存及旧 GPU 发布→新会话读取并走自动一次生成→比配方/生成身份及同环境产物指纹。另一个小型状态测试覆盖保存尚未生成草稿。现有 T1–T6 契约首次完整 GREEN，未发现装配缺失，未制造 RED；不是只测试 serialize==deserialize。
- [x] 未需装配修补，运行：
  `cargo test --release --lib phase_a_recipe_reopen_formation -- --ignored --nocapture --test-threads=1`。
  预期成功生成且通过生产 bundle 校验；GPU 不可用必须明确阻碍，不静默跳过所有发布断言。CPU 配方重建和真实 UI 证据分别报告。
- [ ] 综合回归：`cargo test --release --lib app:: -- --nocapture`；`cargo test --release --test build_cancellation --test spherical_presentation_integration --test spherical_presentation_gpu -- --nocapture`。若改动 engine 公共身份/schema/共享求解器而影响扩大，按 AGENTS.md 追加 Release 全量；不默认跑全 debug 或大语料。
- [ ] 工程检查（一次通过后不重复刷）：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --all-features -- -D warnings`；`cargo check --target wasm32-unknown-unknown --all-features --lib`。任何实际产品缺陷修复后才重跑受影响检查。
- [ ] 原预算测试分别显式执行并报告实际状态：`cargo test --release --test generation_latency draft_full_chain_within_twenty_seconds -- --ignored --exact --nocapture --test-threads=1`；Standard 用 `standard_full_chain_within_one_minute`。失败保留原始数据，不改门槛/ignore 来伪装通过。
- [ ] 用户本人 UI 验收脚本：用 `cargo run --release --bin sekai` 启动；选择水量/目标陆比编辑、预设与 seed undo/redo；重建中转动 3D/缩放 2D/切字段；候选待发布时取消；观察草稿/旧地图身份和错误；保存未生成草稿，退出并在另一目录打开配方，自动现算一次；取消后不自重试；原版本不兼容时只报差异/明确升级副本。验证地图、图例、河线、面积摘要、拾取同步，检查接缝与 LOD。
- [ ] T1 红门单独注明已有 0.0139>0.01 且 ignored；不必为了本期重复高成本已知红测试，不把它标绿。闭环证据通过与整体地理显示质量阻碍分别报告，最终用户 UI 未验收不能写“已交付”。
- [ ] 交接记录：变更/测试/实测、配方保存限制、取消/失败路径、T1 阻碍、性能目标状态与未裁定科学方向。未来获准执行时完成独立审查/提交；是否合并/推送另依授权，不在本计划自动执行。

## 计划自审（编写时完成，产品验证尚未执行）

- R2 §4–5/§8 → T1、T5；§6–7 → T2、T5；§9 → T3、T4、T5；§10/§12 → T2、T8；§11 → T6、T7、T8；§13 边界 → 全局约束/E1停止条件。
- 五项 Review Focus 均有具体拥有者与测试；保存 epoch、同 pass 编辑、自动 open 意图均未只靠“最后输入哈希相同”判定。
- 所有跨任务新类型/接口在拥有任务定义；没有新增公共框架。I/O、文件 wire、输入历史、发布资源分别验证。
- 候选优化文件和方法刻意留作 E1 的证据决策，不伪造未来剖面；E1 必须落下精确接口/测试才可实施。
- 设计已批准，计划仍待批准和执行方式选择；没有开始代码/基准/子代理实现。

## 执行方式建议（待用户选择）

推荐“逐项独立审查”：8 个任务共享少量严格接口，T2 的取消/发布边界及 T3–T5 的版本/磁盘安全错误容易跨任务放大，提前独立审查比收尾才发现更容易控制返工。任务仍按依赖串行，不并行修改同一 app.rs。代价是每项新的实施/审查上下文。

“连续实现后统一审查”也可：由同一执行者逐项完成，保留所有红→绿、E0–E4证据，最后独立检查整个分支；上下文成本较低，但阶段接口和磁盘失败契约的独立审查较晚。

本轮不替用户选择，不调用实施技能或启动实现代理。用户需审阅计划并选择方法后，才进入执行。

## 每项承重技术的出处

| 技术/判据 | 依据与用途 |
| --- | --- |
| 输入命令、原子最终态、输入为权威、显式迁移 | Jul28 current-slice spec §9.2/§10；批准 R2 §4–9 |
| 延后 egui pass 结算、pointer-down 取消、缓存事务 | app.rs 既有 poll/settle 实现及同文件取消回归；保持已验证输入顺序 |
| 科学领域校验与身份复用 | AGENTS.md；engine/artifact.rs、StageGraph::descriptors、world/natural/profile.rs、ReliefSpec；不新造科学公式 |
| Windows 替换、备份及失败位置 | [Microsoft ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew)；T4 真实失败测试，不夸大断电保证 |
| 同机配对、剖面后优化、无收益撤回 | A6 2026-09-29 冻结设计/修订及批准 R2 §11；并行撤回决定必须保留 |
| Draft/Standard 目标 | A1/A6 用户既有目标及 tests/generation_latency.rs；ignored 不代表通过 |
| 审计浮点平台与普通科学不变式区分 | AGENTS.md、world/spatial/audited_platform.rs |
| UI 最终验收/最小充分测试 | AGENTS.md、批准 R2 §12；用户亲自验收不可由代理替代 |
