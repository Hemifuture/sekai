# 程序化海岸：完整生成链与 UI 原型

用户已于首轮验证后授权“继续，适时截图”。本计划细化首轮计划任务 3，
在原独立工作区执行，不合并其他工作区。目标为可操作的 Draft 实验预设，
不是未经实测的最终质量承诺。

## 实施任务

1. 添加 `ProceduralContinents` 实验预设。先以 serde 名称测试确认当前不支持，
   再接入请求/解析枚举与 UI。旧预设和 Random 权重不变；输入已有 serde 身份
   包含新枚举名，因此不全局递增 stage 版本而扰动所有旧种子的子流。
2. 在初始陆壳分配处接入独立噪声场，沿用已有 Continents 形状频谱，
   按面积选取连续标量场的超水平集。复用 `area_prefix_mask`、平台厚度与
   `MaterialColumn::pure`，不挪用旧基底。单元测试检查可重复、不同于旧掩膜、
   初始陆壳面积预算和材料有效性；不复制算法公式。
3. 用一组 Draft seed 42 经过完整生产链，记录 P2/P3/P5 与终点气候的状态。
   生产校验不得放宽；如失败，保留原因并定位，不扫描种子挑成功样本。
4. 编译实验应用，操作新预设，给用户地表与气候视图截图。截图是实际 UI 状态，
   不使用生成图代替。必要时追加局部海岸截图；用户最终验收仍未替代。
5. 定向测试、fmt、默认/Release Clippy、WASM 检查与构建；独立审查后提交。

## 判定与限制

- 本轮只声明 Draft；界面仅在完整 Formation + Draft 时提供实验选择。
  若已选该预设后切换档位，完整链必须显式拒绝非 Draft 请求。
- 继承 Continents 的陆壳/水量推荐仅用于同参数对照，不宣称新预设已测得
  相同终态陆比。不强制最终陆比与原地图相同。
- 采用新的初态形状会改变地壳配置，因此最终地形和气候可以共同改变。
- 当前生产 P2 重采样缺陷仍可能破坏岸线；观察它，不以美化掩盖。
- 不承诺海岸原型一定优于 Civ Atlas；若仅形状已变但细节仍粗，截图如实展示。
- 运行一次一个进程，无网络压力；构建 `-j 2`，不设 RUSTFLAGS；每次生成最多
  10 分钟，超时只终止本任务进程并留日志。

## 执行记录

- [x] 预设 serde 入口 RED→GREEN，旧默认和 Random 分布保留。
- [x] 独立标签的球面噪声初态接入材料构造器，面积/确定性单测通过。
- [x] Draft/42 完整链 ignored 探针通过；生产物质、水量、气候与终态身份校验保留。
- [x] 实际 UI 固定水量截图，以及用户进一步指定的目标陆比 25% 新旧预设与降水截图。
- [x] 面板整格陆比与连续几何不一致的回归 RED→GREEN，删除重复统计实现。
- [x] Release 全量 1,318 passed / 59 ignored；UI 修复后完整链探针与 app 103 项通过。
- [x] fmt/diff、默认和 Release Clippy、WASM check、Trunk Release 构建通过。
- [x] 独立审查无重要问题；两个非阻塞测试建议记录在规格 §9，不扩大本轮范围。

裁定：本轮采用一个完整、可操作的实验预设作为原子提交单元，包含为截图验收
发现的 UI 面积口径修复；保留原生成流程，不为当前视觉问题扩大重写范围。
风险是新形状仍受现有 P2 重采样影响，因此不能将 UI 原型称为最终质量通过。
用户尚未给出视觉验收；后续要分辨初态、重采样与地表过程分别造成的碎边。

结果和逐项证据见首轮规格 §9。实验应用留在用户指定的 25% 目标陆比场景，
本次所有探针、测试及基线应用均已退出。

## 续修：保留实验选择时的重建与启动保护（2026-10-31）

起点 `7e9a163`。仅修补实验预设的 UI/启动入口，不扩大档位支持，不改变气候、
地形或身份算法，也不增加产品开关。`coast-cell-boundary-fix` 计划中的历史
1,339 项通过证据仅属于 `c0463b9`，不是本次改动的全量回归结果。

- [x] 确认旧 `rebuild_blocked` 在编辑档位之前求值，会使同一帧按钮/提示陈旧。
  将两种重建操作放在作者参数之后，统一按本帧最终参数求值；新种子按钮现在
  与按当前参数重建按钮相邻。
