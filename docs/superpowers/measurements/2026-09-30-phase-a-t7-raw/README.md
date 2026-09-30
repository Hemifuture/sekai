# T7 原始证据索引与复现边界

结论见相邻 `2026-09-30-phase-a-t7-optimization.md`。产品候选已撤回；本目录只有日志、身份、脚本和未保留 patch，没有世界 bundle、缓存、网格或求解轨迹。所有耗时为真实 Windows/MSVC 本机值，不是模拟值；不跨批次归因加速。

- `before-identity.json`：BASE、实际工具链/环境、398 个 src/tests Rust 文件字节 SHA256、编译好的 BEFORE exe/rlib/libtests 身份。
- `after-identity.json`、`candidate.patch`：唯一实验产品差异、源码及实际 AFTER 二进制身份。
- `revert-identity.json`、`final-source-identity.json`：撤回单文件字节 SHA；398 个 Rust 文件逐字节恢复，文件集合相同，产品与 Cargo 配置 git diff 为空。
- `probe-before.log`：实现前真实 RHS 分配计数；`probe-pair-*.log` 与 `probe-summary.json`：五组交替、每侧七批，保留漂移批；`probe-c1-*`、`probe-dry-*`：最低成本直接消费者身份。
- `draft-before-initial.log`：原始未配对的三次冷样本，保留但不计入 A/B 收益。`draft/standard-pair-*.stdout/stderr.log` 和 `.receipt.json`：同启动方式、缓存、输入的真实成对观察，含 process wall/OS-reported CPU、环境、UTC、SHA；汇总见对应 JSON。
- `after-build.log`：原生 rustc 的 0xc0000409 退出原文；`after-build-retry-serial.log`：仅降低编译并发为 -j1 后成功。`probe-compile-before.log` 是首次独立探针缺少 Windows native link 搜索路径的链接错误，保留排查痕迹；最终成功链接命令如下，成功本身无 stdout，不把此失败日志当作成功证据。
- `tendency-tests.log`、`integrator-tests.log`、`publication-tests.log`：42+19+7 个实际 PASS；没有用零选中测试冒充成功。
- `final-clippy.log`：收尾clippy PASS（9m27s）；`final-wasm.log`只含中断前依赖输出，非PASS。`final-gates-handoff.json`记录仅停止自己的收尾session13780、Base未开始/跳过，以及同源码T6 wasm门禁沿用。`preserved-artifacts-final.json`核实仍保留的所有两侧compiled artifact SHA。

保留的真实编译产物位于 ignored `.superpowers/sdd/2026-09-30-phase-a-authoring-loop/t7-before/` 和 `t7-after/`；两侧 `generation_latency.exe`、`libsekai.rlib`、`libtests.exe`、`allocation-probe.exe` 均保留供本工作树审查。源码精确身份与 patch 才是复现依据；普通 target 下的实验测试 exe 不能自动当作撤回后的当前产品。

以下命令从工作树根目录执行，环境 `CARGO_INCREMENTAL=0`、Release opt-level=2/debuginfo=0、无 RUSTFLAGS、未设置 RAYON_NUM_THREADS。先确保本机当前 toolchain/依赖身份匹配 manifest；不得为了复现覆盖用户已有改动。

```powershell
$env:CARGO_INCREMENTAL='0'
$taskScratch='.superpowers/sdd/2026-09-30-phase-a-authoring-loop'
$taskRaw='docs/superpowers/measurements/2026-09-30-phase-a-t7-raw'
# $side 为 before 或 after；只链接已经保存、身份已核实的对应生产 rlib。
rustc "$taskRaw/allocation-probe.rs" --edition=2021 -C opt-level=2 -C debuginfo=0 -L dependency=target/release/deps -L native=C:/Users/DYZ/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/windows_x86_64_msvc-0.52.6/lib -L native=C:/Users/DYZ/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/windows_x86_64_msvc-0.53.1/lib --extern "sekai=$taskScratch/t7-$side/libsekai.rlib" --extern blake3=target/release/deps/libblake3-4d4074bdebdaaef3.rlib -o "$taskScratch/t7-$side/allocation-probe.exe"
& "$taskScratch/t7-$side/allocation-probe.exe"
& "$taskScratch/t7-$side/allocation-probe.exe" --c1 --verify-only
& "$taskScratch/t7-$side/allocation-probe.exe" --dry --verify-only
# 真实已执行顺序：Draft 1–3，按噪声补 4–5，再唯一 Standard pair。
& "$taskRaw/run-paired.ps1" -Profile draft -Pairs 3 -StartPair 1
& "$taskRaw/run-paired.ps1" -Profile draft -Pairs 2 -StartPair 4
& "$taskRaw/run-paired.ps1" -Profile standard -Pairs 1 -StartPair 1
```

上述 benchmark 脚本会覆盖同名 raw，复核时应先使用独立输出目录或备份原证据，保持重负载串行。它们不启动 UI，也不读写默认 autosave。RHS 探针的原子 allocator 计数开销限制其耗时解释；请求 bytes 不是 live/peak memory。原图预算仍为 Draft 10–20 秒、Standard ≤60 秒，本轮未达；真实 UI 和最终用户验收留 T8。
