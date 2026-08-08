# Sekai 球面产品呈现设计（S0C）

日期：2026-08-08

状态：产品方向已由用户确认；技术细节已授权按专业判断定稿；等待书面规格复核

上游依据：

- `docs/superpowers/handoffs/2026-08-05-s0b6-spherical-natural-handoff.md`
- `docs/superpowers/specs/2026-08-04-spherical-natural-product-integration-design.md`
- `docs/superpowers/specs/2026-08-03-evolvable-layered-climate-architecture-design.md`

范围：让同一个 `SphericalNaturalFieldDocument` 在二维投影地图和三维球体中完整可见，并共享字段、标注、选择和来源身份。本阶段把应用的新建世界路径切换到权威球面生产图，同时保留旧平面 V1 的显式兼容入口。

## 1. 决策摘要

S0C 采用“共享字段语义、独立二维／三维 presenter、统一稳定身份、显示缓存可删除”的架构。

- 应用使用单画布，在二维地图和三维球体之间切换，不同时分屏。
- 二维默认使用 Equal Earth 等面积投影，并提供等距圆柱投影作为经纬与接缝诊断视图。
- 三维球体永远使用未变形的单位球面。高程以及其他任何字段都只能通过色带、线条、箭头、数值和选中标注表达，绝不位移球面顶点。
- 36 个现有自然字段全部可查看：32 个单元标量／分类字段用于填色，2 个边字段用于线条，2 个单元向量字段用于箭头。
- 显示状态有一个单元填色槽位和一个可选叠加槽位。叠加槽位只接受边字段或向量字段。
- 风、未来洋流和季风等向量使用动态箭头。长度和颜色表达权威强度，流动相位表达方向；动画速度是可读性映射，不冒充物理时间。
- 二维和三维使用同一个字段目录、色带、数值格式、诊断、`CellId`／`EdgeId` 选择和来源身份；只有几何、相机与拾取入口不同。
- 投影、相机、缩放、动画和显示 LOD 不进入 Stage 图、Artifact、构建哈希或科学缓存键。
- 不引入任意维度／任意几何的万能 renderer，也不增加新的第三方依赖。

## 2. 目标与非目标

### 2.1 目标

1. 从正式球面自然图构建并原子发布 `SphericalNaturalFieldDocument`。
2. 从同一权威球面快照派生二维地图网格、三维球体网格和统一拾取索引。
3. 在两个视图中使用字节一致的字段 payload 和相同的显示语义。
4. 正确处理二维接缝、极点、投影轮廓、三维背面隐藏和稳定实体选择。
5. 让静态画面不做与单元数成比例的逐帧 CPU 工作；动画只更新小型 uniform。
6. 保持 Rust 1.85、native 和 `wasm32-unknown-unknown` 兼容。
7. 在球面端到端验收完成后，使全新应用状态默认创建球面世界。

### 2.2 非目标

- 不按高程、地壳厚度、流量或任何其他字段改变球体半径或单元形状。
- 不实现地形挤出、法线贴图、阴影、昼夜光照、云层或最终地图美术。
- 不把箭头动画解释为时间演化、季节推进或真实粒子追踪。
- 不新增最终大气环流、海洋环流、季风模型或时间维字段；未来这些字段只复用本阶段建立的向量呈现契约。
- 不实现自由飞行相机、透视摄影模式或 2D/3D 同屏联动。
- 不把旧平面世界自动包裹、投影或重采样成球面。
- 不建立正式项目存档容器；只给当前持久化 UI 状态增加明确的来源标签和兼容迁移入口。

## 3. 所有权与依赖方向

权威数据与派生呈现的依赖关系固定为：

```text
BuildOutcome
    ↓ verified provenance
SphericalNaturalFieldDocument
    ├── FieldCatalog / 36 borrowed payloads
    └── SphericalSurfaceSnapshot
             ↓
PublishedSphericalPresentation
    ├── PreparedFieldLayers       共享字段、色带、诊断与标注语义
    ├── PreparedProjectedMap      二维几何与接缝派生
    ├── PreparedGlobeMesh         三维单位球几何派生
    └── SphericalEntityLocator    统一 CellId / EdgeId 定位
             ↓
    ┌────────┴────────┐
2D map presenter   3D globe presenter
```

