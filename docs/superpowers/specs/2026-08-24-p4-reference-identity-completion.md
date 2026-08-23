# P4 独立参考与身份债完成记录

日期：2026-08-24
上游规格：`2026-08-23-p4-physical-budget-correction-design.md` R7
状态：Task 2 自动验证完成；最终 UI 验收仍归瞬态气候—地貌总里程碑

## 1. 完成边界

本任务只修复 P4 comparison truth path 与证据身份，不改变生产
`SplitExplicitRk3V1`、P4 artifact、字段或 UI。reference 现在实际执行“一次
标量 endpoint + 独立 classic RK3 平滑动力”，而不是再次实例化 selected split。

实际运行身份写入 `FormationProcedureIdentity.integration_procedure`。单步报告和
formation-cycle 报告都从被运行的积分器取得 identity；候选若与 reference 使用
同一 integration procedure，即使其他字段完全相同也不能 qualify。

## 2. RED 与回归

RED 在生产修改前因以下缺口失败：

- 无 `ClimateIntegrationProcedure`；
- comparison report 无 reference/candidate 实际 procedure；
- diagnostics 无 endpoint 执行次数；
- aggregate gate 无 implementation independence。

GREEN 后的 focused Debug 结果：

- `global_circulation_comparison`：6 passed、1 ignored；
- formation identity/cycle 子集：3 passed；
- `global_circulation_integrators`：19 passed；
- classic RK3 smooth-wave 三阶回归、closed C2 annual mass、取消原子性和逐位确定性
  全部保持通过。

相邻 Debug 回归继续通过：contracts 11 项、generation 7 项、stage 3 项、layered
physics 11 项、natural registry 5 项、field views 6 项。RTX 4080 SUPER / Vulkan
上的 spherical presentation suite 5 项通过，16 幅 `Rgba8UnormSrgb` golden 逐幅
保持既有 BLAKE3。最终 `cargo fmt --all -- --check`、全 workspace/all-targets/
all-features Clippy `-D warnings` 与 `wasm32-unknown-unknown --all-features --lib`
三道门禁均通过。

## 3. Release comparison evidence

`target/p4/integrator-comparison.json` 使用
`sekai.p4-integrator-comparison.v2`：

- V1（`aca5584`）：`146,333 B`，BLAKE3
  `e6faf68523c64e3593af0b6f3f7a3b859f331436f8884e1465da288a531add9c`；
- V2：`430,940 B`，BLAKE3
  `87d005c217f5fb6faef70124026f89bc04c132f20ff2f4757b776cbb5ed8bee3`。

四组 C1/C2、open/coastal fixture 的每月 reference 都报告
`ExplicitEndpointThenClassicRk3V1`、`72` 个细步和 `72` 次 endpoint；candidate
报告 `SplitExplicitRk3V1`。四组 formation 的 reference/split 都在第 1 cycle
达到原 gate。最坏 comparison 数值为 vector NRMSE `0.0476854863`、scalar bias
`0.0466908296`、monthly precipitation bias `0.0063323951`、annual precipitation
bias `0.0063257317`；没有修改任何门槛。

## 4. 产品身份清单

17-seed Draft/C2 P4 writer 重新运行 `272.814149 s`，结果与 R4 冻结证据逐位
相同：

- JSON：`147,831 B`，BLAKE3
  `dbbe8225c4417b92cc8fc04a2c549852d011d5c1dfa819b9af340887654a81d9`；
- CSV：`53,220 B`，BLAKE3
  `476cae8d33bc304a02f249cb35dccd33228cd8a76fd604459ca24f7d25805449`；
- seed 42 artifact JSON：`69,718,763 B`，BLAKE3
  `1421508ed54c318271f09540ef745c51702944c908b093623a79477f1dc2e911`；
- authoritative surface：
  `0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e`；
- climate grid：
  `e363198870843d0a620862a5a03ac98cc48ea5eb74f3a96539c2cce5fd664dcc`。

因果清单：

| 边界 | 本任务结果 |
|---|---|
| P0–P3 | 无代码、schema、artifact 或 fingerprint 变化 |
| P4 equation/global model | 生产方程逐位不变，不刷新 fingerprint domain |
| `GlobalCirculationStage` | 继续 V3，不无依据升版 |
| P5 / T1 | P4 产品 identity 不变，因此本任务不刷新下游；最终前向共演身份由总计划 Task 9 重录 |
| natural registry | `7daf32cc8d7d00033b9bc541c8642bbe6482d30cb85ab99aa0f0a4cf18f9e740` 不变 |
| formation registry | `a9dbe80b57cd69cdaf2bebafd362a8f803b43cdde5c667970f81a8d3a2f5c11a` 不变 |
| vector sampled IDs | `[5, 11, 23, 37, 38, 42, 47, 53, 64, 66, 67, 81, 100, 101, 108, 125, 127, 129, 151, 155, 158]` 不变 |
| 16 幅 GPU golden | 呈现输入与代码均未改变，不刷新字面量；最终总里程碑再次跑完整 GPU suite |

旧 P4 Task 7 中要求冻结旧 fixed-point P5/T1 evidence、atlas 与 performance 的部分
已被 `2026-08-24-transient-climate-geomorphology.md` 明确替代。继续生成即将废止
的 100 ka fixed-point 金样没有产品价值；新的 P3→P5→T1 old→new 清单与两档全量
回归由该计划 Task 9 统一冻结。

## 5. 出处

- Hairer, Nørsett & Wanner (1993), *Solving Ordinary Differential
  Equations I*：classical three-stage, third-order Runge–Kutta reference。
- Press et al. (2007), *Numerical Recipes*, third edition, §9.4：R4 已冻结的
  safeguarded endpoint phase-change root；本任务不改变其公式或常量。
