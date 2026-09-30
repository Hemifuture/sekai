# Phase A T7 单项工程优化证据

## E1 候选记录（实现前）

- BASE：`8f71bd9ccd9bab891ec180ba6be3f70ec1436535`，唯一产品文件 `src/generators/natural/formation/global_circulation/tendency.rs`，函数 `LayeredTendencySystem::apply_pair_momentum_exchanges`。不改接口或调用者。
- 来源：T6 stage probe 的 P4 start solve 19.793 s、P5+endpoint P4 23.290 s，总计占 46.976 s 的 91.7%；detached renderer preparation 0.063057 s，不足以优先于实测生成热路径。具体调查了生产 split-explicit 的 generation/fast tendency、RK derivative 分配、strain workspace、gradients、work-domain 重建；现有 transient temperature-gradient 优化和 A6 已撤回并行均排除。
- 唯一假设：C2 lower-air/mixed-ocean surface exchange 每个湿单元的 `mass_per_area(state, UpperAtmosphere)` 都构造同一 `ClimateLayerLayout::for_profile`（两个 Vec），可在该 exchange 的 cell 循环外计算一次。该 helper 只依赖 immutable profile、布局 density 和 state reference thickness；不依赖 cell、不读 height anomaly。逐单元 `momentum_mass_per_area` 的实际深度验证不动。
- 改前实际分配探针：未改生产 rlib，C2 face resolution 24（3456 cells）、部分湿 land fraction .25、非零纬向速度；workspace 预热后每批 30 次真实 `evaluate_fast_with_workspace`，7 批均 208680 请求分配 / 74716800 requested bytes。7 批 elapsed 秒为 .217414900/.224896900/.225662500/.239904300/.218734800/.212965900/.190405100。观察时输出相等断言通过；Debug hash `cc8dbf66b9be5379a6771e47f50564020c1b53e4824418a3432b6b1d9fc2ed33`。计数包括整个公开 RHS，不猜整个求解器精确占比。raw `2026-09-30-phase-a-t7-raw/probe-before.log`。
- RED：此工程布局/分配修改没有新独立行为；依 T7 指令用失败的配对性能假设作红证据，不造 wall-clock 单元阈值。假设期望每次 RHS 移除 `2 * (3456 - 1)` 次循环内布局分配；当前未优化测得 6956 次/eval，显然未实现循环外单次构造。绿证据由同一个独立 probe 的实际分配差和逐位输出 hash 给出。
- 最小正确性命令：`CARGO_INCREMENTAL=0 cargo test --release --lib generators::natural::formation::global_circulation::tendency::tests:: -- --test-threads=1`；已有生产 surface-stress、实际厚度、部分湿/全陆/层配置契约直接消费该函数，先核实具体测试名/数量，不以 0 selected 算通过。布局修改不新增生产 helper/公开 API 或测试专用抽象。
- 输出要求：同审计平台的完整 Draft/Standard graph result hash 与 surface fingerprint 逐位相等；探针 hash 逐位相等；科学硬门、层实际质量、取消检查点、运算顺序和所有 f32/f64 cast 保持。
- E2：保留 BASE compiled BEFORE graph observer exe/rlib/libtests 与 source SHA manifest 于 ignored scratch；同机同输入、Release opt2/debuginfo0、CARGO_INCREMENTAL0、无 RUSTFLAGS，serial BEFORE/AFTER，T6 `observe_phase_a_latency` 冷 stage cache/P1 cold 条件。先 Draft3，Standard1；接近噪声补相关 alternating 对，不挑最快值。编译不计入生成。
- 风险：C1 无 upper role、完全干单元原先跳过 surface branch；因此 upper reference 只在既有 C2 surface-exchange 条件成立时构造，不对 C1查 upper。实际厚度仍逐cell校验，硬失败传播不改。取消周期仍256cells及末尾；有限一次纯布局构造不会引入全图计算。
- 撤回范围：只撤回上述函数内本轮 hoist；保留测量证据与原始命令，不触碰既有用户工作、其他优化或科学金样。若收益不能重复或输出变，撤回。
- 工程依据：既有 immutable borrowing、循环不变量外提与 Vec allocation elimination；没有新科学技术/常量/阈值。布局与密度的 SSOT 仍是生产 `ClimateLayerLayout`，不复制物理公式。