模块依赖继续保持：

```text
world
  ↑
view
  ↑        ↑
gpu       ui
   \      /
      app
```

约束：

- `world` 不知道投影、相机、egui 或 wgpu。
- `view` 只消费 `world` 的只读快照和字段 schema，不依赖 `engine`、`app` 或 GPU。
- `gpu` 只消费已经验证的 `view` 准备包，不读取生成器或 Artifact。
- `app` 是唯一同时看见 `BuildOutcome`、字段文档、UI 和 GPU 资源的组合根。
- `SphericalNaturalFieldDocument` 保持无投影、无 mesh、无 GPU 状态。
- 现有 `NaturalFieldPayloadBundle` 继续是 36 个自然字段 ID 到 payload 的唯一映射。

## 4. 来源身份与原子发布

### 4.1 呈现来源

每个球面呈现派生物携带不可自行构造的来源值：

```text
SphericalPresentationSource
├── root_seed
├── surface_ref
├── build_result_hash
└── graph_contract_version
```

它只能从已验证 `SphericalNaturalBuildIdentity` 的全部只读值构造，不重新计算或接受外部拼装的世界身份。以下对象在组合时必须具有完全相同的来源：

- 字段层；
- 当前二维投影网格；
- 三维球体网格；
- 统一实体定位器；
- GPU 显示包。

来源不一致是结构化错误，不能按相同单元数继续组合。

### 4.2 世界候选发布

一次新世界构建按以下顺序完成：

1. 使用 `spherical_natural_foundation_graph()` 和精确的八个外部 Artifact 构建 `BuildOutcome`；
2. 从经来源校验的 outcome 构造完整球面字段文档；
3. 构造统一实体定位器；
4. 构造默认 Equal Earth 二维网格和三维单位球网格；
5. 按保留后的显示选择准备填色与叠加字段；
6. 对文档、几何、字段和来源执行交叉验证；
7. 一次性替换当前 `PublishedSphericalPresentation`。

任何一步失败都保留上一份完整世界和显示包。禁止发布只有文档、只有二维或只有三维的半成品，也禁止静默退回平面物理。

### 4.3 显示级原子更新

- 更换投影或中央经线时，只构造新的二维候选网格；成功后替换二维缓存。
- 更换填色／叠加字段时，只构造新的字段层候选；成功后替换字段层。
- 相机、平移、缩放和动画相位只修改小型视图状态或 uniform。
- GPU 上传先完成尺寸与来源预检，再替换 renderer 内部资源；失败继续绘制上一次成功上传的包。

## 5. 共享字段与标注层

### 5.1 两个显示槽位

`SphericalFieldDisplayState` 在现有字段状态基础上明确拆分：

- `fill_field: FieldId`：一个 `Cells + ScalarF32/CategoryU32` 字段；
- `overlay_field: Option<FieldId>`：零个或一个 `Edges + ScalarF32/CategoryU32` 或 `Cells + Vector2F32` 字段；
- 填色范围、填色色带、叠加范围和叠加色带；
- 诊断开关与范围；
- `SelectedSurfaceEntity`；
- 向量动画暂停、显示速度和 glyph LOD。

默认填色为最终地表高程，默认无叠加。切换二维／三维时完整保留这两个槽位和选择。

### 5.2 类型到视觉通道

| 字段域与类型 | 视觉表达 | 精确值位置 |
|---|---|---|
| Cells + ScalarF32 | 单元填色，按显示范围采样顺序／发散色带 | 检查器与选中标注 |
| Cells + CategoryU32 | 单元填色，按稳定分类键采样分类色带 | 检查器与选中标注 |
| Edges + ScalarF32 | 线条颜色与有界线宽 | 检查器与选中标注 |
| Edges + CategoryU32 | 稳定分类颜色与固定基础线宽 | 检查器与选中标注 |
| Cells + Vector2F32 | 局部切向动态箭头，长度与颜色表达模长 | 分量、模长、单位与方向角 |

不为每个字段建立 presenter 私有映射。字段 schema 决定域、值类型、单位、范围、依赖和调色板语义；共享准备层据此选择视觉通道。

`FieldPaletteHint::Vector` 在共享准备层中选择一套统一的顺序模长色带；长度与颜色都读取同一个已解析模长范围。该规则不修改既有字段 schema，也不按字段 ID 建第二张样式表。

