# Sekai 独立字段显示系统 V1 设计

日期：2026-07-29
状态：已授权设计，待实施计划
上位设计：`docs/superpowers/specs/2026-07-28-sekai-current-slice-world-design.md`

## 1. 目标

本子系统为世界作者和生成算法开发提供一个独立、可验证的字段观察器。它把稳定的空间几何、字段 schema、字段值和显式诊断转换为显示数据，但不参与世界生成，也不修改世界真值。

V1 必须做到：

- 枚举并检查已注册字段；
- 在有限平面单元格上显示连续标量和离散分类字段；
- 使用字段 schema 中的单位、有效范围、标签键和调色板语义；
- 支持字段选择、显示范围、调色板、图例、单元格选择和诊断叠加；
- 静态几何只在空间内容改变时构建和上传；
- 字段值只在字段内容或选择改变时准备和上传；
- 调色板和范围切换主要通过 GPU 参数完成；
- 使用确定性夹具、CPU 参考采样、GPU 抽查和截图基线验证显示正确性；
- 让现有地形应用通过单向适配器使用新显示路径，同时不把旧 `MapSystem` 变成新领域契约。

显示系统是后续自然、魔法和社会生成的观察基础，不是最终地图美术系统。

## 2. V1 范围

### 2.1 包含

- renderer-neutral 的 `view` 模块；
- 从 `FieldRegistry`、`ExtensionFieldSet` 和只读空间数据构造字段目录；
- 单元格域 `ScalarF32` 与 `CategoryU32` 的 GPU 填色；
- 顺序与发散标量调色板、分类调色板；
- schema 范围、数据范围和手工范围；
- 图例值、单位、分类标签和字段依赖显示模型；
- 单元格悬停或点击选择，以及所选值检查；
- 单元格级错误、警告和信息诊断掩码；
- 无字段、字段缺失、类型不支持和 GPU 预算错误的非崩溃状态；
- 现有高度和板块编号到 V1 字段视图的应用层适配；
- 原生与 WASM 构建；
- CPU、GPU、截图和上传次数测试。

### 2.2 明确排除

- 生成或修改任何自然、魔法、社会字段；
- `WorldSnapshot` 空壳；
- 历史年代、事件、时间滑块和变化回放；
- 地形晕渲、等高线、纹理、阴影和最终制图美化；
- 向量箭头、河流/道路/地脉网络、聚落点和行政区叠加；
- 编辑命令、笔刷、约束、撤销重做和局部重算；
- 项目保存、显示偏好持久化和外部城镇链接；
- 把渲染颜色、归一化值、GPU 缓冲或选择状态写入 `world`；
- 为兼容旧应用而让 `view` 依赖 `models`、`terrain`、`app`、egui 或 wgpu。

向量、网络和实体叠加是显示系统的下一交付片，不在 V1 中预建空接口。届时由真实自然或社会契约以及独立设计驱动。

## 3. 方案比较与决定

### 3.1 方案 A：直接泛化旧高度图渲染器

优点是改动小、很快能显示多个颜色图层。缺点是继续以 `MapSystem`、`CellsData`、egui `Pos2` 和旧 Voronoi 数据为核心接口，未来正式快照仍需重写，而且难以证明每帧没有重建。

不采用。

### 3.2 方案 B：CPU 生成每顶点颜色

优点是着色逻辑简单、容易做截图。缺点是字段、范围或调色板变化都要在 CPU 重建大量颜色，并重复每个多边形顶点的颜色，违反性能边界。

不采用。

### 3.3 方案 C：只读字段视图 + 准备数据 + 通用 GPU 字段渲染器

`view` 只理解世界契约；准备阶段把空间多边形转换为稳定的显示网格，把字段转换为每单元一个紧凑值；GPU 顶点通过单元 ID 读取字段值，片元阶段根据 uniform 和固定调色板着色。应用层负责把当前旧数据或未来正式快照适配到同一入口。

采用该方案。它在领域数据、显示语义、GPU 资源和 UI 编排之间建立单向依赖，并允许渐进迁移。

## 4. 系统边界

依赖方向必须是：

```text
world
  ↑
view
  ↑        ↑
gpu/field  ui/field
     \      /
        app
```

约束：