## 局部 E2：真实分配与交替 RHS 配对

同一 probe source 分别链接保留的 BEFORE/AFTER 生产 rlib，Release opt2/debuginfo0；独立单线程 System allocator 转发计数，workspace 预热。没有持久世界；Debug 文本只进入内存 hash，磁盘只写计数、耗时与 hash。该 probe 有原子计数开销，因此局部耗时不能外推产品全链收益。

每 30 次 RHS：分配请求 `208680 → 1380`（精确少 `207300`，每 RHS `6956 → 46`）；requested bytes `74716800 → 34915200`（精确少 `39801600`，约 53.27%）。五组的每一批均一致；这不是 peak/RSS/GPU device memory。每组完整保留 7 批，顺序交替 A→B、B→A，没有挑最快值。

| 组 | BEFORE median s | AFTER median s | median 差 |
| --- | ---: | ---: | ---: |
| 1 | .1369613 | .1275514 | -6.87% |
| 2 | .1358595 | .1277173 | -5.99% |
| 3 | .1357902 | .1284538 | -5.40% |
| 4 | .1373655 | .1432492 | +4.28% |
| 5 | .1403810 | .1273238 | -9.30% |

组 4 的 AFTER .1258342–.1602899 s 明显漂移，不能隐去；raw 全范围见 `probe-summary.json` 及 `probe-pair-*-*.log`。局部分配目标 GREEN；完整生成是否收益超噪声仍以交替全链样本判断。

## E3：已完成的最低成本正确性证据

- `t7-after/libtests.exe generators::natural::formation::global_circulation::tendency::tests:: --nocapture --test-threads=1`：42 PASS，1 ignored，0.01 s。实际 Release binary 与 E1 的 cargo filter 相同；先 `cargo test --release -j1 --lib --test generation_latency --test global_circulation_integrators --no-run` 确认生产源码编译。
- `global_circulation_integrators-041ea9701218bb85.exe --test-threads=1`：19 PASS，.46 s，包含全 C2 年质量、确定性、预取消、scalar/moisture/热量硬契约。
- `t7-after/libtests.exe app::phase_a_tests::publication_ --test-threads=1`：7 PASS，1.52 s，覆盖取消后所有消费者保留、GPU prepare 失败、epoch/stale 拒绝、worker exit 后释放槽、发布世界/河线/mesh/packet/report 同步。
- probe 的三个直接真实消费者逐位 Debug 输出 hash：部分湿 C2 `cc8dbf66b9be5379a6771e47f50564020c1b53e4824418a3432b6b1d9fc2ed33`、部分湿 C1 `f7f3ce8c25912b7accefd54474e891af30b5cb6cdc2b6c183e121cdced3f240f`、全干 C2 `d6c973bce83235db1f3b860279792f80faff0bfaa242a935ef203c3009a8890d`，BEFORE/AFTER 全相同。C1 不读 inactive upper；全干不进入逐cell surface stress；部分湿 fixture 覆盖被改分支，既有机械测试另覆盖非零 upper/lower/ocean height anomaly。
- 完整输出 hash 链已逐源核实：`scheduler.rs` 的 `graph.output_hashes` 和 `result_hash` 聚合 graph stage 的 `ArtifactKey + ContentHash`；`artifact.rs` 的 `StoredArtifact::new_cancellable` 先验证，再 `stream_hash_cancellable`，由 `serde_json::to_writer` 把整个 `Serialize` payload 喂给 BLAKE3；`graph.rs` 的 `NaturalFormationBundleArtifact` 只有真实 `bundle`。故 observer 的 BuildResultHash 绑定最终 climate/surface/product 内容，不是 input cache key。无科学金样刷新。
- 仅内部循环不变量外提，输出和接口不变，无可见 UI 改动，不制作 before/after 图。实际窗口取消反馈、长帧分位、2D/3D 操作和最终用户验收依 T6/T7 handoff 仍 pending T8；不能把单元测试或现有截图算真实 UI 性能 PASS。
- 编译首尝试原生 rustc 退出 `0xc0000409 STATUS_STACK_BUFFER_OVERRUN`，无 Rust diagnostic；`after-build.log` 原样保留。仅改变 compile concurrency 为 `-j1`，同源码同配置重试成功 2m06s，不推断硬件/驱动/代码病因，不改系统或 codegen。

