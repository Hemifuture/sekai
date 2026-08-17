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