- `world` 不依赖 `view`、egui、wgpu 或应用状态；
- `view` 只依赖 `world`；
- `view` 不依赖 `engine`，避免显示契约反向绑定构建调度；
- `gpu/field` 只消费 `view` 已准备的数据和 wgpu 资源；
- `ui/field` 只消费 `view` 的目录、状态和检查器模型；
- `app` 是唯一可以同时看见旧模型、构建报告、UI 和 GPU 资源的组合根；
- `engine::BuildReport` 到诊断视图的转换属于应用适配，不进入 `view`；
- 生成器、规则包和字段 schema 不能指定任意 RGBA 颜色或 GPU 行为，只能提供已有的语义调色板提示；
- 显示选择、范围覆盖、透明度和悬停状态不是世界真值。

## 5. Renderer-neutral 字段契约

### 5.1 借用视图

`view::field` 定义借用式 `FieldView<'a>`。它持有 `&FieldSchema` 和 `&FieldData`，不复制完整字段。

主要接口：

```rust
pub struct FieldView<'a> {
    schema: &'a FieldSchema,
    data: &'a FieldData,
}

impl<'a> FieldView<'a> {
    pub fn schema(&self) -> &'a FieldSchema;
    pub fn len(&self) -> usize;
    pub fn value(&self, index: usize) -> Option<FieldValue>;
    pub fn scalar_values(&self) -> Option<&'a [f32]>;
    pub fn category_values(&self) -> Option<&'a [u32]>;
    pub fn vector_values(&self) -> Option<&'a [[f32; 2]]>;
    pub fn stable_id_values(&self) -> Option<(StableIdKind, &'a [u32])>;
}
```

`FieldValue` 是一个小型复制值枚举，用于检查器访问单个元素。布尔字段通过逐值读取处理，不要求把 `Vec<bool>` 暴露为不存在的普通切片。

### 5.2 字段目录

`FieldCatalog<'a>` 按 `FieldId` 稳定排序，来自：

- 不可变 `FieldRegistry`；
- 已验证 `ExtensionFieldSet`；
- 后续强类型核心字段适配器。

V1 的 `FieldCatalog::from_extension_fields` 枚举注册表中的所有 schema，而不是只枚举已有 payload。每项状态为：

- `Available(FieldView)`；
- `MissingPayload`。

这样字段缺失会成为显式可显示状态，不会静默消失。目录只分配少量条目和引用，不复制字段数组。

`FieldCatalogBuilder` 允许未来核心字段注册借用视图。它拒绝重复 `FieldId`、schema/值类型不匹配和不同长度的同域字段。

### 5.3 V1 可渲染矩阵

| 域 | ScalarF32 | CategoryU32 | Boolean | Vector2F32 | StableIdU32 |
|---|---:|---:|---:|---:|---:|
| Cells | 填色 | 填色 | 仅检查器 | 仅检查器 | 仅检查器 |
| Global | 仅检查器 | 仅检查器 | 仅检查器 | 仅检查器 | 仅检查器 |
| Edges | 仅检查器 | 仅检查器 | 仅检查器 | 仅检查器 | 仅检查器 |
| Entities | 仅检查器 | 仅检查器 | 仅检查器 | 仅检查器 | 仅检查器 |

“仅检查器”是完整支持的只读值查看状态，不创建地图填色。UI 必须明确显示“不适用于 V1 单元填色”，不得降级为错误颜色或空白。

## 6. 空间显示网格

### 6.1 输入

正式路径从实现 `Topology` 的有效 `SpatialSnapshot` 读取：

- 世界边界；
- 连续 `CellId`；
- 每个单元的逆时针多边形。

`view::mesh::CellMeshBuilder` 也提供受验证的逐单元构造 API，供测试夹具和应用层旧数据适配器使用。该 API 仍要求：

- 单元 ID 连续且不重复；
- 每个已显示单元至少三个有限顶点；
- 多边形位于声明边界内；
- 三角索引和容量转换不溢出。

旧适配器可以把旧 Voronoi 中无法填充的单元记录为诊断并省略其三角形，但必须保留原始字段索引；正式 `SpatialSnapshot` 路径不允许缺失几何。

### 6.2 准备格式

`PreparedCellMesh` 包含：

- 归一化到 `[0, 1] × [0, 1]` 的 `f32` 顶点位置；
- 每个顶点对应的 `u32 CellId`；
- `u32` 三角形索引；
- 原始 `WorldRect`；
- 去掉世界原点后的本地宽高；
- 单元数量；
- CPU 选择索引；
- 非语义 mesh revision。