## 全链 E2：真实同机配对

输入仍为 T6 的 seed 42、半径 6371000 m、domain defaults，原 production graph、原 profile、原时钟范围与校验。机器/工具链沿 T6 raw environment；本轮 `before-identity.json` 另记录实际 rustc/cargo、环境、UTC、所有源 SHA256 和 BEFORE 二进制 SHA256。AFTER 只含候选 patch；`after-identity.json` 保留源 SHA 与真实 binary SHA。所有生成串行，无 app/GPU screenshot/编译/probe 重叠，无新 RUSTFLAGS，RAYON_NUM_THREADS 未设。

实现前的 BEFORE 单进程 cold3（79.939270/82.937103/91.787541 s，hot 1.543425 s）显示明显机器/启动批次漂移；它们未被删除，也不与下一批最快结果配成收益。随后 A/B 都用同一 `Start-Process` hidden 方式运行保存的 exe，每个 fresh process `SEKAI_PHASE_A_RUNS=1`：先 fresh MemoryStageCache/P1 cold，再该进程同输入内存 stage-cache hot。循环次数从 T6 默认3改为1只是为了交替、固定两侧同一进程/cache条件；未缩窄原 total，也不把后续阶段缓存热样本当 cold。每次 receipt 记录完整 args、UTC、PID、binary SHA、process wall 与 OS-reported process CPU，内部 total 与启动/热样本包含的 process wall 分开。CPU 计数只作 OS 原始值，不推断温度/频率或根因。

前三 Draft pair 结果混合，按 E2 补且只补两对相关样本；五对顺序 A→B、B→A、A→B、B→A、A→B。

| Draft pair | BEFORE cold total s | AFTER cold total s | AFTER 相对差 |
| --- | ---: | ---: | ---: |
| 1 | 38.138318 | 40.808206 | +7.00% |
| 2 | 37.803546 | 36.069798 | -4.59% |
| 3 | 37.107312 | 36.188658 | -2.48% |
| 4 | 36.825272 | 42.317458 | +14.91% |
| 5 | 36.767818 | 35.440654 | -3.61% |

BEFORE median 37.107312 s、mean 37.3284532 s、范围 36.767818–38.138318；AFTER median 36.188658 s、mean 38.1649548 s、范围 35.440654–42.317458。median -2.48%，mean +2.24%，3 快/2 慢；不足以证明收益可重复并超过观测噪声。所有 cold 都 0 cache hit/2 miss，hot 都 2 hit/0 miss；Draft 20252 cells、5 endpoint cycles/720 fast substeps，全内容身份和 surface fingerprint 全相等。

唯一 Standard pair：BEFORE 91.339940 s → AFTER 88.411641 s（单次 -3.21%）；surface 1.876352/1.907396 s，graph 89.463588/86.504244 s。hot total 2.564585/2.577510 s；79212 cells、4 endpoint cycles/767 fast substeps，BEFORE/AFTER/cold/hot 全内容身份和 surface fingerprint 相等。单次 Standard 结果不足以覆盖 Draft 的混合结果；候选已未满足保留条件，故不扩大 Standard 样本来挑成功证据。