### 5.3 向量契约与动态箭头

球面文档中的 `Vector2F32` 统一解释为规范局部东／北分量：

- 三维 presenter 使用权威 cell radial 和 `canonical_east_north_basis` 重建三维切向量；
- 二维 presenter 使用投影在该点的局部有限差分／解析 Jacobian，把同一切向量映射到屏幕方向；
- 数据在极点继续使用已经冻结的规范切向基；若二维投影在该点的 Jacobian 降秩且映射后方向低于有限阈值，则只省略该二维 glyph 并发布显示诊断，不能任意旋转箭头。三维箭头和检查器值仍然存在；
- 零向量不绘制方向 glyph，但检查器仍显示精确零值；
- 非有限值在文档验证阶段已被拒绝，presenter 不修补数据。

箭头由静态实例数据和逐帧 uniform 组成：

- 长度和颜色由字段模长及显式显示范围确定；
- 箭头朝向是权威向量方向；
- 沿箭身移动的亮段／箭头相位显示流向；
- 动画速度经有界可读性曲线映射，不宣称等于物理秒、季节或传播时间；
- 暂停时保留静态方向和强度；
- 动画开启时只更新相位 uniform，并请求下一帧，不重建或重传实例数组。

未来洋流、季风或其他时间切片向量可以复用同一视觉契约。时间切片的选择和科学演化必须由未来字段／时间设计显式提供，不能由动画相位伪造。

### 5.4 标注密度与 LOD

边线可以按选中字段过滤无事件／零强度记录，但不能改变底层 edge 值。向量 glyph 使用由稳定 `CellId` 和来源身份决定的嵌套 LOD 子集：

- 远景显示稀疏、确定性的代表单元；
- 放大后逐级加入更多单元，已有 glyph 不跳换身份；
- 当前选中 cell 始终包含在 glyph 集合中；
- LOD 只影响 glyph 实例缓存，不影响字段 payload、拾取或科学缓存。

每帧不得扫描全部 cell 来重新决定 LOD。只有跨越离散缩放阈值时才重建实例集合。

## 6. 二维投影地图

### 6.1 投影集合

S0C 首版只提供两个明确投影：

1. `EqualEarth`：默认主题地图投影，保持相对面积，适合比较高程、降水、地质和水文等全球字段；
2. `Equirectangular`：经纬线性诊断投影，用于接缝、极点和经纬定位测试，不宣称面积或形状正确。

Equal Earth 使用原作者发布的球面正反算公式与系数，不引入外部投影库。其逆算采用有最大迭代次数和收敛阈值的 Newton–Raphson；不收敛或落在地图轮廓外时返回结构化的无命中／投影错误。

参考：

