# Spherical Circulation Solver Comparison Results

## Environment and commands

测量日期为 2026-08-03，代码提交为 `9b9369b`。参考机器：Windows 11 Pro 10.0.22631、Intel Core i9-14900KF（24 核、32 逻辑处理器）、31.8 GiB RAM；Rust 1.97.1、Cargo 1.97.1、LLVM 22.1.6、Trunk 0.21.14。求解器本身为单线程 CPU 路径，Release 配置使用仓库既有 `opt-level = 2`。

已通过的主要命令：

```text
cargo test --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-features --lib --target wasm32-unknown-unknown
trunk build
cargo test --release --test circulation_transient -- --nocapture
cargo test --release --test circulation_steady balanced_solver_converges_for_all_fixtures_at_all_report_resolutions -- --nocapture
cargo run --release --bin circulation_compare -- --resolutions 12 --samples 1 --json target/circulation-comparison.json
```

正式 JSON 使用 schema 1，包含 2 次不计时预热和 1 次正式样本。性能数字因此是单样本，不冒充九样本中位数。此前还运行了 `n=4`、`n=8` 同协议试测，用于验证稳定性和缩放；当前提交的可审计原始报告是 `target/circulation-comparison.json`。

## Geometry and conservation validation

Cubed-sphere 网格、跨面邻接、面积闭合、常量零梯度、共享边成对通量、Coriolis 切向性、海岸零法向通量和确定性均通过解析或集成测试。三类 `n=8` 瞬态冷/热启动均达到年周期；稳态在 `n=12/24/32 × 3` 全部组合收敛。全目标 Debug 门共 192 个库测试并覆盖所有集成与二进制目标，无新增失败；既有超大 Voronoi 测试仍按原状态忽略。

下表保留提交 `9b9369b` 在 `n=12` 记录的旧版相对诊断值。旧实现中稳态只记录水汽输送诊断，瞬态只记录大气/海洋散度闭合，因此这些历史数字不能解释为统一的“质量误差”，也不能在两种求解器之间直接比较：

| Fixture | legacy relative diagnostic | steady final residual | transient cold residual | transient warm residual |
|---|---:|---:|---:|---:|
| Aqua Planet | 7.55e-9 | 9.79e-5 | 5.11e-6 | 5.82e-6 |
| Two Basins | 2.14e-8 | 9.74e-5 | 9.31e-5 | 5.26e-6 |
| Earth-like Harmonics | 1.60e-8 | 9.43e-5 | 8.33e-5 | 9.47e-5 |

物理修正后的 `relative_mass_error` 统一定义为三项数值闭合残差的最大值：大气体积通量、海洋体积通量、以及用同一边质量通量成对推进的层加权水汽。表面交换、凝结和湿度边界投影是显式预算项，不计作通量闭合误差。上表没有按新定义重算，后续引用 `n=12` 数字前必须重新运行比较协议。

实现过程中实际捕获并修复了三个未收敛案例：`n=12 Two Basins`、`n=24 Two Basins` 的大气 GMRES，以及 `n=32 Earth-like` 第 12 月的稳态外层迭代。修复只提高预算和重启空间，未放宽 `10⁻⁶` 线性容差或 `10⁻⁴` 状态容差。

## n=12, n=24, and n=32 performance

`n=12` 实测（864 单元，毫秒）：

| Fixture | grid | forcing | steady | transient cold | transient warm | validation | comparison | exact output bytes/snapshot |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Aqua Planet | 1.542 | 1.121 | 360.429 | 50,915.045 | 51,082.560 | 0.154 | 0.645 | 497,664 |
| Two Basins | 1.537 | 1.160 | 818.638 | 39,728.086 | 66,336.998 | 0.155 | 0.656 | 497,664 |
| Earth-like Harmonics | 1.570 | 1.167 | 868.348 | 39,922.602 | 39,987.089 | 0.169 | 0.674 | 497,664 |

快照中报告的基础 dense-state 计数为稳态 38,016 字节、瞬态 228,096 字节；上表的 497,664 字节按八个实际输出切片长度与元素大小精确求和。两者都不等同于进程 RSS；GMRES 临时基和 allocator 元数据不包含在基础 dense-state 计数中。

`n=24/32` 的稳态收敛是实测：九个组合的 Release 回归门合计 133.32 秒。没有执行高分辨率瞬态九样本长测。下表是以 `n=12` 实测和显式有限体积算法的 `O(n³)`（单元数 `O(n²)`、CFL 步数 `O(n)`）得到的外推，单位秒，不能当作实测：

| n | Fixture | projected cold | projected warm | projected 2-warmup + 9-sample case hours |
|---:|---|---:|---:|---:|
| 24 | Aqua Planet | 407.3 | 408.7 | 2.49 |
| 24 | Two Basins | 317.8 | 530.7 | 2.59 |
| 24 | Earth-like Harmonics | 319.4 | 319.9 | 1.95 |
| 32 | Aqua Planet | 965.5 | 968.7 | 5.91 |
| 32 | Two Basins | 753.4 | 1,257.9 | 6.15 |
| 32 | Earth-like Harmonics | 757.1 | 758.3 | 4.63 |

完整原协议外推为：`n=12` 0.88 小时、`n=24` 7.03 小时、`n=32` 16.69 小时，合计约 24.6 小时。由于 `n=12` 已全部大幅失败一致性门槛，继续消耗这段时间不会改变当前生产资格结论。

## Cold start versus steady warm start

