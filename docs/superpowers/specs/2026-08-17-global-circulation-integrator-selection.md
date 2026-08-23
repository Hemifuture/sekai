# P4 Integrator Selection Evidence

Date: 2026-08-17  
Status: frozen Task 6 evidence

## Decision

`SplitExplicitRk3V1` is the only production integrator. The public constant
`SELECTED_PRODUCTION_INTEGRATOR` fixes that decision. IMEX remains runnable
only as a rejected, same-equation comparison strategy and is not a product or
UI option.

No agreement threshold was relaxed during selection.

## Implemented algorithms

The reference is the classical three-stage, third-order Runge-Kutta method:

```text
k1 = F(y_n)
k2 = F(y_n + dt k1 / 2)
k3 = F(y_n - dt k1 + 2 dt k2)
y_(n+1) = y_n + dt (k1 + 4 k2 + k3) / 6
```

Every stage evaluates the same shared layered tendency. Exact constant-field
preservation was repaired at the shared edge interpolation boundary, so the
uniform C1 equilibrium is bit-identical after a step. A refinement study in
the truncation-dominated range passes the declared third-order gate.

Split-explicit decomposes the shared tendency into the fast linear
shallow-water/Coriolis part and the remaining slow part. It evaluates the slow
tendency once at the macro-step start, then adds that frozen tendency to every
stage of deterministic fast RK3 substeps. Thus slow sources and paired
exchanges are integrated once over the macro step rather than once per fast
cycle. The substep count is recomputed from the current maximum characteristic
speed, not just the resting wave speed, and keeps the reported fast CFL at or
below `0.35`.

IMEX applies Crank-Nicolson to the affine height, momentum, temperature, and
deep-reservoir core. Monotone nonlinear temperature transport and moisture are
kept explicit, so they do not contaminate the Krylov operator. It solves the
matrix-free CN increment equation
with bounded unrestarted GMRES and an explicit unit diagonal preconditioner;
humidity uses explicit Heun because its phase-change limiter is nonlinear.
This full-state formulation is algebraically equivalent to a block-eliminated
CN system, but does not implement the planned Schur/Helmholtz performance
optimization. Since IMEX failed the locked accuracy gates and cannot enter the
product, that losing-path optimization is intentionally not promoted as
completed product work.

## Release corpus

Command:

```text
cargo test --release --test global_circulation_comparison release_candidate_corpus_has_at_least_one_universally_qualified_integrator -- --nocapture
```

The four fixed cases cover C1 and C2, open-ocean and blocked-coast edge
permeability, and opposite seasonal months on an `n=3` closed cubed sphere.
Each candidate advances one 21,600-second macro step and is compared with the
same-equation RK3 reference at 300-second steps.

| Case | IMEX actual CN residual | Split vector corr / NRMSE | Split scalar corr | Winner |
|---|---:|---:|---:|---|
| C1 open, month 0 | 0.000003649 | 0.999911 / 0.045007 | 0.999940 | Split |
| C1 coast, month 6 | 0.000003781 | 0.999911 / 0.045228 | 0.999939 | Split |
| C2 open, month 0 | 0.000004120 | 0.999783 / 0.036446 | 0.999904 | Split |
| C2 coast, month 6 | 0.000004133 | 0.999781 / 0.036604 | 0.999903 | Split |

Locked gates are vector correlation `>= 0.995`, vector normalized RMSE
`<= 0.05`, scalar correlation `>= 0.999`, and scalar absolute bias `<= 0.1`.
Split passes all four cases. IMEX is rejected even earlier: after tightening
the internal Krylov stopping criterion by 20 times, a direct substitution into
the complete CN increment equation still leaves `3.65e-6` to `4.13e-6`
relative residual, above the unchanged `1e-6` gate. The comparator records
that candidate failure and continues evaluating split; it never publishes the
unverified IMEX state.

## Regression gates

- uniform equilibrium: all three integrators preserve the exact C1 state;
- RK3 refinement: third-order ratio is tested above the `f32` quantization
  floor;
- large macro step: both candidates remain finite with positive layer depth;
- IMEX: bounded GMRES reaches its declared relative residual;
- deterministic repeat: state and diagnostics compare exactly;
- pre-cancelled runs: all return `Cancelled` without publishing a state;
- artificial vector bias: the locked comparator rejects it;
- shared tendency, transport, circulation operator, and thermodynamic
  regressions remain unchanged.

## 修订 R1（2026-08-24）：独立端点分裂 RK3 参考

本修订替代“每个 RK stage 都评估完整 shared layered tendency”及旧 Release
corpus 数值，但不改变 `SplitExplicitRk3V1` 的生产选择或任何冻结 agreement
门槛。

P4 R4 加入 step-dependent monotone transport、water-limited condensation 与
moist-enthalpy saturation adjustment 后，完整 tendency 已不再是可在三个 RK
stage 重复调用的 autonomous derivative。生产探针曾测得即使 `300 s` 步长，
重复 endpoint 也会造成约 `1.2 K` 空气温度偏差。因此 reference 的一个物理
细步现在明确定义为：

```text
y*        = E_dt(y_n)                 # 标量输运、辐射、交换和相变端点一次
y_(n+1)  = RK3_classic(dt, G, y*)    # 只对完整平滑厚度/动量方程做三 stage
```

`E_dt` 与生产 split 共用唯一 `apply_scalar_endpoint`；`G` 在每个 classic RK3
stage 独立重评估完整显式厚度和动量方程，但不再执行温度、水汽或相变端点。
这保留 Hairer, Nørsett & Wanner (1993) 的 classical three-stage RK3 作为平滑
动力参考，同时遵守 R4 已冻结的“每个物理步只执行一次 endpoint”语义。

运行身份由实际积分器发布的 `ClimateIntegrationProcedure` 给出：reference 为
`ExplicitEndpointThenClassicRk3V1`，候选分别为 `ImexCrankNicolsonV1` 与
`SplitExplicitRk3V1`。候选只有在 capability、守恒解释和 equation model
fingerprint 相同，且 integration procedure 与 reference 不同时才有资格进入
comparison；伪造相同 procedure 的 self-comparison 由 hard gate 拒绝。

Release V2 corpus 覆盖 C1/C2、open/coastal 与十二个月。每个 `21,600 s` 报告
实际执行 `72` 个 reference 细步和 `72` 次 endpoint。四组最坏 split 结果为：

| 指标 | 实测最坏值 | 冻结门槛 |
|---|---:|---:|
| vector normalized RMSE | `0.0476854863` | `<= 0.05` |
| scalar absolute bias | `0.0466908296` | `<= 0.1` |
| monthly precipitation relative bias | `0.0063323951` | comparison 既有门槛 |
| annual precipitation relative bias | `0.0063257317` | `<= 0.01` |

四组 formation reference/split 都在同一个第 1 cycle 达到既有 residual gate；
未放宽阈值。comparison evidence schema 从 V1 升为 V2：`aca5584` 基线为
`146,333 B`、BLAKE3
`e6faf68523c64e3593af0b6f3f7a3b859f331436f8884e1465da288a531add9c`；本修订为
`430,940 B`、BLAKE3
`87d005c217f5fb6faef70124026f89bc04c132f20ff2f4757b776cbb5ed8bee3`。
