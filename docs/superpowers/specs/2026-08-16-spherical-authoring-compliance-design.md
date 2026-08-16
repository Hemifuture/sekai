# Sekai 球面作者参数与图层依从性设计

状态：已批准

日期：2026-08-16

范围：修正球面正式应用的图层可见性、初始大陆壳语义、目标陆地面积、实际面积反馈与参数验收；不改变程序化板块构造、单位球几何、单一当前态或 LegacyPlanarV1。

## 1. 决策摘要

1. `TectonicSpec::continental_crust_fraction` 明确更名为产品文案“初始大陆地壳比例”。它继续只约束初始相干大陆性场，不承诺演化后的大陆壳面积，更不等同于陆地。
2. 新增正交的 `ReliefSpec::target_land_fraction`，产品文案为“目标陆地面积比例”。它只在构造高度图完成后，以球面单元真实面积加权的高程分位数选择海平面；不修改高度、板块、地壳类型、海岸连通块或球体顶点。
3. 正式文档在一次构建时缓存面积摘要：解析后的初始大陆壳请求、演化后大陆壳面积、目标陆地面积、实际陆地面积与选定海平面。UI 每帧只读常数大小摘要，禁止重新扫描单元。
4. 左侧恢复正式的“显示图层”区域，包含“填色”“叠加”“诊断”三个独立复选框。字段选择仍由一个填色槽位和一个可选边／向量槽位承担，不恢复旧 Voronoi、Delaunay 和点调试渲染。
5. 填色与叠加可见性是 renderer-neutral 持久化显示状态，只更新固定大小帧 uniform；不得重建世界、字段层、几何、glyph 或上传大缓冲。
6. 默认和具名 formation 选择会可见地把“初始大陆地壳比例”和“目标陆地面积比例”同时填入对应推荐值；选择 Random 保留作者已填写的两个数值。

## 2. 参数契约

### 2.1 初始大陆地壳比例

`TectonicSpec` schema V1 保持兼容；字段名不变，Rust 文档与 UI 文案改为“初始大陆地壳比例”。初始状态继续使用相干球面噪声排序和面积分位数，在一个权威单元面积误差内命中请求。

构造演化允许碰撞压缩、地块转移、裂谷伸展和洋壳扩张改变最终表面覆盖。最终大陆壳不得被事后阈值或海岸算法强行改回输入比例。验收改为：

- 初始面积命中请求；
- 在相同 seed/formation/activity 下，提高初始比例不得降低最终大陆壳面积；
- 支持范围内的最终大陆壳必须同时保留大陆壳和洋壳，并满足冻结的有界保有率；
- UI 明确同时显示请求值和演化后实际值。

### 2.2 目标陆地面积比例

新增：

```rust
pub struct ReliefSpec {
    pub schema_version: u16,
    pub target_land_fraction: f32,
}
```

V1 支持范围为 `0.05..=0.75`，默认 `0.38`。反序列化严格验证有限值、范围和 schema；旧存档缺少该字段时由 `TemplateApp` 的字段级默认迁移为 V1 默认，不回退整个应用状态。

`ReliefSpecArtifact` 是球面自然图的外部输入；`SphericalReliefStage` 是唯一消费者。LegacyPlanarV1 图不声明、不读取该 Artifact，因此旧输出与 hashes 保持冻结。

### 2.3 面积加权海平面

输入为完整构造高程、权威球面 cell 面积和目标陆地比例。实现执行一次稳定排序：

1. 按高程降序排列；相同高程按 `CellId` 升序稳定打破计算顺序；
2. 累加真实球面面积，选择使累计陆地面积最接近目标值的相邻前缀；
3. 在入选最低高程与未入选最高高程之间选择有限海平面；
4. 使用既有 `LandOceanField::classify(elevation, sea_level)` 生成正式分类；
5. 对高程平台无法被单一海平面拆分的情况，选择误差更小的一侧，并报告可审计的实际比例。

算法复杂度为 `O(N log N)`、额外内存 `O(N)`，20,252 cells 只在 world rebuild 时运行一次。它是标准 hypsometric quantile，不改变地形因果或形态。正式验收使用单元面积，不使用 cell count 或屏幕像素。

## 3. 模块与依赖

```text
world/natural/relief_spec.rs
    ReliefSpec + validation only
              ↓
generators/natural/relief_spec.rs
    ReliefSpecArtifact transport only
              ↓
generators/natural/land_fraction.rs
    pure area-weighted sea-level selection
              ↓
SphericalReliefStage → SphericalReliefSnapshot
              ↓
SphericalNaturalFieldDocument
    cached SphericalNaturalAreaSummary
              ↓
app UI read-only labels
```

