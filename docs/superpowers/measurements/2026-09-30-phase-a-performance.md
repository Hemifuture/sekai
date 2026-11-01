# Phase A T6 性能观测 — 2026-09-30

状态：CPU E0、一次既有 stage probe 和一次 detached renderer 观测已完成；真实 UI 性能采样延后 T8，用户验收未完成。T6 不包含优化。下列数字均来自实际样本；中断运行不计入完成基线。

原始证据位置（2026-10-25 移出仓库）：原 `2026-09-30-phase-a-performance-raw/` 整目录按原相对结构移至仓外 `D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/`；下文链接与未加前缀的原始文件名（如 `old-draft-{1,2,3}.log`）均指该仓外目录。逐文件 Git blob 校验清单见 `D:/hemifuture-probes/phase-a-20261001/git-blob-index.txt`；Git 历史中 `7143d1e` 及之前的提交仍含原文件。

## 来源与环境

- 旧基线：`a716138ad619ebc2cd335b7a7fdaa6b79c6876ab`，通过 `git archive` 放入本任务 ignored scratch 的未修改源码。归档 SHA256 和二进制来源见[原始证据](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/old-binary.json)，未切换用户工作树。
- 当前构建：HEAD `f2ad407507668f9d02727c3b235920738940a8fc` 加 [source SHA256 manifest](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t6-source-manifest.json) 指定的六个 T6 源文件。构建发生在最终 T6 commit 前，明确为未提交源码构建，不声称干净 revision。[编译产物](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/current-binaries.json) 指定真实生产库与测试 binary。
- Intel Core i9-14900KF，24 核/32 逻辑处理器；NVIDIA GeForce RTX 4080 SUPER，驱动 `32.0.15.9186`；Windows 11 Pro `10.0.22631` x64；高性能电源计划。实际 boost 时钟和 GPU device 内存未测。WMI AdapterRAM 字段溢出，不作为显存容量依据。
- Rust `1.98.0 (88d9e12ae 2026-08-18)`、Cargo `1.98.0`、LLVM `22.1.8`、`x86_64-pc-windows-msvc`。[机器/环境原始记录](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t6-environment.json) 保留精确输出。
- 仓库 Release `opt-level=2`、`debuginfo=0`、`CARGO_INCREMENTAL=0`，不设新 RUSTFLAGS，不改产品 profile。`RAYON_NUM_THREADS` 未设置，默认 available parallelism 为 32。编译与计时串行，截图 app/probe 不与完成的性能样本重叠。其他机器负载/温度未控制，不能据此声称统计显著加速。
- Formation 输入：seed 42、半径 6,371,000 m，WorldFormation/Tectonic/Relief/Geologic domain defaults，Draft/Standard 质量档。没有改科学参数或质量阈值。CPU graph 样本无窗口/渲染。
- 不持久化世界或 solver 快照。cold 表示 fresh MemoryStageCache；hot 表示同输入成功构建后保留的内存 stage cache；不是跨启动/打开项目恢复，退出后不保存该 cache。hot stage-cache 样本仍重建 P1 surface。旧 cold 各为独立进程，当前 cold 为同进程依次重置 cache。旧测试没有 identity/hot 输出，不声称这两项具有旧/新对照。

## 计时边界

原预算测试保留原时钟与 Draft 20 s / Standard 60 s 断言。`observe_phase_a_latency` 复用 surface/external/graph/validation 路径，只报告成功/失败，不设性能断言。total 从 P1 前开始，到 engine 返回后结束。observer 在计时前重置 cache；旧预算在 graph 时钟内构造空 cache。不得把目标缩窄到更小操作。报告各 stage duration 是独立报告值，不推测开始时间。

app 观测只在既有 `SEKAI_FRAME_STATS=1` / Ctrl+Alt+F sampler 开启时运行。记录真实 OpenOrRequest、worker 等待、input/surface、graph、presentation、GPU 安装及第一次新世界绘制请求入队。Validation 无法独立观察，明确包含在相应操作内。worker 阶段为 WaitWorker 的子 span，不再次加到 root elapsed。终态区分 Published、Cancelled、Failed、Stale。取消记录请求、UI update feedback、实际观测 worker exit、slot release；这些是 app 事件，不是屏幕反馈实测。