坐标先用 `f64` 减去边界原点并除以边界尺度，再转为 `f32`，避免大世界坐标直接降精度。渲染时只把归一化坐标乘以本地宽高，不重新加回可能很大的世界原点；画布相机工作在这个原点平移后的本地空间。每个单元使用扇形三角剖分；输入来自已验证的凸 Voronoi 多边形。

### 6.3 选择索引

选择索引按固定网格分桶保存单元包围盒。画布点击位置先转换为本地坐标，再除以本地宽高成为归一化坐标；查询只检查命中桶中的候选多边形，并用点在多边形内测试确认。分桶构造顺序和候选排序必须确定；它是可删除的显示派生物。

选择不改变世界字段。不存在命中时返回 `None`。

## 7. 显示状态与调色板

### 7.1 状态

`FieldDisplayState` 是 UI-independent 的显示偏好：

- 选中的 `FieldId`；
- `DisplayRangeMode`；
- 可选调色板覆盖；
- 诊断叠加是否启用；
- 当前选中的 `CellId`；
- 图例展开状态。

V1 不把该状态写入项目文件。状态引用的字段消失时，控制器按稳定排序选择第一个可渲染字段；没有可渲染字段时进入明确空状态。

### 7.2 范围

`DisplayRangeMode`：

- `Schema`：使用 `FieldSchema.valid_range`；
- `Data`：使用字段有限最小值和最大值；
- `Manual(ValueRange)`：使用经验证的手工范围。

规则：

- 有 schema 范围时默认 `Schema`，否则默认 `Data`；
- 常量字段映射到调色板中点；
- 手工范围之外的有效值被钳制，但图例显示实际范围与显示范围；
- 非有限值不应出现在已验证字段中；测试或旧适配器出现时使用诊断洋红色，并产生诊断；
- 无可用范围时不发布半准备字段。

### 7.3 调色板

`view::palette` 定义 renderer-neutral 的线性 RGBA 和确定性采样：

- `Sequential`：低到高的感知有序色带；
- `Diverging`：负侧—中点—正侧；
- `Categorical`：固定、色盲可区分的有限色表；
- 诊断色：错误洋红、警告橙、信息青、缺失黑白棋盘语义。

颜色不进入 `FieldSchema`。schema 只保留 `FieldPaletteHint`，显示系统从允许的内置调色板集合选择。

分类键按 schema 中排序后的键映射到紧凑索引，颜色索引取 `compact_index % categorical_palette_len`。图例最多展开前 256 个排序项；超过时颜色会按同一公式重复，UI 明确显示总数和截断提示。未知分类键使用错误色，不过已验证 `ExtensionFieldSet` 正常不会产生它。

CPU 参考采样与 WGSL 必须共享相同的：

- 范围归一化规则；
- 色标停止位置；
- 分类键映射表；
- 诊断覆盖优先级。

## 8. 诊断与检查器

### 8.1 诊断输入

为保持 `view → world` 的依赖方向，`view` 定义最小展示输入：

```rust
pub struct CellDiagnosticRef<'a> {
    pub severity: ViewDiagnosticSeverity,
    pub code: &'a str,
    pub field_id: Option<&'a FieldId>,
    pub cell_id: Option<CellId>,
    pub message: &'a str,
}
```

应用层把 `BuildReport::diagnostics()` 映射到该结构。无 cell 的诊断进入列表；有 cell 的诊断同时生成每单元掩码。

叠加优先级为 `Error > Warning > Info > None`。诊断掩码是独立显示缓冲，不修改字段值。选择具体字段时，默认显示全局诊断和该字段诊断；用户可切换为全部字段诊断。

### 8.2 检查器

选择字段和单元后，检查器显示：

- 字段 label key，若无本地化则回退到完整 `FieldId`；
- 原始值、格式化值、单位和分类标签；
- schema 有效范围、当前显示范围；
- 字段依赖；
- 该字段/单元的诊断；

V1 不显示空的“值来源”区域。真正的 provenance 适配在自然或规则子系统提供来源数据后单独加入。

## 9. GPU 架构

### 9.1 缓冲

`gpu::field::CellFieldRenderer` 维护：

- 顶点缓冲：归一化位置和 `CellId`；
- 索引缓冲；
- 标量 `f32` 或分类 `u32` 字段缓冲；
- 分类紧凑索引缓冲；
- 诊断 `u32` 掩码缓冲；
- 调色板缓冲；
- 画布、范围、字段种类和诊断模式 uniform；
- 已上传 revision 与容量；
- 仅用于报告的上传计数和字节数。

