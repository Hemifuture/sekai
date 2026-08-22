# 陆地占比驱动与水线求解（T0b）设计草案

状态：草案，§8 已由用户裁定（R1）；Task 1 实测后冻结。日期：2026-08-22。
前置：`2026-08-21-t0-hypsometric-calibration-design`（T0，已交付）。

## 1. 动机

用户指令（2026-08-22）：陆地占比不是硬约束，而是用户可调的世界选项——
用户决定这个世界的陆地占多少，从而得到不同类型与样式的世界；用户只
关心最终结果，初始陆壳占比可以保留但不应与目标冲突。

现状（代码事实）：

- 形成链（`WorldPipeline::Formation`，产品默认）的海面由
  `solve_physical_sea_level` 按**地球水量**（`EARTH_OCEAN_VOLUME_M3`
  按球面面积缩放，`scaled_earth_ocean_inventory_m3`）解出；
  `ReliefSpec::target_land_fraction` 在该链上只用于 P3 的
  `LandFractionConstraintStatus` 诊断（`Satisfied` / `Infeasible`），
  对应的"目标陆地面积比例"滑块在形成链上被禁用
  （`src/app.rs`，`ui.add_enabled(pipeline == LegacyFoundation, …)`）。
- `TectonicSpec::continental_crust_fraction`（"初始大陆地壳比例"，
  `MIN_…=0.10`–`MAX_…=0.75`）是形成链上唯一可动的相关旋钮；预设把
  它与 `recommended_land_fraction` 设成**同一个数**
  （`ResolvedWorldFormationPreset`，Continents 0.38/0.38），即
  "陆壳 = 陆地"的错误等式（T0 规格 §11.4）。
- T0 校准后 17 粒语料：陆壳 0.38 → 陆地中位 0.20（地球 0.41 → 0.29），
  作者目标 0.38 在全部种子上判 `Infeasible`。

## 2. 物理事实（设计的硬边界）

### 2.1 水量恒等式

记球面平均水深 h = V_water / A_sphere。地球：
`EARTH_OCEAN_VOLUME_M3` 1.335e18 m³ / 5.10e14 m² = **2.62 km**
（Eakins & Sharman 2010：洋盆体积 1.3324e9 km³、面积 3.619e8 km²、
平均深度 3682 m）。设陆地占比 L、湿面积平均水深 D，则

    (1 − L) · D = h

| L | 所需 D | 对照 |
| ---: | ---: | --- |
| 0.20（本世界现状） | 3.27 km | 本世界湿面积均深 ≈ 3.3 km（含淹没陆架） |
| 0.29（地球） | 3.69 km | 地球实测 3.68 km |
| 0.38（Continents 标称） | 4.22 km | 比地球深 0.54 km |
| 0.42（Supercontinent 标称） | 4.51 km | — |

结论：**在地球水量下，陆地占比有上限**（洋盆按 GDH1 + 沉积盖层的深度
律，湿面积均深不会超过 ~3.9 km ⇒ L ≲ 0.33；精确上限由 Task 1 按预设
实测）。陆壳比例再高也越不过这条线——多出来的陆壳把水挤高，反过来
淹没自己。要让陆地占比成为覆盖"火山群岛 16% … 超大陆 42% … 干旱世界
60%"全谱的自由选项，**水量必须是可变量**。

### 2.2 两个量的物理含义

- `continental_crust_fraction`：球面上承载厚而轻的大陆壳的面积份额。
  决定大陆的**数量与几何**（多少块、多大、碰撞频率）。
- 陆地占比：陆壳中露出水面的份额 + 少量露出的洋壳（火山岛、洋脊）。
  由陆壳厚度分布（干舷，Wise 1974）、洋盆深度、水量三者共同决定。
- 露出率 = 陆地 / 陆壳：地球 0.71，本世界 0.53。差异的已测部分：
  初始清单（CRUST1.0 台地表均值 38.7 km）演化到终态均值 35.4 km
  （−3.3 km ⇒ Airy 151.5 m/km ⇒ 大陆整体低 ~500 m），且厚尾不足
  （≥ 44 km 份额 0.031 对 CRUST1.0 0.115）。归因（裂谷拉张预算
  `MAXIMUM_RIFT_EXTENSION_AREA_FRACTION` 面积 +15%、体积守恒；
  重采样混合；碰撞消耗）属 Task 1 实测。

