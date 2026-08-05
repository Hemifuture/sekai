# Sekai 球面自然产品状态与会话交接

> 交接日期：2026-08-05（Asia/Shanghai）
>
> 实现基线：`main` / `a48564b`（`fix: bind build provenance and peak memory gate`）
>
> 交接范围：S0B.6 球面自然产品接入已完成；下一阶段为 S0C 球面产品呈现

## 1. 一句话状态

球面已经是**新世界自然数据的唯一几何与拓扑事实源**，完整自然生产图、六个球面自然阶段、36 个只读字段及其来源校验都已完成；但当前可见画布仍是明确隔离的旧平面 V1 呈现器，二维球面投影和三维球体呈现尚未开始。

因此，下一会话不应重做 S0B.6，也不应把平面数据升级成另一份权威数据；应进入 S0C，让二维与三维界面共同派生自同一个 `SphericalNaturalFieldDocument`。

## 2. 仓库快照

- 工作分支：`main`
- 最后一个生产实现提交：`a48564b`
- 写交接文档前工作树：干净
- 与远端关系：`main...origin/main [ahead 105]`
- 远端状态：这些本地提交尚未推送；没有得到新的明确指令时不要自行推送
- 原功能分支与 `.worktrees/spherical-natural-product-integration` 已在快进合并后安全删除
- 当前没有已知阻塞项

交接文档自身会作为紧随 `a48564b` 的纯文档提交保存；下一会话应以 `git status --short --branch` 和 `git log -5 --oneline` 重新确认实际 HEAD。

## 3. 已完成内容

### 3.1 唯一事实源与生产图

- `SphericalSurfaceSnapshot` 是新世界几何、拓扑、邻接、面积及稳定实体身份的唯一权威来源。
- `spherical_natural_foundation_graph()` 是球面新世界唯一生产入口，共 16 个细粒度阶段。
- 图只接收八个外部输入，不接收平面空间 Artifact；缺失、多余或跨表面的输入都会被拒绝。
- 根种子、构建报告、最终结果哈希和完整 Artifact 集合已绑定到不可伪造的 `BuildOutcome` 来源校验。
- 结果采用原子发布：候选构建失败时保留上一份完整文档，不暴露半成品。

### 3.2 六个球面自然阶段

以下科学阶段都直接绑定同一个 `SurfaceRef`，并通过各自的类型化 Stage/Artifact 传递：

1. 板块构造（tectonics）
2. 地幔与热点（mantle）
3. 地形起伏（relief）
4. 地质底质（geology）
5. 初步气候（preliminary climate）
6. 水文—侵蚀原子结果（hydro-erosion）

科学算法仍位于独立生成器中；Stage 只负责依赖、缓存、校验和运输，没有复制第二套算法。

### 3.3 数据与呈现解耦

- `SphericalNaturalFieldDocument` 持有八个权威 Artifact 的 `Arc`，不复制球面几何。
- 文档发布 36 个自然字段；字段 ID 到 payload 的映射只存在于 `NaturalFieldPayloadBundle` 一处。
- 标量和类别字段直接借用权威快照；局部东/北向量与边界数组是可丢弃、可重建的显示缓存。
- 文档不包含投影坐标、`PreparedCellMesh`、GPU 数据或 Canvas 状态。
- 同一个成功结果重复构造文档会复用全部 Artifact，同时得到字节一致的派生缓存。

### 3.4 旧平面路径的准确定位

- 旧路径已显式命名为 `legacy_planar_natural_foundation_graph()`，并保留旧公开别名以兼容现有代码与序列化结果。
- 当前 `app.rs` 可见画布只调用这个旧平面入口；这是 S0C 切换前的产品兼容层，不是球面数据的事实源。
- 不要删除平面 V1，直到 S0C 的球面文档构建、二维/三维呈现、交互和工程兼容测试完成。
- 完成切换后，二维地图应只是球面数据的投影视图，三维球体应只是另一种呈现；二者不得各自产生自然事实。

## 4. 尚未完成或刻意不在本阶段完成

- 尚无二维球面投影呈现器。
- 尚无三维球体呈现器。
- 当前界面尚未从旧平面构建切换到球面构建。
- 初步气候不是最终行星环流模型；尚未加入最终大气环流、海洋环流、海气耦合、ENSO、热带气旋或多层气候。
- 尚无跨长时间尺度的地形、侵蚀、板块或生态演化时间线。
- 尚无项目归档格式和最终产品级存取流程。

这些是明确的后续阶段，不是遗漏的 S0B.6 修复项。产品顺序应先完成 S0C 共同呈现边界，再在同一球面事实源上继续增强科学过程，避免科学和显示同时迁移造成双重变量。

## 5. 已验证结果

合并后的 `main` 已重新执行：

```powershell
cargo test --workspace --all-targets --all-features
```

结果：退出码 `0`，耗时约 `170.5 s`。

S0B.6 最终验收还包括：