全部 observer invocation 成功（初始1、Draft配对10、Standard配对2，共13次）；这是成功构建/校验/身份，不是原预算 PASS。Draft 各冷样本仍超过20s，Standard仍超过60s。T6 原基线50/117s→本轮某批37/91s的差包含批次/启动/机器噪声，**不计作优化**。T6 最后基线也不被本轮未保留候选替换。

内容 hash 逐位相等的 raw 值可复核于每份 stderr；Draft `BuildResultHash` bytes 固定 `[93,57,197,6,90,38,50,67,172,52,139,2,188,43,194,188,12,56,17,20,193,84,197,25,229,159,164,243,175,53,159,12]`，Standard 固定 `[76,63,218,143,171,74,18,101,74,221,223,40,179,186,227,38,165,43,211,70,173,89,137,158,215,222,9,59,86,51,166,163]`。这些是同 Windows/MSVC 审计环境实际有限字段序列化输出，不作跨平台逐位恢复承诺。

## E4：撤回，保留证据

- 结论：只撤回候选 hoist。局部分配下降可复核且 deterministic；完整生成延迟没有证明稳定收益，不能将局部计数或某个最快样本冒充产品加速。原科学参数、CFL/FAS/cycles、profile、质量门槛、T1候选和 A6 决定均未改变。
- `candidate.patch` 保存受控的8行新增/4行删除；BEFORE/AFTER compiled exe/rlib 留在 ignored scratch，SHA 和源身份均有 raw manifest。撤回后产品 `git diff` 为空，唯一编辑文件的字节 SHA 恢复为 BEFORE `60A2990E8961B54D5A998480758CD9F2D99B945220884EB2B4445B4276D964DB`（`revert-identity.json`）。未撤用户原工作、没有持久世界/网格/轨迹。源文件回到原始 CRLF 后先在内存确认完整 SHA 再写入。
- T7 本轮以证据-only 本地提交结束，不把失败候选代码提交。Draft10–20/Standard60目标未达；本轮没有留存加速或完成全部UI验收的声明。
- 后续边界：检查过 work-domain 重建、RK derivative copies、strain reusable workspace、fast gradient/pressure 路径；当前没有另一条达到 E1 的生产独占耗时/调用次数/受控效益证据，不能仅凭91.7%的求解聚合占比猜下一 winner。相关源码确有缓冲生命期与重复派生的调查线索，但取舍涉及额外保存/复制与完整 RHS 成本，尚不能选定必要下一候选。需要生产 RHS 的更细 CPU 采样或算子 exclusive timing 与实际调用计数，将成本绑定真实 Draft/Standard 工作负载；不需要改变方程/步长、拆 graph、保存 solver 状态或增加公开 API。本轮不无界循环，也不宣称所有安全机会已排除。进一步数值策略必须独立设计裁定。
- 本轮实测直接消费者均绿、全链科学/输出身份未退；真实UI帧分位、取消到反馈/worker退出/释放、2D/3D用户操作仍按 handoff pending T8。用户可按原 Phase A 流程编辑配方→显式重建，预期地图/图例/摘要与原版一致；本轮没有用户可见新行为。

最终 `final-source-identity.json` 核对全部398个 src/tests Rust文件字节SHA与BEFORE/BASE一致，集合一致，产品/Cargo配置git diff为空；故沿用同源码T6科学与wasm gates。已启动的收尾wrapper中fmt、clippy通过（9m27s）；随后按controller避免无关重复编译的指令，仅Ctrl-C结束自己的session13780，wasm还在cold dependencies阶段，明确为INTERRUPTED而非PASS，Base release未开始/跳过。停止后无active cargo/rustc/benchmark。`final-gates-handoff.json`记录target状态：top-level release rlib不存在，deps的7716 rlib仍为实验AFTER，现target不能用于当前UI；T8必需的新测试/回归编译时一次重建current。BEFORE/AFTER compiled artifacts仍保留，最终SHA见`preserved-artifacts-final.json`。任务提交SHA在scratch `task-7-report.md`登记；raw目录可复核全部数据、脚本与未保留patch。