GPU 不持有 `WorldSnapshot`、`FieldRegistry` 或 `ExtensionFieldSet`。

### 9.2 更新协议

CPU 先完整构造 `PreparedCellMesh`、`PreparedCellField` 和 `PreparedDiagnosticMask`。只有全部校验成功，应用资源才原子替换当前显示包并增加 revision。

渲染器在 `prepare` 中比较 revision：

- mesh revision 改变：上传几何和索引；
- field revision 改变：上传每单元字段；
- diagnostic revision 改变：上传诊断掩码；
- palette revision 改变：上传色标；
- 每帧只允许更新画布变换等小 uniform。

字段切换不重建几何。范围切换不重传字段。静态画面不得分配与单元数成比例的 CPU 容器。

### 9.3 缓冲容量

缓冲按需求增长并复用，不使用当前固定一百万顶点的静态假设。显式 V1 预算：

- 最多 200,000 个单元；
- 最多 6,000,000 个显示顶点；
- 最多 12,000,000 个三角索引；
- 单次 GPU 缓冲大小必须经过 `u64` 检查；
- 超过预算返回结构化 `DisplayPrepareError`。

这些是显示分配上限，不改变世界生成预算。提高上限必须有峰值内存和上传时间证据。

### 9.4 Shader

顶点着色器：

1. 读取归一化位置和 `CellId`；
2. 将归一化位置乘以本地宽高并应用画布变换；
3. 通过 `CellId` 读取该单元的标量位模式或分类紧凑索引；
4. 使用显示范围和调色板求出基础颜色；
5. 按诊断优先级覆盖或混合；
6. 把颜色传给片元阶段。

片元着色器：

1. 接收同一单元三个顶点一致的颜色；
2. 输出线性到目标格式所需颜色。

着色器不包含地形、气候、魔法或社会业务规则。

## 10. UI 与现有应用接入

### 10.1 UI

V1 在现有左侧面板增加独立“字段观察”区域：

- 按稳定顺序选择字段；
- 显示字段类型、域和单位；
- 选择 schema/data/manual 范围；
- 选择兼容调色板；
- 开关诊断；
- 展示图例；
- 展示悬停/选中单元的值。

UI 只修改 `FieldDisplayState` 并请求准备或 uniform 更新。它不直接访问 GPU 缓冲，不调用生成器，也不修改字段数组。

### 10.2 旧应用适配

`app` 中的私有 `LegacyTerrainDisplayAdapter` 在地形生成完成时一次性读取：

- 旧 Voronoi 单元几何；
- `height`；
- 生成器返回的板块编号。

它构造：

- `sekai.legacy.elevation@1`：单元标量，值由旧 `u8` 转为显示用 `f32`；
- `sekai.legacy.plate_id@1`：单元分类；
- 一个仅供显示的准备网格。

该适配数据：

- 不序列化；
- 不进入构建哈希；
- 不被自然生成阶段读取；
- 不成为 `WorldSnapshot`；
- 每次旧地形重新生成后整体替换；
- 仅用于证明当前应用可以迁移到新显示协议。

原有 Delaunay、Voronoi 边和点开关暂时保留。旧 `HeightmapCallback` 被新字段回调替代后删除，避免同时维护两套单元填色路径。

未来自然快照接入时，只替换应用层数据源，不修改 `view`、shader 或 UI 字段控制器。

## 11. 错误处理

结构化错误至少区分：

- 未知字段；
- 已注册但无 payload；
- schema 与数据类型不一致；
- 字段域长度与空间单元数不一致；
- V1 不支持该域/类型的地图填色；
- 无有效显示范围；
- 非有限旧适配值；
- 非法或缺失单元几何；
- 顶点、索引或缓冲预算超限；
- 数值或字节大小转换溢出；
- GPU 资源创建或上传失败。

准备失败时：

- 不替换最后一个有效显示包；
- UI 展示错误；
- 渲染器不读取部分缓冲；
- 世界、缓存和生成结果不受影响。

禁止在正常用户输入、缺失字段或资源不匹配路径上 `unwrap`。

## 12. 测试设计

### 12.1 契约测试

- 字段目录稳定排序；
- 已注册但缺失 payload 可见；
- 重复字段、schema/数据不匹配和长度错误被拒绝；
- 所有值类型可由检查器安全读取；
- 只有 V1 支持矩阵中的组合可准备为填色字段；
- 字段选择失效时确定性回退。

### 12.2 网格与选择测试