| Fixture | cold time ms | cold steps / years | warm time ms | warm steps / years | warm-start outcome |
|---|---:|---:|---:|---:|---|
| Aqua Planet | 50,915.045 | 27,600 / 5 | 51,082.560 | 27,600 / 5 | 无收益 |
| Two Basins | 39,728.086 | 22,080 / 4 | 66,336.998 | 27,600 / 5 | 更慢且多一年 |
| Earth-like Harmonics | 39,922.602 | 22,080 / 4 | 39,987.089 | 22,080 / 4 | 无实质收益 |

稳态快照不是可靠的瞬态加速初值。Two Basins 中，它把形成过程从 4 年延长为 5 年；实现没有静默回退冷启动，因此报告保留了这个负结果。

## Monthly wind agreement

门槛为相关系数至少 0.95、nRMSE 至多 0.20、方向一致率至少 0.90。下表列出每个夹具十二个月中最差值及月份（1–12）：

| Fixture | min correlation (month) | max nRMSE (month) | min direction agreement (month) | steady speedup vs cold |
|---|---:|---:|---:|---:|
| Aqua Planet | 0.5471 (4) | 1.0576 (10) | 0.8121 (10) | 141.3× |
| Two Basins | 0.3405 (4) | 1.3380 (10) | 0.5667 (4) | 48.5× |
| Earth-like Harmonics | 0.6121 (4) | 0.9614 (10) | 0.6793 (5) | 46.0× |

三个夹具的风场均不是瞬态风场的等价近似。速度优势很大，但不满足所见即所得前提。

## Monthly ocean-current agreement

门槛为相关系数至少 0.90、nRMSE 至多 0.30、方向一致率至少 0.85：

| Fixture | min correlation (month) | max nRMSE (month) | direction agreement | sampled direction area |
|---|---:|---:|---:|---:|
| Aqua Planet | -0.0743 (4) | 1.8940 (4) | 1.0000 | 0% |
| Two Basins | 0.4523 (4) | 1.2160 (10) | 1.0000 | 0% |
| Earth-like Harmonics | 0.4985 (4) | 1.1192 (10) | 1.0000 | 0% |

方向统计要求两者速度都至少 0.01 m/s；本次没有任何同时过阈值的面积，所以 `1.0` 是“无可比较样本”的确定性约定，不是方向验证成功。相关和幅值误差已明确失败。

## Temperature and precipitation agreement

温度门槛为相关至少 0.98、绝对面积加权偏差至多 0.5 °C；降水门槛为月空间相关至少 0.95、年总量相对偏差绝对值至多 2%。

| Fixture | min air-temp corr (month) | max abs temp bias °C (month) | min precip corr (month) | steady / transient annual precip | annual relative bias |
|---|---:|---:|---:|---:|---:|
| Aqua Planet | 0.7584 (10) | 0.5567 (8) | 0.0000 (9) | 0.0000 / 1.514476 | -100.00% |
| Two Basins | 0.6959 (4) | 0.5253 (9) | -0.0211 (5) | 0.035043 / 1.625845 | -97.84% |
| Earth-like Harmonics | 0.7845 (10) | 0.5108 (2) | -0.0099 (5) | 0.005867 / 1.677205 | -99.65% |

降水总量列是十二个月面积均值之和；所有月份等长 30 天，因此比值等同于年总量比值。差异来自稳态月平衡没有保留瞬态季节记忆与过渡期凝结，不能用展示层缩放修正。

## WYSIWYG eligibility failures, if any

三个夹具全部失败：Aqua Planet 57 项、Two Basins 59 项、Earth-like Harmonics 59 项。失败项计数如下：

| Metric | Aqua | Two Basins | Earth-like |
|---|---:|---:|---:|
| wind correlation | 6 | 6 | 6 |
| wind nRMSE | 10 | 10 | 10 |
| wind direction | 6 | 6 | 8 |
| current correlation | 6 | 6 | 6 |
| current nRMSE | 10 | 8 | 8 |
| air-temperature correlation | 6 | 8 | 7 |
| air-temperature bias | 4 | 2 | 1 |
| precipitation correlation | 8 | 12 | 12 |
| annual precipitation total bias | 1 | 1 | 1 |

结论对 `n=4/8/12` 没有趋近通过的趋势；这不是单个阈值边缘失败，而是风、洋流、温度和降水同时发生的大幅差异。

## Evidence-based recommendation

不应把 `BalancedSteadySolver` 用作 `TransientShallowWaterSolver` 的预览：它在三个夹具、三个试测分辨率上都与瞬态周期气候明显不一致，违反所见即所得。也不应把当前显式瞬态实现同步接入参数拖动：`n=12` 已需 40–51 秒，`n=24/32` 外推为数分钟。

若现在只从科学路线中二选一，应保留瞬态方程作为唯一真实性参考，稳态实现仅留作诊断/回归，不进入 UI。进入生产前应先做一次有边界的瞬态优化迭代，优先评估保持同一方程的半隐式/IMEX 重力波与 Coriolis 推进、工作区复用和确定性并行；之后重新执行同一比较协议。异步生成、内容指纹缓存和“显示上一次已收敛结果 + 更新状态”可以解决交互阻塞，但不能替代数值加速。

本轮不选择生产求解器，也未修改 `WorldSpec`、现有气候管线或 UI。

## Questions reserved for the production-integration decision

1. 是否授权下一轮只优化瞬态求解器，并以“科学字段不变、同一快照协议、同一 WYSIWYG 指标”为硬约束？
2. 产品是否接受异步更新与缓存，还是要求 `n=24` 在一次同步操作中完成？
3. 优化后若稳态仍不一致，是否删除其产品入口，仅保留测试用途？