- [Equal Earth 原始论文](https://doi.org/10.1080/13658816.2018.1504949)
- [Equal Earth 数学与实现细节](https://shadedrelief.com/ee_proj/EEp_Math_and_Implementation_details_%202019-04-16.pdf)

`SphericalProjection` 是 renderer-neutral 的窄接口，只负责：

- 单位方向与二维规范坐标的正算／逆算；
- 投影轮廓和规范 bounds；
- 中央经线归一化；
- 局部向量方向映射；
- 明确错误。

不预建任意椭球、任意 datum 或插件式投影系统。

### 6.2 接缝与极点

二维准备从权威球面 cell 的三角扇开始，而不是把整个 cell 粗暴解释成一个平面多边形：

1. 以权威 cell centroid 和相邻 canonical boundary vertex 形成带 `CellId` 的球面三角形；
2. 在相对于中央经线的角坐标中展开三角形；
3. 只对穿越反经线的三角形做球面弧交点求解与裁剪；
4. 在接缝两侧生成独立显示片段并复制显示顶点；
5. 每个片段继续携带原始 `CellId`，边片段继续携带原始 `EdgeId`；
6. 投影后再做有界三角化和有限性验证。

允许一个权威 cell 或 edge 对应多个显示 primitive；不允许产生新的语义 ID。极点使用规范经度和球面裁剪规则，不能用删除极区、夹断纬度或复制“极点 cell”来掩盖奇点。

验收必须证明：

- 所有权威 cell 在投影网格中至少有一个非退化片段；
- 显示 primitive 中的 `CellId` 集合与权威集合完全相等；
- 非接缝 cell 不被无故拆分；
- 接缝两侧点击返回同一个权威 ID；
- 极点附近没有漏填、跨屏长三角形或非有限坐标。

### 6.3 二维交互与拾取

- 平移和缩放沿用二维画布语义，并为每个投影保留独立相机状态。
- 点击先将屏幕位置变换为投影规范坐标，再执行投影逆算得到单位球方向。
- 单元定位由共享 `SphericalEntityLocator` 依据权威 Voronoi site 决定；边界平局按最低 `CellId` 稳定处理。
- 边叠加激活时，在命中 cell 的有限 incident edges 中按球面弧距离选择最近边。命中阈值固定为 `8` 个 egui logical pixels，再通过当前投影和缩放的局部逆尺度换算为有界角距离；平局按最低 `EdgeId`。
- 地图轮廓外、接缝空白或无邻近边时返回 `None`，不钳制到最近实体。

首版只要求点击选择，不做每帧全分辨率 hover 查询。

## 7. 三维球体

### 7.1 不变形硬约束

`PreparedGlobeMesh` 的所有表面位置都来自权威 `UnitVector3`，并位于半径恰为 `1` 的显示球面上：

- 物理行星半径只用于元数据和自然计算，不直接作为 GPU 世界坐标；
- 高程字段只改变颜色和标注；
- 即使输入极端高程夹具，球体顶点、包围球和拾取球都必须字节不变；
- 不提供“地形夸张”隐藏开关或默认值。

### 7.2 球体网格

每个权威 cell 使用 centroid 与循环 boundary vertices 构造向外一致绕序的三角扇。显示顶点可以为每个 cell 重复，以附带该 cell 的稳定 ID；这种重复是可删除 GPU 派生数据，不成为第二地表。

网格包含：

- 单位球位置；
- 稳定 `CellId`；
- 有界 `u32` 索引；
- 用于测试的来源身份、计数和单位半径验证结果。

相机、字段、调色板和箭头不进入该静态网格。字段切换或相机旋转不能重传球体几何。

### 7.3 相机与可见性

首版使用正交轨道相机：

- 拖动执行确定性 trackball 旋转；
- 滚轮改变正交缩放；
- 重置恢复规范朝向和完整球体入框；
- 不提供自由飞行、roll 编辑或透视参数。

球体是凸单位面。填充使用一致绕序和背面剔除；边线与箭头按视向半球裁剪，背面标注不能透过球体显示。字段颜色采用无光照色带，不用明暗乘法篡改数值颜色。球体轮廓、规范经纬网或选中轮廓可以使用独立中性色，但不编码自然数据。

### 7.4 三维拾取

点击先生成相机射线并与单位球求最近正向交点：

- 无交点返回 `None`；
- 交点归一化后交给与二维完全相同的 `SphericalEntityLocator`；
- 边叠加时使用与二维相同的 incident-edge 球面距离规则；
- 相机、缩放和屏幕分辨率不能改变最终权威 ID。

同一个单位方向经二维逆投影或三维射线得到时，必须返回完全相同的 `CellId`／`EdgeId`。

## 8. UI、状态与视图切换

### 8.1 单画布模式

中央区域只显示一个 presenter。顶部或画布工具区提供明确的“二维地图／三维球体”分段切换：

- 切换不重跑 Stage 图；
- 填色、叠加、诊断和实体选择共享；
- 二维投影／平移／缩放和三维旋转／缩放分别保留；
- 切回某视图恢复其最后状态。

二维模式额外显示投影选择、中央经线和重置地图；三维模式显示重置球体。向量叠加激活时显示播放／暂停、显示速度与 glyph 密度控制。

### 8.2 检查器

检查器根据 `SelectedSurfaceEntity` 工作：

- `Cell(CellId)`：显示填色值、可选向量分量／模长、单位、字段来源和 cell 诊断；
- `Edge(EdgeId)`：显示边类别／强度、owners 和单位；当前诊断契约没有 `EdgeId` 上下文时只显示 global／field 诊断，不能伪造 edge 归属；
- 没有选中实体：显示当前字段的图例、范围和说明。

选择一个接缝复制片段时，UI 只显示一次权威实体。检查器不读取 GPU 缓冲，也不把格式化文本写回字段。

## 9. 应用切换与旧平面兼容

### 9.1 新建球面世界

应用新增窄的球面外部输入构造边界，精确插入：

- `SphericalSpaceArtifact`；
- tectonic、geologic、climate、hydro-erosion spec Artifact；
- formation spec、rule-pack set、author constraints Artifact。

默认球面空间为半径 `6_371_000 m`、目标 `20_000` cells；它是应用默认作者参数，不是算法常量。S0C 不新增半径或分辨率编辑 UI，但持久化状态保留显式球面空间规格，为后续产品控件提供唯一入口。

预览和实际构建都只能调用同一个 `spherical_natural_foundation_graph()`，使用同一组输入、根种子和 Artifact。允许不同显示 LOD，不允许预览走另一套科学算法。

### 9.2 持久化来源标签

当前 eframe 状态不是正式世界存档，但必须避免把旧参数静默解释成球面事实。新增显式来源标签：

```text
PersistedWorldOrigin
├── LegacyPlanarV1
└── SphericalV1
```

- 全新 `TemplateApp::default()` 使用 `SphericalV1`；
- 缺少该字段的旧持久化状态反序列化为 `LegacyPlanarV1`；
- legacy 状态继续调用明确命名的旧图并显示兼容提示；
- 用户可以显式选择“用当前作者参数重新生成球面世界”，产生新的球面来源身份；
- 不提供可来回切换平面／球面的普通模式选择器；
- legacy 路径不会出现在球面新世界创建服务中。

### 9.3 旧 presenter 生命周期

现有 `PreparedCellMesh`、`CellFieldRenderer` 和 legacy 文档在 S0C 保留，仅服务 `LegacyPlanarV1`。新球面 presenter 不通过扩大 `PreparedCellMesh` 为任意二维／三维万能类型来复用旧路径。

球面端到端、兼容和存储来源标签全部通过后，再由独立后续决策决定旧 presenter 的移除时间；S0C 不删除它。

## 10. GPU 资源与更新协议

### 10.1 独立 presenter，共享语义

二维和三维分别拥有几何 buffer、pipeline 和相机 uniform，但共享已经准备的字段值、诊断、色带和格式规则。一个 WGSL 源可以提供独立的 map/globe/edge/vector entry point，并只保留一份字段取色与诊断覆盖函数。

GPU 不持有 `SphericalNaturalFieldDocument`、Artifact 或 `FieldRegistry`。

### 10.2 Revision

revision 至少分为：

- 二维 geometry；
- 三维 geometry；
- fill field；
- overlay field；
- diagnostics；
- fill palette；
- overlay palette；
- vector glyph instances。

更新矩阵：

| 操作 | 科学图 | 2D geometry | 3D geometry | 字段 buffer | glyph instances | uniform |
|---|---:|---:|---:|---:|---:|---:|
| 切换 2D/3D | 否 | 否 | 否 | 否 | 否 | 是 |
| 平移／相机／普通缩放 | 否 | 否 | 否 | 否 | 否 | 是 |
| 跨 glyph LOD 阈值 | 否 | 否 | 否 | 否 | 是 | 是 |
| 切换投影／中央经线 | 否 | 是 | 否 | 否 | 仅 2D | 是 |
| 切换填色字段 | 否 | 否 | 否 | fill | 否 | 是 |
| 切换叠加字段 | 否 | 否 | 否 | overlay | 是 | 是 |
| 动画相位推进 | 否 | 否 | 否 | 否 | 否 | 是 |
| 重新生成世界 | 是 | 是 | 是 | 全部 | 是 | 全部 |

静态帧不能增加 geometry、field、diagnostic 或 palette 上传计数。

### 10.3 跨平台图元

边线和箭头使用三角形／实例化 quad 构造受控宽度，不依赖不同平台行为不一致的宽线 primitive。所有 buffer 大小先做 `usize → u64/u32` 检查，再与显式预算及 `wgpu::Limits` 比较。

## 11. 性能与内存

S0C 的首要规模是现有约 20,000-cell 产品世界，并继续拒绝超过权威球面上限的输入。

要求：

- 世界发布时构造默认二维网格、三维网格和定位器各一次；
- 两种视图共享字段数组，不为 2D/3D 各复制 36 份 payload；
- 允许几何为附带稳定 ID 而复制显示顶点，但必须线性有界；
- 两个球面 CPU 几何缓存、定位器和初始 glyph 实例在 20k 夹具上的合计新增常驻内存不超过 `128 MiB`；
- Release 参考机上，20k 默认二维＋三维 CPU 呈现准备合计不超过 `1 s`；
- 静态帧无与 cell/edge 数量成比例的 CPU 分配或遍历；
- 相机与二维画布操作只更新 uniform；
- 动态箭头每帧只更新固定大小 uniform，实例 buffer 只在字段、世界、投影或离散 LOD 改变时上传；
- native 目标以交互式 60 FPS 为性能目标，WASM 目标以 30 FPS 为最低验收目标。交互证据统一使用 `1280×720` 画布、20k 世界、地表高程填色、盛行风中等 glyph LOD 和连续 10 秒旋转／平移；记录参考硬件、浏览器、平均值与 1% low。自动化门以无逐帧 O(n) 工作、上传计数和固定离屏夹具耗时为主，人工浏览器 smoke 补充真实帧率证据。

性能测量必须分别报告：二维 geometry、三维 geometry、定位器、字段层、glyph instances 和 GPU 上传字节，不能只给总和掩盖所有权。

## 12. 错误与恢复

结构化错误至少区分：

- 文档来源校验失败；
- 呈现派生物来源不一致；
- 不支持的 fill／overlay 域或值类型；
- payload 与 cell/edge cardinality 不一致；
- 投影输入非有限、逆算不收敛或坐标在轮廓外；
- 接缝裁剪产生退化／非有限 primitive；
- 球体顶点偏离单位半径或绕序错误；
- 射线无球面交点；
- 顶点、索引、实例、picker 或 GPU buffer 超预算；
- revision 或整数转换溢出；
- GPU 资源缺失或上传失败。

恢复规则：

- 世界候选失败：保留上一份完整世界和两个 presenter；
- 投影候选失败：保留上一份有效二维网格和投影选择；
- 字段／叠加准备失败：保留上一份字段层；
- GPU 更新失败：保留上一次成功 GPU 状态并在 UI 显示错误；
- 不自动放宽验证阈值，不删除极点／接缝单元，不改用平面生成。

## 13. 测试与验收矩阵

### 13.1 投影数学

- Equal Earth 正算对照作者发布的球面参考值；
- 两个投影在全局经纬采样、接缝邻域和极点邻域完成正反算 round-trip；
- Equal Earth 逆算有界收敛，轮廓外坐标明确拒绝；
- 中央经线平移保持单位方向与稳定 ID；
- 局部向量映射保持方向，零向量稳定。

### 13.2 二维几何与接缝

- 每个权威 cell 恰好出现在语义 ID 集合中；
- 接缝复制只增加 display primitive，不增加语义实体；
- 极点、接缝、非接缝和中央经线夹具无漏填、长跨屏三角形或 NaN；
- 同一 edge 的分裂线段保留同一 `EdgeId`；
- 修改投影不改变任何 Artifact、字段哈希或 `BuildResultHash`。

### 13.3 三维几何与不变形

- 所有 globe vertex 的半径在严格容差内为 `1`，绕序朝外；
- cell primitive ID 集合与权威 cell 完全一致；
- 更换高程 payload、显示范围和任意其他字段时，globe geometry 字节完全不变；
- 背面 cell、边和箭头不会穿透显示；
- 相机旋转和缩放只改变 uniform。

### 13.4 共享字段与标注

- 36 个字段恰好进入一个共享目录；
- 32 个 cell fill、2 个 edge overlay 和 2 个 vector overlay 的支持矩阵完整；
- 二维和三维 presenter 持有同一个 `Arc<PreparedFieldLayers>`；CPU 字段、诊断和色带 allocation 必须指针相同，只有各自的几何与 GPU 上传资源独立；
- 同一实体、同一字段在 2D/3D 检查器中返回完全相同的权威值和格式；
- 动画只改变相位 uniform；暂停后画面稳定；
- glyph LOD 嵌套、确定且总含选中 cell。

### 13.5 拾取

- 预定义单位方向经二维逆投影和三维射线返回相同 `CellId`；
- 接缝两侧和两极返回权威 cell；
- edge 阈值、incident 限制和稳定平局规则通过；
- 地图轮廓外、球体外和无邻近边返回 `None`。

### 13.6 原子发布与缓存失效

- 文档、map、globe、locator 或字段层任一候选故障都不替换已发布包；
- projection/camera/LOD/animation 的精确失效矩阵通过；
- 新世界使全部旧显示缓存失效，且无法混装新旧来源；
- 相同成功 outcome 重建显示缓存会复用权威 Artifact，并得到确定的几何与 glyph 数据。

### 13.7 GPU、性能与工程门

- CPU 参考取色与 map/globe GPU 离屏像素在量化容差内一致；
- edge 分类／标量和 vector 静态／动画离屏夹具通过；
- 静态第二帧不增加大 buffer 上传；
- 20k Release 时间、内存和上传字节门通过；
- `cargo fmt --all -- --check`；
- `cargo check --workspace --all-targets --all-features`；
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`；
- `cargo test --workspace --all-targets --all-features`；
- `cargo test --workspace --doc`；
- `cargo check --target wasm32-unknown-unknown --all-features --lib`；
- native 与浏览器各完成一次真实交互 smoke：投影切换、球体旋转、字段／叠加切换、箭头暂停和同实体拾取。

## 14. 预期文件边界

实施计划应优先沿以下职责拆分，实际文件名可以按现有模块风格微调：

```text
src/
├── view/
│   ├── spherical_projection.rs   # Equal Earth / equirectangular 正反算
│   ├── spherical_mesh.rs         # 2D 接缝网格与 3D 单位球网格
│   ├── spherical_picking.rs      # 统一 cell/edge 定位
│   └── field_layers.rs           # fill + overlay + vector glyph 语义准备
├── gpu/
│   └── spherical/
│       ├── mod.rs
│       ├── callback.rs
│       └── renderer.rs           # 独立 2D/3D entry point 与共享取色
├── ui/
│   └── spherical.rs              # 视图、投影、叠加与动画控制
├── app/
│   └── spherical_presentation.rs # 来源组合、候选与原子发布
└── app.rs                        # 球面新建入口与 legacy 兼容编排

assets/shaders/spherical_field.wgsl
tests/spherical_projection.rs
tests/spherical_presentation_mesh.rs
tests/spherical_picking.rs
tests/spherical_presentation_integration.rs
tests/spherical_presentation_gpu.rs
tests/spherical_presentation_performance.rs
```

`PreparedCellMesh` 和当前 `gpu::field` 保留给 legacy V1。共享 helper 可以下沉，但不得让新球面路径反向依赖 legacy 类型。

## 15. 实施切片建议

1. 共享字段层、fill/overlay 状态和稳定选择类型；
2. Equal Earth／等距圆柱正反算、接缝三角化和二维拾取；
3. 不变形单位球网格、轨道相机和三维拾取；
4. 共享 GPU 取色、二维／三维 fill 与原子资源更新；
5. edge 线条、vector glyph、LOD 和纯显示动画；
6. 正式球面 app 构建／发布、单画布切换和 inspector；
7. legacy 来源标签、显式迁移入口和兼容回归；
8. GPU 金图、20k 性能、WASM、边界审计和最终端到端验收。

每个切片必须按测试驱动实现；任何生产行为先有能按预期失败的测试。

## 16. 完成定义

S0C 只有在以下条件全部满足后完成：

1. 全新应用状态只通过正式球面图创建自然世界；
2. 同一球面文档能在 Equal Earth、等距圆柱和不变形三维球体中查看；
3. 36 个字段全部能通过填色、边线、箭头或检查器表达；
4. 高程和所有其他数据都不会改变三维球体几何；
5. 2D/3D 对同一 `CellId`／`EdgeId` 和字段返回完全相同的权威值；
6. 接缝与极点不复制、遗漏或改变语义实体；
7. 投影、相机、缩放、LOD 和动画不触发科学图失效；
8. 新世界原子替换旧世界，任何失败都不混合来源或暴露半成品；
9. 旧无标签状态进入明确 `LegacyPlanarV1`，只有显式重新生成才能成为球面世界；
10. native、WASM、完整测试、严格 Clippy、GPU 抽查和 20k 性能门全部通过。