约束：

- `land_fraction` 不依赖 tectonic process、UI、GPU 或 projection；
- tectonics 不读取 `ReliefSpec`；
- relief 不反写 tectonic snapshot；
- 文档摘要只从已经交叉验证的 source-bound Artifacts 构造；
- UI 不复制面积计算；
- map/globe 继续消费同一 `SphericalReliefSnapshot`。

## 4. 图层可见性

`SphericalFieldDisplayState` 增加 `fill_visible` 和 `overlay_visible`，默认均为 `true`。持久化 wire 使用 `#[serde(default)]`，旧存档保持现有外观。

新增动作：

```rust
SetFillVisible(bool)
SetOverlayVisible(bool)
```

两者只返回 presenter-uniform invalidation。回调把两个布尔值写入 `SphericalFrameUniform`：

- 填色关闭时基础填色透明，但诊断仍可独立显示；
- 叠加关闭时 edge/vector fragment 丢弃；
- 字段选择、PreparedFieldLayers、packet identity、revisions、glyph IDs 和几何 Arc 全部保持不变。

左侧控件顺序固定为：投影／相机、字段选择、“显示图层”复选框、向量动画、面积摘要、实体检查。

## 5. 面积摘要与文案

`SphericalNaturalAreaSummary` 保存五个有限值：

- `requested_initial_continental_fraction`；
- `evolved_continental_fraction`；
- `target_land_fraction`；
- `actual_land_fraction`；
- `sea_level_m`。

UI 使用一位小数百分比和带符号百分点差值：

```text
面积依从性
初始大陆壳：请求 38.0%
演化后大陆壳：24.5%
目标陆地：38.0%
实际陆地：38.0%（+0.0 pp）
海平面：-1234 m
```

摘要在文档构造时扫描一次并缓存；静态、相机、投影、动画和图层开关帧均为 `O(1)`。

## 6. 原子性与错误

- 非法 `ReliefSpec` 在 Stage 执行和 GPU 准备前拒绝；旧 publication、renderer source、revisions、clock 和 UI 已发布摘要保持不变。
- 无有效 cell、高程非有限或无法得到有限海平面返回 typed error；不得默认为 0 m、重随机 seed 或回退旧算法。
- world rebuild 成功后，新的 snapshot、面积摘要和 renderer packet 在既有 Task 9 publication 协议中一次替换。

## 7. 验收

### 7.1 参数与科学语义

- `ReliefSpec` 默认、边界、NaN、schema 和旧 TemplateApp 存档迁移；
- 不均匀 cell 面积 fixture 精确证明面积加权而非 cell-count quantile；
- 5 个 formation × 17 seeds 的实际陆地面积误差不超过 cutoff plateau 面积，并在正式连续高程 fixture 上不超过一个最大 cell 面积；
- 20,252-cell seeds `[3, 7, 11, 19, 42]` 的实际陆地与目标差不超过 `0.01`；
- 同 seed 的目标陆地 `0.20 < 0.38 < 0.55` 产生单调实际面积，而高度字段 bits 完全相同、只允许 sea level/land mask 改变；
- 同 seed 的初始大陆壳 `0.20 < 0.38 < 0.55` 产生单调演化后大陆壳面积；
- 不允许最终 land mask 与 crust-kind mask 退化为恒等。

### 7.2 UI 与 GPU

- public 两阶段 UI action 测试覆盖三个复选框、旧存档默认与持久化 roundtrip；
- 填色／叠加开关保持 exact packet/layers/map/globe Arc、所有 immutable upload counters 和 revisions；
- map/globe offscreen readback 分别证明填色、叠加和诊断独立可见；
- 左侧真实 UI smoke 能看到“显示图层”和“面积依从性”，重建后数值与权威 Artifact 相同。

### 7.3 工程门禁

- 所有 stage graph、artifact hash、source identity、atomic publication、WASM、strict Clippy、格式、required-GPU、Release 20k 性能和完整 workspace 测试通过；
- LegacyPlanarV1 hashes 不变；
- 单位球顶点不读取 elevation；
- 无历史切片、第二套海岸算法、投影物理或公开 morphology 中间态。

## 8. 完成定义

只有在正式图层开关、两个独立参数、实际面积摘要、面积加权目标、原子错误路径、多 seed/20k/GPU/UI 验收和完整工程门禁全部通过，且工作树以独立实现和验收提交保持干净时完成。本切片不留下待实现项。