### 2.3 两条杠杆的分工（设计核心）

| 杠杆 | 改变什么 | 归谁 |
| --- | --- | --- |
| 陆壳比例 | 大陆几何（样式） | **预设**携带；高级覆盖 |
| 水线（水量） | 淹没程度（陆地占比） | **用户主旋钮** |

两者正交：陆壳决定"有多少大陆材料"，水线决定"多少是干的"。它们
此前冲突，只因为两个旋钮都在试图设定同一个结果。

## 3. 设计

### 3.1 语义：`ReliefSpec` 增加海面策略

`src/world/natural/relief_spec.rs`（世界语义层，SSOT）：

```rust
pub enum SeaLevelPolicy {
    /// 海面由世界水量解出；陆地占比是结果。默认。
    WaterInventory,
    /// 海面按 `target_land_fraction` 解出；水量是结果（隐含库存）。
    TargetLandFraction,
}
```

`ReliefSpec` 增加两个字段，schema 升版：

- `sea_level_policy: SeaLevelPolicy`；
- `water_inventory_ratio: f32`——**世界表层水量**，以地球水量（按面积
  缩放，`scaled_earth_ocean_inventory_m3`）为单位的比值，默认 1.0，
  合法范围 `MIN_WATER_INVENTORY_RATIO`–`MAX_WATER_INVENTORY_RATIO`
  （世界常量，拟 0.05–5.0：下界保证存在海洋，上界远在"水世界"之外）。
  用户裁定（R1）：水量是一等世界参数，不绑定地球数值，只要求算法
  科学（体积守恒、浴缸恒等式、测高一致）；其 UI 旋钮**后期**再加，
  本里程碑只显示。`WaterInventory` 模式用它解海面；`TargetLandFraction`
  模式把解出的隐含比值写回快照推导值（§3.2），不回写 spec。

`target_land_fraction` 字段保留（范围 `MIN_TARGET_LAND_FRACTION` 0.05
–`MAX_TARGET_LAND_FRACTION` 0.75 不变）。legacy 链
（`LegacyFoundation`）没有水量解，始终按目标切海面，忽略本策略
（文档化；legacy 为维护态）。

### 3.2 P3：目标模式的海面解与隐含水量

`solve_physical_sea_level` 不动。目标解**复用既有生产助手**
`select_area_weighted_sea_level`（`src/generators/natural/land_fraction.rs`，
legacy 球面链同一实现：按 `LandOceanKind::quantized_centimeters` 排序、
不拆分等高台地、取 |实际 − 目标| 最小的海面，分类规则与产品一致），
再以 `water_volume_at_sea_level_m3` 算出该海面的**隐含水量**——不新写
第二个百分位求解器（SSOT）。离散格元下目标在一格面积内达成，即既有
`land_fraction_constraint_tolerance`。

- P3 `generate`：按 `sea_level_policy` 分派；`WaterInventory` 模式的
  库存 = `water_inventory_ratio × scaled_earth_ocean_inventory_m3(area)`；
  目标模式下把隐含水量写入
  快照既有字段 `water_inventory_m3`（`realized_water_volume_m3` 同值，
  相对误差 0），`constraint_status` 按既有 `constraint_status(requested,
  physical, tolerance)` 评定（目标模式下由构造为 `Satisfied`）。
  **快照 wire 不变**：隐含水量比 = `water_inventory_m3 /
  scaled_earth_ocean_inventory_m3(area)` 由读取方推导，不落盘。
- P5 零改动：它只消费 `inputs.relief.water_inventory_m3()` 并用浴缸解
  重解海面（`FormationSeaLevelSolver`），隐含库存天然守恒；陆比漂移
  由 T1v2 既有门禁（≤ 0.01）约束。
- 这不是直方图重映射：高程一个不动，只移动水线；与 legacy 链"按面积
  百分位切海面"的区别在于陆壳清单已按 CRUST1.0 校准、洋底已按 GDH1 +
  沉积校准，水线移动不再把年轻洋壳顶成大陆。

### 3.3 质量报告：测量而非门禁