- `cargo fmt --all -- --check`：通过
- `cargo check --workspace --all-targets --all-features`：通过
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`：通过，零警告
- 完整测试登记：963 个测试，948 个非忽略测试通过，15 个明确忽略；另有 11 个 benchmark case
- `cargo test --workspace --doc`：通过，0 失败，8 个明确忽略示例
- `cargo check --target wasm32-unknown-unknown --all-features --lib`：通过
- 独立最终复审：没有 Critical、Important 或 Minor 问题

20k 球面 Release 基准：

- 平面基线：`1884.693 ms`
- 球面完整图：`1354.992 ms`
- 球面/平面比：`0.718946`
- 球面单独上限：`5 s`；相对上限：`2.5×`，均通过
- 20,252 cells / 40,500 vertices / 60,750 edges
- 常驻语义数据：`22,655,488 B`
- 序列化 Artifact：`56,376,190 B`
- 球面相对平面新增峰值工作集：`0 B`；门限 `256 MiB`

完整哈希、缓存失效矩阵、逐项验收映射和运行环境以实现计划末尾的 `Execution Evidence` 为准，本文件不复制那套详细事实：

- [S0B.6 实现计划与执行证据](../plans/2026-08-04-spherical-natural-product-integration.md)
- [S0B.6 设计](../specs/2026-08-04-spherical-natural-product-integration-design.md)

## 6. 下一会话的首要目标：S0C

先设计再实现一个共同的球面呈现边界，使产品真正达到“同一份结果，二维和三维都可见”：

1. 从现有球面生产图构建并原子发布 `SphericalNaturalFieldDocument`。
2. 二维地图只做球面到屏幕的投影、裁剪、网格和拾取，不重新计算自然数据。
3. 三维球体只做球面网格、相机、着色和拾取，不重新计算自然数据。
4. 两种视图共同使用 36 字段注册表、稳定 cell/edge 身份、色带和数值格式。
5. 预览与实际构建必须使用同一个 Stage 图、输入、种子和 Artifact；允许不同显示 LOD，但不允许不同科学算法。
6. 先保留旧平面 V1 作为兼容回退，等球面端到端验收通过后再决定移除或迁移旧入口。

建议 S0C 的设计至少覆盖：投影选择与接缝/极点、2D/3D 共用字段层、网格缓存所有权、交互拾取身份、相机与视图状态、WASM/native 性能、原子切换，以及旧项目兼容策略。

### S0C 必须守住的验收原则

- 2D 与 3D 查询同一实体、同一字段时返回完全相同的权威数值。
- 改变投影、相机、缩放或显示 LOD 不得使自然生产图失效。
- 重新生成世界会使旧显示缓存失效，但不会混合新旧 Artifact。
- 投影接缝和极点不会复制、漏掉或改变稳定实体身份。
- 呈现模块不得反向依赖自然科学生成器的内部实现。
- 球面文档继续保持无投影、无 mesh、无 GPU 状态。

## 7. 继续工作时先读这些文件

按顺序：

1. 本交接文档。
2. [S0B.6 设计](../specs/2026-08-04-spherical-natural-product-integration-design.md)。
3. [S0B.6 实现计划与执行证据](../plans/2026-08-04-spherical-natural-product-integration.md) 的 `Execution Evidence`。
4. `src/app/spherical_natural_display.rs`：球面只读字段文档与来源校验。
5. `src/app/field_document.rs`：数据文档与已呈现文档的契约边界。
6. `src/app/natural_field_payloads.rs`：36 字段的唯一 payload 映射。
7. `src/generators/natural/spherical_stage.rs`：球面总图及部分 Stage/Artifact。
8. `src/world/spatial/sphere_geometry.rs`：规范球面局部切向基。
9. `src/app.rs`：仍在使用旧平面呈现器的实际产品切换点。

## 8. 不应破坏的架构约束

- 球面快照唯一权威；二维坐标和三维 mesh 都只是派生缓存。
- 科学生成、Stage 编排、字段文档、投影、渲染、交互分别保持正交。
- 一个稳定概念只保留一个映射或事实来源；不得为 2D/3D 复制字段表、色带规则或科学数据。
- 跨模块依赖应指向窄接口，呈现层消费字段文档，不穿透到各科学生成器。
- 每个派生缓存都必须能由权威 Artifact 与明确的视图参数重建。
- 所有发布都保持完整性和原子性；不要用平面物理静默替代球面构建失败。
- 保持 Rust 1.85、native 与 `wasm32-unknown-unknown` 兼容，不随意增加依赖。

## 9. 给下一会话的直接启动语

可以直接对新会话说：

> 请先阅读 `docs/superpowers/handoffs/2026-08-05-s0b6-spherical-natural-handoff.md`，核对 git 状态后从 S0C 开始。不要重做 S0B.6；先完成二维投影与三维球体共用同一球面字段文档的设计，再按测试驱动实现。保持球面快照为唯一事实源、预览和实际同算法、显示缓存可重建、模块正交。非方向性问题请自行作出专业判断并继续。