- 四单元有效夹具得到确定顶点和索引；
- 大坐标归一化不丢失宏观位置；
- CellId 与字段索引保持一致；
- 预算和整数溢出被拒绝；
- 点选择在边界内稳定，边界外返回 `None`。

### 12.3 调色板与诊断测试

- CPU 标量端点、中点、常量和钳制结果；
- 发散零点；
- 分类键稳定映射；
- 未知分类、非有限值、缺失值使用诊断色；
- Error/Warning/Info 优先级；
- 图例单位、范围和标签与 schema 一致。

### 12.4 GPU 测试

- WGSL 在原生和 WASM gate 中编译；
- 小型离屏目标渲染标量与分类夹具；
- 选定像素与 CPU 参考值每通道误差不超过一个 8-bit 量化级；
- 调整范围只增加 uniform 更新，不增加 geometry/field 上传；
- 切换字段只增加 field 上传；
- 重复静态帧不增加 geometry、field 或 diagnostic 上传。

无可用 GPU 适配器的开发机器可以跳过离屏抽查，但 CI 必须至少在一个软件或硬件后端执行；纯跳过不构成验收通过。

### 12.5 截图基线

提交小型、固定夹具的标量、分类和诊断 PNG：

- 固定分辨率、固定色彩空间；
- 由 CPU 参考渲染器生成；
- 测试比较像素和稳定 BLAKE3；
- 更新基线必须是显式命令，不在测试失败时自动覆盖。

### 12.6 边界扫描

验收必须证明：

```powershell
rg -n 'egui|eframe|wgpu|crate::app|crate::gpu|crate::ui|crate::engine|crate::generators|crate::terrain|crate::models' src/view
rg -n 'crate::view|egui|eframe|wgpu' src/world src/engine src/generators
rg -n 'crate::engine|crate::generators|crate::terrain|crate::models' src/gpu/field src/ui/field
```

均无越界依赖。

## 13. 预期文件边界

```text
src/
├─ view/
│  ├─ mod.rs
│  ├─ field.rs          # 借用字段视图和目录
│  ├─ mesh.rs           # 准备网格和选择索引
│  ├─ palette.rs        # renderer-neutral 调色板与 CPU 参考采样
│  ├─ diagnostics.rs    # 诊断输入和每单元掩码
│  ├─ prepared.rs       # 原子准备包与 revision
│  └─ state.rs          # 显示状态与控制器
├─ gpu/
│  └─ field/
│     ├─ mod.rs
│     ├─ callback.rs
│     └─ renderer.rs
├─ ui/
│  └─ field/
│     ├─ mod.rs
│     ├─ controls.rs
│     └─ inspector.rs
└─ app.rs               # 组合根和私有旧数据适配

assets/shaders/field_fill.wgsl
tests/field_view_contracts.rs
tests/field_display_mesh.rs
tests/field_display_golden.rs
tests/field_display_integration.rs
tests/golden/field-display/*.png
```

文件可在实施计划中按共同变化的职责合并，但不得把 view、GPU、UI 和旧适配器放入同一文件。

## 14. 实施切片

1. renderer-neutral 字段目录、值检查和状态；
2. 调色板、范围、诊断掩码和 CPU 参考采样；
3. 空间网格准备、容量预算和选择索引；
4. GPU 标量/分类填色与上传 revision；
5. 字段控制、图例和检查器 UI；
6. 旧高度/板块单向适配并移除旧填色回调；
7. 黄金截图、离屏 GPU 抽查、性能断言和全仓门禁。

每个切片按 TDD 完成并单独提交。前一切片的公共契约通过测试后，后一切片才能依赖它。

## 15. V1 验收标准

V1 完成时必须满足：

- `view` 只依赖 `world`；
- `world`、`engine` 和生成器不知道显示系统存在；
- scalar/category 两种字段共享同一个 GPU 填色器；
- 字段 schema 决定单位、标签、范围语义和兼容调色板；
- 正式 `SpatialSnapshot` 夹具和旧应用适配走相同准备/渲染入口；
- 字段、范围、调色板和诊断切换不会修改世界数据；
- 静态第二帧不会重建或重传几何和字段；
- 相同输入生成相同准备数据与 CPU 截图；
- CPU/GPU 颜色抽查在允许误差内一致；
- 旧高度与板块字段可在当前应用中选择和检查；
- 缺失或不支持字段显示明确原因且不崩溃；
- 原有地图生成、Delaunay、Voronoi、地形测试继续通过；
- 原生、Release、WASM 和 Trunk 构建通过。