- P3 单世界报告新增测量 `water-inventory-ratio`（隐含水量 / 地球缩放
  水量，**无界**；WaterInventory 模式恒为 1）。
- 语料门禁（17 粒，参考设定 = 预设默认 + WaterInventory 模式）不变；
  `physical-land-area-fraction` 的 0.20–0.55 带不动。
- `Infeasible` 不再是目标模式的结论；WaterInventory 模式下它仍表达
  "地球水量下未达到标称陆地"，UI 文案改为陈述事实（§3.5）。
- 露出洋壳（L 超过陆壳可露出上限）不禁止：P5 衬底把露出的洋壳当镁铁质
  基岩处理（今日火山岛/洋脊即如此），UI 给出提示（§3.5）。用户裁定
  （R1）：过程守物理，结果归玩家，系统只给建议值。
- 水量建议带 `WATER_INVENTORY_RATIO_ADVISORY_MIN/MAX`（世界常量，拟
  0.5–2.0）只用于 UI 提示，不钳制、不入门禁。

### 3.4 预设：陆壳携带几何，标称陆地按实测重钉

- `recommended_continental_crust_fraction` 各预设值**不改**（几何与
  v5 语料门禁 `continental-area-fraction` 0.30–0.45 绑定，属非目标）。
- `recommended_land_fraction` 重新定义为：该预设在 WaterInventory 模式
  下的 **17 粒语料陆地中位（实测，Task 1 钉值）**。它是 UI 在自动模式
  下显示的"预期陆地"，也是目标模式滑块的初值。
- 本里程碑**不重定任何预设的陆壳值**（用户裁定 R1）：陆地占比由水线
  精确控制后，陆壳只剩几何职责，没有必要再为露出率去动它。

### 3.5 UI（`src/app.rs` 左侧面板，形成链）

互斥驱动（用户提案，2026-08-22）：

```
世界形态  [Continents ▾]
驱动      (•) 陆壳比例（物理解：海面由地球水量决定）
          ( ) 陆地占比（海面按目标求解，海水量随之推算）
陆地占比  [====•=====] 20.2%      ← 陆壳驱动时锁定，显示上次构建实测
初始大陆地壳比例（高级）[===•===] 38%  ← 陆地占比驱动时锁定，显示预设值
```

- 默认驱动 = 陆壳比例（物理解），即今日行为；指纹与证据不变。
- 切换驱动只改 `ReliefSpec::sea_level_policy`；被锁定的滑块禁用并显示
  推算/实测值（不只是变灰）。
- "面积依从性"组：`陆地面积：目标 x%｜实际 y%｜海水量 = r × 地球`
  （r 在物理模式下即 `water_inventory_ratio`，目标模式下为隐含比值）；
  r 落在 §8.2 提示带之外时附一行提示；若目标超过"陆壳可露出上限"
  （陆地 > 演化后陆壳面积 − 淹没陆架下限），提示"将露出洋底"。
  `FormationAreaSummary` 增加 `water_inventory_ratio`（由快照推导）。
- legacy 链：滑块行为不变（始终目标驱动）。

### 3.6 指纹与证据

- 默认（WaterInventory）模式下 P3/P5 产物必须**逐位不变**：以 T0 刷新
  后的 P3 证据哈希（`ea5ef259…` / `b166ee75…`）与 P5 seed 42 工件
  `83a67fc6…` 为守门值（集成测试断言）。
- `ReliefSpec` schema 升版改变其 wire，凡把该 spec 纳入身份的缓存键/
  指纹会变；Task 5 实测哪些冻结值实际改变（原则：只刷新确实改变的，
  输出哈希不变者不动），清单写入本规格修订条目。T0 Task 4 的教训：
  `src/app/spherical_natural_display.rs` 的 `EXPECTED_FIELD_HASH` 与
  GPU 金样的 `EXPECTED_SAMPLED_IDS` 也在链上。
- 目标模式新增证据：Continents 预设、seed 42、目标 0.38 的 P3/P5 产物
  哈希与隐含水量比入 P3/P5 完成记录修订条目。

## 4. 非目标

- 不动陆壳几何：预设陆壳值、v5 板块运动学、裂谷/碰撞预算（§8.3 例外
  须显式修订）。