- [x] 启动及显式重建共用请求入口，在创建线程、生成网格和搬走缓存之前拒绝
  不支持的已保存选择。菜单/按钮/请求共用 app 判定；档位边界抽取到
  `NaturalQualityProfile::supports_procedural_coast`，生产阶段也引用此事实源。
  LegacyFoundation 的 UI 排除仍保留，没有把它冒称为完整 Formation 链。
- [x] 最小回归：启动预检 RED（错误创建 worker）→GREEN；headless egui
  在真实 `TemplateApp::update` 内切换 Draft→Standard→Draft，断言选择发生
  的同一 pass 内两个按钮的可用状态。纯函数测试不能发现绘制顺序，因此这项
  使用无 GPU/无系统鼠标输入的控件测试；不生成额外 seed 世界。
- [x] Release app 消费者 109 项通过（8.08 秒测试执行，另有编译时间）；
  档位门禁只提取既有判定，没有改动共享求解器/schema/身份，故不扩全量。
- [x] 最终 fmt、Clippy、WASM check/Trunk Release、本机 exe 构建通过。
- [ ] 用户亲自验收；代理不接管鼠标，不以 headless 测试替代最终 UI 验收。

用户验证：运行本工作区 `target/release/sekai.exe`，在“世界形态”选
“程序化海岸（实验）”；从“质量档位”草稿切至标准/高，两个重建按钮应禁用
并出现完整生成链草稿限制提示；切回草稿或改选普通多大陆，按钮恢复可用。
保存实验预设加非草稿组合后重启，应立即提示拒绝、不启动昂贵生成；改回草稿
后可重新构建。气候响应、其他档位科学支持与湖岸遗留不属于本次修补。

### 本轮命令与结果

所有 Cargo 构建限制 `-j 2` / `CARGO_BUILD_JOBS=2`，无 RUSTFLAGS；开始及结束
核查无外部 cargo/rustc，本轮未启动实际窗口、未控制系统鼠标。

- `cargo test --release -j 2 --lib unsupported_procedural_startup_does_not_spawn_a_build
  -- --exact app::natural_app_tests::unsupported_procedural_startup_does_not_spawn_a_build`：
  修改请求入口前预期失败，证实不支持的启动会创建 worker。
- `cargo test --release -j 2 --lib app:: -- --test-threads=2`：首次 109 passed，
  8.08 秒执行。随后只新增 Draft 域的一个断言，生产代码/fixture 未变，却不必要
  地重复了整套；第二次在两个 GPU 初始化夹具停滞并触发 600 秒上限。
  没有获取等待堆栈，不能断言 GPU 是唯一根因；协调者指出已有并行 GPU 卡死
  先例，后续 GPU 测试必须 `--test-threads=1`。命令超时后 Win32_Process
  核查无 cargo/rustc/sekai 测试残留，不以加时/扩大语料解决。
- `cargo test --release -j 2 --lib a_kept_procedural_coast_selection
  -- --test-threads=1`：最终新增断言所在测试 1 passed。
  超时命令用 `&&` 链接的 `natural_quality_profiles` / `world_formation_spec`
  测试未执行；本轮不宣称全量通过。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets --all-features -j 2 -- -D warnings`：通过。
- `cargo check --target wasm32-unknown-unknown --all-features --lib -j 2`：通过。
- `CARGO_BUILD_JOBS=2 trunk build --release`：通过，更新 `dist/sekai_bg.wasm`。
- `cargo build --release --bin sekai -j 2`：通过，无编译告警。

精确本机路径：
`C:/Users/DYZ/orca/workspaces/sekai/coast-climate-spike/target/release/sekai.exe`。
SHA-256：`E095D76132571D6DC72B1F4448E1CF19DB1BC8851E5C1CD783D193C209F2BB57`。
本轮未启动 exe，运行时视觉结果与最终验收仍由用户确认；未改变既存湖岸限制。

## 每项承重技术的出处

- 先验初态及后续物理耦合：首轮规格 §6 的 CESM 与 Hergarten & Robl (2022)。
- 标量场超水平集塑形、多尺度噪声：Civ Atlas 的公开制图先例（只参考方法，
  不复制 AGPL 源码）；noise-rs 0.9 已有实现。频谱沿用本项目已有
  `FormationTectonicRecipe::initial_crust_profile`，不新钉系数。
- 面积、水量、地壳材料和校验：复用现有生产算子；初态噪声不是物理过程。
- 本次入口支持域：沿用本计划“判定与限制”的 Formation + Draft 授权，
  `CausalNaturalFormationStage` 与 app 引用同一领域判定，不引入新科学阈值。
- 同帧控件回归：egui/eframe 0.31 的 `Context::run`、`Frame::_new_kittest` 与
  AccessKit 输出；无 OS 输入及 GPU 初始化，验证真实 update 控件顺序。