打开日志区分完整 `read_project` 操作（read + decode + validation 包含）与 request→receipt、后续 build。open-origin build 从 IO 发起开始，可包含兼容确认的人等待和旧 worker 等待，并明确标签；普通 rebuild 保持 `build_request` origin。Save 是独立 runtime 日志，保留产品原 notice。GpuInstall 包含 detached renderer 准备与安装；FirstSubmittedFrame 仅代表实际 egui 新世界 draw request 入队，不代表 GPU 执行结束或 OS present。

frame p50/p95/p99/max 使用原始 update interval 与 nearest rank `ceil(n*p/100)`（inverted empirical CDF，Hyndman–Fan type 1；[方法定义](https://numpy.org/doc/stable/reference/generated/numpy.quantile.html)），继续标为 update 指标，不声称 display refresh/FPS。

## E0 已完成样本

| Revision/profile | Cache | Surface s | Graph s | Total s | 原预算 |
| --- | --- | ---: | ---: | ---: | --- |
| Old Draft 1 | cold，独立进程 | 0.70 | 60.02 | 60.72 | FAILED，20 s |
| Old Draft 2 | cold，独立进程 | 1.01 | 72.04 | 73.05 | FAILED，20 s |
| Old Draft 3 | cold，独立进程 | 1.35 | 66.36 | 67.72 | FAILED，20 s |
| Old Standard valid | cold，独立进程 | 2.30 | 127.01 | 129.30 | FAILED，60 s |
| Current Draft 1 | cold stage cache | 0.585158 | 49.755924 | 50.341082 | 超出 20 s |
| Current Draft 2 | cold stage cache | 0.653902 | 49.374511 | 50.028413 | 超出 20 s |
| Current Draft 3 | cold stage cache | 0.486939 | 42.253483 | 42.740422 | 超出 20 s |
| Current Draft hot | hot stage cache，P1 cold | 0.752182 | 0.227272 | 0.979455 | 不作 cold 预算对照 |
| Current Standard 1 | cold stage cache | 2.273466 | 114.739538 | 117.013005 | 超出 60 s |
| Current Standard hot | hot stage cache，P1 cold | 2.876712 | 0.863337 | 3.740049 | 不作 cold 预算对照 |

旧 Draft median 67.72 s，范围 60.72–73.05 s。三个完成构建均达到 5 climate cycles / 720 fast substeps 后触发未改的预算断言，退出 101。[run ledger](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/old-runs.jsonl) 和 `old-draft-{1,2,3}.log` 保存实际输出。此前对话中的 52–73 s 无原始日志，只作为历史上下文。

旧 Standard attempt 1 因用户新增真实窗口截图要求而停止，没有 endpoint latency，排除出完成基线。[中断 raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/old-standard-aborted-for-ui.log) 与[中断记录](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/old-standard-ui-interruption.json) 区分 runner wall 130.446 s 和停止时 process CPU 9.719 s。已完成 Draft 未重跑；唯一补测的有效 Standard 已完成，endpoint 为 4 cycles / 767 fast substeps，原 60 s 预算 FAILED，退出 101；[有效 raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/old-standard-valid.log) 和 [remaining ledger](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/remaining-runs.jsonl) 保留。

当前 Draft cold median 50.028413 s（范围 42.740422–50.341082），三个 cold 均未达 20 s 目标；hot 命中两个 stage，但 P1 仍冷建。四次 BuildResultHash、surface fingerprint 完全一致，cells=20252、endpoint=5 cycles/720 fast substeps。observer 1 PASS 仅表明成功/身份契约，不是预算通过；[current Draft raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/current-draft.log) 保留六位小数和所有 identity/stage 行。old median67.72 与 current50.03 是本次观测差异，不归因T6加速：没有数值优化，机器负载未控制、cold进程边界不同。当前 Standard cold 117.013005 s，仍未达 60 s；hot 3.740049 s，同输入 identity/fingerprint 一致，cells=79212，endpoint=4 cycles/767 fast substeps，cold 0hits/2miss、hot 2hits/0miss；[current Standard raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/current-standard.log) 保留实际值。两个 observer 各1PASS，只代表构建/identity校验成功。内部 stage probe 与 detached renderer 已按下文各运行一次。

## 复现与证据

先完成编译，再直接调用测试 executable，报告生成耗时排除编译。两版串行共用当前 worktree 的 own target；旧版为未改的归档源码，不注入旧 observer。

```powershell
$env:CARGO_INCREMENTAL='0'
# 在归档旧源码，CARGO_TARGET_DIR 指向 own worktree target：
cargo test --release --test generation_latency --no-run --message-format=json
# 在当前 worktree：
cargo test --release --lib --test generation_latency --no-run --message-format=json
# executable 从 compiler JSON 获取，无编译与运行重叠：
& $oldExecutable draft_full_chain_within_twenty_seconds --ignored --exact --nocapture --test-threads=1
& $oldExecutable standard_full_chain_within_one_minute --ignored --exact --nocapture --test-threads=1
$env:SEKAI_PHASE_A_PROFILE='draft'; $env:SEKAI_PHASE_A_RUNS='3'
& $currentExecutable observe_phase_a_latency --ignored --exact --nocapture --test-threads=1
$env:SEKAI_PHASE_A_PROFILE='standard'; $env:SEKAI_PHASE_A_RUNS='1'
& $currentExecutable observe_phase_a_latency --ignored --exact --nocapture --test-threads=1
Remove-Item Env:SEKAI_PHASE_A_PROFILE,Env:SEKAI_PHASE_A_RUNS
```

[旧编译日志](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/old-release-build.log)、[当前编译日志](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/current-release-build.log) 保存实际 profile flags/executable 路径。旧编译 2m25s、当前编译 59.53s 是编译观测，不作为生成延迟。controller 的精确 own-cache 回收、Release 准入 receipts 和[空间监测](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/space-monitor.jsonl) 均保留。operation reserve 1 GB 是工程空间约束，不是科学/性能阈值，也不保证精确峰值空间。

## 检查与真实 UI

metrics 3 个测试与 frame intervals 9 个测试从实际 RED 转绿。新增 IO 2 PASS，app consumer 2 PASS / 1 ignored，publication contract 7 PASS。project_ui regression 17 PASS / 1 sandbox failure：失败为已知 ReplaceFileW sandbox access denied；同一个精确测试在授权普通 native process 下 1 PASS。[sandbox error](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t6-overwrite-sandbox.log) 和[普通环境 run](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t6-overwrite-normal.log) 保留；未改 IO 算法迎合该环境。clippy 全 targets/features warnings denied、fmt、wasm lib check 均 exit 0，rawlogs 在证据目录。没有将 0 selected 当 PASS。

真实 native 截图已取得，并与性能计时隔离。task-local scratch launcher 复用生产 `TemplateApp::new(cc)`，现有 eframe `NativeOptions.persistence_path` 指向 task 内独立 RON 文件（本地代码已核实是 file path）。不启动默认 main 写用户 autosave。launcher 仅隔离 storage/window，使用既有 default Draft 输入。离屏 GPU consumer 测试与实际窗口证据分开。T8 用户验收 pending。

[截图索引](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/screenshots-index.json) 保留 UTC、源身份/生产库 SHA、定向 PrintWindow method、图像 SHA、实际 client 1280×900。源索引保留原 scratch 绝对路径；同名证据副本在该目录可恢复。四张图分别是[生成中](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/t5-generating-printwindow.png)、[文件菜单](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/t5-file-menu.png)、[真实二维发布](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/t5-published.png)、[默认新项目配方保存](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/t5-draft-saved.png)。生成中显示 11 s 只代表截图瞬间进度，不能作 total。实际保存的 JSON 是独立 scratch 的纯配方，世界尚未发布；不写用户已有项目。adapter 未记录，不猜测。

本轮真实 UI 未测 open/reopen、生成取消、旋转/缩放/字段切换、3D、长帧分位采样；截图不能代替这些操作或用户验收。发布图初始板块数控件外观可见「1」，生成前为「12」；证据不足以解释数值/裁切原因，记录为真实 UI 显示待 T8 核验，不改科学输入、不猜正常。helper 仅关闭自己的 PID 58168，退出 UTC 11:30:37.8159723Z，恢复 E0 后没有 UI/捕获进程并行。


四图已上传 Library，真实 captions/file IDs/version=0 与 receipt 见 [Library delivery](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/library-delivery.md)、[receipt](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/library-upload-receipt.json)。本次窗口未开启 FRAME_STATS，没有取得 native frame quantiles，不能算 UI 性能通过。T6 的真实 open/rebuild/cancel/rotate-field/2D/3D 性能采样诚实延后到 T8：复用同一 task 内隔离 launcher，开启既有 sampler，记录端到端及 update 分位、取消反馈、发布长帧，同时完成用户验收；不能以本轮截图替代，也不在 T6 为此重复全链。



## E0 后的定位：既有 Draft stage probe

causal stage 在 current Draft 首 cold 占 49.117225/50.341082 s，Standard cold 占 112.540042/117.013005 s，因此按计划只运行一次既有 `stage_timing_probe`（Draft，seed42，同 Release test binary），无生产数值/观测位置改动、无新 dump（SEKAI_PROBE_TAG 未设置）。[raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/current-draft-stage-probe.log) / [run receipt](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/stage-probe-run.json)：1 PASS，probe TOTAL=46.976 s。

| 既有 probe 操作 | 秒 |
| --- | ---: |
| P1 bundle | 0.540 |
| climate domain | 0.356 |
| P2 tectonics | 0.695 |
| P3 substrate | 0.437 |
| P3 relief | 1.559 |
| P4 start forcing | 0.306 |
| P4 start solve | 19.793 |
| P5 + endpoint P4 | 23.290 |

两个包含 climate solve 的段共 43.083 s，约占该 probe TOTAL 91.7%；这只定位当前成本，不证明某个算法优化可行。P5 accepted surface substeps=1，start/endpoint climate 均 5 cycles/60 steps/720 fast substeps；final residual 分别 .2140/.2138。这个既有手动生产操作 probe 总口径不是原 BuildEngine 完整预算，不能替代 E0 total 或当达标证据。没有启动 T7/修改求解器。

```powershell
$env:CARGO_INCREMENTAL='0'; $env:SEKAI_PROBE_PROFILE='draft'
Remove-Item Env:SEKAI_PROBE_TAG -ErrorAction SilentlyContinue
& $currentLibTestExecutable stage_timing_probe --ignored --nocapture --test-threads=1
Remove-Item Env:SEKAI_PROBE_PROFILE
```


## T2 detached renderer：实际耗时与进程内存

串行运行一次 ignored `app::build_metrics_app_tests::observe_phase_a_detached_renderer`，真实 Formation Draft/seed42，初始发布后同输入 hot replacement，使用实际 `prepare_world_publication` 新 renderer 并保留旧已发布世界。该 fixture 的 inactive Legacy `target_cell_count=162` 不控制 Formation surface，Formation surface 由 Draft profile 决定；不把它当 162-cell Formation。adapter 实测 `NVIDIA GeForce RTX 4080 SUPER`，target format `Rgba8UnormSrgb`。这是离屏 native test，不是窗口呈现。[raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/current-draft-detached.log) / [run receipt](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/detached-run.json)：1 PASS，整个 fixture 56.97 s（含第一次冷构建），真实 replacement preparation=0.063057 s。

| 原生进程时点 | Working set bytes | Peak working set bytes |
| --- | ---: | ---: |
| replacement 准备前 | 558,231,552 | 669,110,272 |
| 旧世界与 detached 准备资源同时活着 | 600,629,248 | 669,110,272 |
| commit 后 | 460,398,592 | 669,110,272 |

瞬时 working set 差为 +42,397,696 bytes，但 OS working-set 变化不能当精确资源分配量；peak 是整个进程 high-water，不是本阶段增量峰值。GPU device memory 未测。准备包含真实验证/构造，不能据此假定所有 seed/profile/driver 上同样便宜；Standard renderer 未扩跑。

```powershell
$env:CARGO_INCREMENTAL='0'; $env:SEKAI_PHASE_A_PROFILE='draft'
& $currentLibTestExecutable app::build_metrics_app_tests::observe_phase_a_detached_renderer --ignored --exact --nocapture --test-threads=1
Remove-Item Env:SEKAI_PHASE_A_PROFILE
```

## 最终状态与限制

完成 E0 的 old cold4样本、current cold4+hot2样本，current observers 2 PASS；已有 stage probe1PASS、新 detached observation1PASS。old 原预算4 FAILED（Draft3/Standard1），current cold 从观测值看同样未达20/60s目标；未将 observer成功伪装性能通过。唯一 aborted Standard单独保留。不增加新阈值，不降低质量，不提前实施T7。

当前无 active build/benchmark/app，最后读取 C free=13,931,524,096 bytes（瞬态值，非保证）。source manifest 七项含未修改 Cargo.toml 全匹配，E0/截图复用相同生产源，不为截图变更科学参数。fmt/clippy/wasm证据对应这些源；之后只改 measurement/raw/report 文档，未改源代码。

T8须补真实UI sampler操作与用户验收，并核验发布图板块数控件显示。未测操作与GPU显存均明确列出；这些限制不阻止把T6观测代码/CPU证据交独立review，但不把UI性能或最终用户验收标绿。

## T8：真实 Formation 配方重开与原预算（2026-09-30）

源码checkpoint `49e20840db736a233876e1028dedf2cc5b0c70b6`，BASE `12007ebc65f12967f50847ac85d9b0071f366bdf`。仅新增私有native cfg(test)整链/草稿保存测试；没有生产、科学或性能策略修补，不改T1/20s/60s门槛和ignore。所有昂贵测试串行，CARGO_INCREMENTAL=0、无RUSTFLAGS/profile修改。构建前C free=5,412,282,368bytes，热own-release额外峰值规划1GB/操作保留1GB；交接时free=4,687,478,784bytes。规划不是精确测得峰值。[空间receipt](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-space-admission.json)与[消费者准入](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-integration-space.json)保留。

新增 ignored `app::phase_a_tests::phase_a_recipe_reopen_formation` 的首次完整运行GREEN，1PASS/0FAILED/0ignored/618filtered，224.34s，exit0；编译48.77s不计入test时间。唯一Draft/seed42从生产worker生成/GPU发布，编辑事务切TargetLandFraction并设0.4，显式重建/保存；销毁旧app、所有stage/surface/display缓存及旧renderer后，新app通过真实Open/IO与App::update自动一次现算。编辑后recipe、generator身份和全部SphericalPresentationSource（RootSeed、SurfaceRef、BuildResultHash、graph contract）匹配；新token epoch1/request1/revision0，历史不跨会话。生产Formation document构造已经执行bundle及跨层校验。三次构建是初始/编辑后/冷重开对照的最小链，224.34s不是单次生成预算、原生屏幕延迟或优化收益。[原始整链](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-reopen-first-complete.log)保留确切bytes/指纹。首次编译在test未开始前纠正inactive目标陆比fixture而中断，exit1，[中断记录](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-reopen-first.log)不计产品RED，未造失败。

应用回归191PASS/0FAILED/2ignored/426filtered（9.39s、exit0），其中新增小型未生成草稿保存不跑全链；普通native环境运行避免已验证ReplaceFileW sandbox OS5。[app raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-app-regression.log)。直接消费者build_cancellation7PASS、spherical_presentation_gpu5PASS、spherical_presentation_integration31PASS，全部0fail/ignored/filtered，0.02/0.65/6.62s，exit0；[raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-consumers.log)。无共享身份/schema/求解器改动，不扩Release全量。fmt实际exit0，[raw](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-fmt.log)；wasm复用不变生产源码的T6已通过证据；新增cfg(test)已由本次计划all-targets/all-features/-D warnings Clippy实际覆盖，26.54s、exit0。

原预算分别显式运行，不用observer代替、不调整门槛：

| 原预算测试 | P1 surface s | graph s | full total s | 目标 s | endpoint | 实际状态 |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| Draft/seed42 | 0.61 | 58.23 | 58.83 | 20 | 5cycles/720fastsubsteps | 1FAILED、exit101 |
| Standard/seed42 | 2.89 | 88.35 | 91.25 | 60 | 4cycles/767fastsubsteps | 1FAILED、exit101 |

[Draft原始失败](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-draft-budget.log)和[Standard原始失败](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-standard-budget.log)各为一个冷构建样本，test时间58.84/91.26s；没有成对负载控制，不能与T6/T7数字归因比较，也不能宣称优化收益。未重跑已知T1 ignored红门0.0139>0.01，不标绿；整体地理显示质量仍有阻碍，性能目标未达。

必要消费者编译1m00s成功刷新生产 `target/release/deps/libsekai-7716a1b8ffc33b4e.rlib`，74,931,004bytes，SHA256 `A3F6D1F3CCD4B48070B8AB5A085F76BD86E49125D8D945A438B7CEA819844299`；明确排除T7残留实验库和另一个旧10cb库。[生产库receipt](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-production-library.json)保留实际编译时BASE+source状态及后续相同源码checkpoint；[source manifest](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-source-manifest.json)含398个src/tests文件及Cargo.toml。root在BENCH-IDLE后安排已有隔离native helper，代理实际3D/open-auto-once/cancellation/生产UI采样证据已经完成，详见 [UI原始报告](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/t8/ui-report.md) 与 [11PNG索引](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/native-ui/t8/screenshots-index.json)；它们不替代用户本人验收。


T8最终状态：新增tests必要Clippy实际exit0，原始 [lint log](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-clippy.log)/[receipt](D:/hemifuture-probes/phase-a-20261001/2026-09-30-phase-a-performance-raw/t8/t8-clippy-receipt.json)。仅一次热metadata检查，CARGO_INCREMENTAL=0，无RUSTFLAGS；启动free1,508,237,312bytes、结束1,490,661,376bytes，删除0。此前SPACE暂停是历史情况，未知恢复原因不归因清理；源码仍49e2084。代理UI使用同一freshA3F6库、inactive native1280×900/RTX4080SUPER/Vulkan，无周期wrapper重绘；PID69756已于13:54:56.1452172Z正常退出。instrumented Open读请求0.007852s/worker0.0070838s，pre-worker0.024922s，Published69.724806s（Graph66.682725/GpuInstall0.060270）；FirstSubmittedFrame起点69.724761s、自身0.000044s，仅应用绘制入队。热构建取消反馈0.0016165s、观察worker退出0.999387s、槽释放0.999937s；显式热重建Published2.163288s。三个实际10s窗口p50/p95/p99/max（ms）为1.375/21.209/21.507/28.094、2.817/24.510/26.316/65.799、1.806/21.251/24.994/100.690；最后窗口仅覆盖热发布，非最终冷Open/stock idle/OS-present测量。首Open的observer关闭，之后只补一次instrumented Open；未验精确候选瞬间取消、异目录/新进程Open、不可兼容升级及完整河线/拾取/接缝/LOD，本人UI仍pending。3D/Open/Cancel三张Library真实succeeded/version0及身份见原始receipt；本地Windows xattrs未成功，不混为Library成功。原预算失败/T1红门均保留，DONE_WITH_CONCERNS不等于性能或整体显示质量已交付。