- 不修 T0 开放项"陆壳清单离散度不足"；本里程碑只**归因**（Task 1），
  修复另案。
- 不动 P5 方程、T1/T1v2、色带、河流。
- 不做直方图重映射（§3.2 论证）。
- 不改 legacy 链语义。
- 水量旋钮（直接拨 `water_inventory_ratio`）延后：参数与显示本里程碑
  落地，交互后期加（用户裁定 R1）。

## 5. 科学依据

- 海面由水量与测高曲线共同决定、陆地占比是二者的函数：Cowan & Abbot,
  ApJ 2014（类地行星露出陆地份额 = 测高积分在水量处的取值）；
  Eakins & Sharman 2010（ETOPO1 洋盆体积/面积/均深）。
- 行星水储量的可变性：Hirschmann, Annu. Rev. Earth Planet. Sci. 2006
  （地幔含水可达数个海洋）；Cowan & Abbot 2014（表层水量可在数量级内
  变化而不成水世界）。因此把水量当世界参数是正当的。
- 干舷与露出率：Wise 1974（大陆干舷恒定）；CRUST1.0（Laske et al. 2013）
  全部陆壳 36.1 ± 8.4 km 与稳定台地分位表。
- 测高目标与包络：T0 规格 §3（ETOPO1）。

## 6. 门禁与测试

- 单元：P3 目标模式在合成地形上达成目标（误差 ≤ 一格面积份额，
  `select_area_weighted_sea_level` 既有测试覆盖选择本身）、隐含水量与
  正解互逆（`solve_physical_sea_level(隐含) == 同一海面`）、
  `SeaLevelPolicy` 序列化/校验。
- 集成（release）：P3 目标模式 seed 42 目标 0.38 → `constraint_status
  == Satisfied`、P5 产物陆比漂移 ≤ 0.01（复用 T1v2 门禁助手）；默认
  模式逐位不变（§3.6）。
- 应用：`ReliefSpec` 持久化往返含新字段；驱动切换的锁定语义；摘要显示
  水量比。
- 门禁：fmt / clippy `-D warnings` / wasm；全量套件两档
  `--no-fail-fast`、分离进程启动（T0 Task 5 教训）。

## 7. 用户验收（交付时附）

1. `cargo run --release`；Continents、seed 42、草稿档，默认驱动重建：
   画面与 T0 交付一致（陆地 ≈ 20%）。
2. 驱动切到"陆地占比"，滑块拨到 38%，重建：海岸线外推、陆架露出；
   摘要显示 `海水量 ≈ 0.9 × 地球`（Task 1 实测后填精确值）。
3. 拨到 60%：提示"将露出洋底"，深海平原成为低地；拨到 10%：内陆海
   淹没，仅高地露出。
4. 切回陆壳驱动：回到 1 的画面，陆地滑块锁定并显示实测值。

## 8. 开放问题（用户裁定，2026-08-22）

1. **默认驱动**：陆壳比例（物理解，今日行为，指纹不动）。——**裁定：是。**
2. **水量**：建议带 0.5–2.0 × 地球仅提示、不钳制。——**裁定：水量后期
   要做成用户可调，不必按地球数据，只要算法科学。** 落实为 §3.1 的
   一等参数 `water_inventory_ratio` + 建议带常量。
3. **预设陆壳值是否重定**：原拟按实测露出率定规则。——**裁定：不动**
   （用户未采纳该规则；陆地由水线控制后无必要）。
4. **露出洋壳**：允许 + 提示。——**裁定：可以；过程守物理，结果归玩家，
   可给建议值。**
5. **标称陆地的来源**：改为实测中位。——**裁定：同意。**

Task 1 实测后待钉的数值：各预设 `recommended_land_fraction`（实测中位）、
`MIN/MAX_WATER_INVENTORY_RATIO`、建议带常量、§2.1 的陆地上限表。

## 9. 修订记录

- R0（2026-08-22）：草案。
- R1（2026-08-22）：用户裁定 §8 五项；水量升格为一等世界参数
  `water_inventory_ratio`（§3.1），预设陆壳值明确不动（§3.4），
  建议带只提示（§3.3），水量旋钮延后（§4）。
