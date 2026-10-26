# 站点气候导出设计：把终点 P4 月气候按区域取样，供下游天气发生器使用

日期：2026-10-26
状态：**R1（2026-10-27）已实现于 `feat/site-climatology-export`**；实施期偏离与 Stage 0 实测见 §12，以 §12 为准。
上位：`AGENTS.md`；P4 `2026-08-17-global-atmosphere-ocean-p4-design.md`；目标尺度优化
`2026-10-14-resolution-bounded-optimization-design.md`（本导出是 P4 月字段的新消费者，见 §8）。
下游：Voxim `Docs/Weather.md`（P1 接入 sekai 区域气候与日历）。

## 1. 背景与用户意图

用户 2026-10-26 决定：Voxim 的青岚地图（16 km 平面方形，以后会放大）**位于一颗由 sekai 生成的行星上**。
游戏里的天气由 Voxim 的确定性天气发生器产生，它的季节、降水频率、盛行风和天气系统来向
要与这颗行星在该位置的气候一致。用户还决定，世界矩形到行星的映射用"锚点 + 尺度 + 旋转"，
并且**按面积取样，不取单点**。

sekai 的产品是最终当前态（AGENTS"产品边界"），不产生逐日天气。本设计只做一件事：对已发布的终点
P4 `GlobalCirculationSnapshot`，按作者选定的区域做确定性的面积取样，加上少量有出处的统计派生，
导出为带身份的 JSON。天气序列、空间天气和游戏内表现都由 Voxim 负责。

## 2. 范围与非目标

范围：

- 作者在 UI 上选区域：锚点、旋转、尺度、覆盖范围；
- 对 12 个月做面积加权取样；
- 派生统计：湿日比例、湿日转移概率、湿日平均雨量、主来流方位、天气尺度活动指数、相对湿度；
- 能力标记；
- 导出 JSON 并附身份；
- UI 显示区域框和月表。

非目标：

- 不新增气候过程，不改变 P4 求解或发布；
- 不生成逐日天气序列；
- 不做区域内空间梯度（地图放大、出现消费者时再修订）；
- 不做海洋站点的特殊处理；
- 不进入构建图或持久缓存契约：导出依赖作者在 UI 上的选择，是对已发布产物的只读查询。

## 3. 输入（已发布产物，代码事实）

唯一气候来源是终点 P4 的 `GlobalCirculationSnapshot`（`src/world/natural/global_circulation.rs`），不使用初步气候
`SphericalPreliminaryClimateSnapshot`。使用其中 `GlobalCirculationFields` 的以下字段：

| 字段 | 类型 | 说明 |
|---|---|---|
| `monthly_air_temperature_c` | `MonthlyScalarField` | 月均近地面气温 |
| `monthly_precipitation_mm_day` | `MonthlyScalarField` | 月均降水 |
| `monthly_orographic_precipitation_mm_day` | `MonthlyScalarField` | 其中的地形雨部分 |
| `monthly_specific_humidity` | `MonthlyScalarField` | 比湿 |
| `near_surface_wind_m_s` | `MonthlyVector3Field` | 近地面风（切向三维） |
| `upper_wind_m_s` | `Option<MonthlyVector3Field>` | 上层风；只在 C2 布局（`new_c2*`）发布 |
| `vertical_wind_shear_m_s` | `Option<MonthlyVector3Field>` | 上层减下层；同上 |

另外读取：

- 快照的 `surface_ref`（身份）、`layout`（`ClimateLayerLayout` 各层的 `reference_thickness_m`、`density_kg_m3`，供 §5.2 使用）和 `capabilities`；
- 同一权威面的格面积、格中心、高程与海陆掩膜。

`upper_wind_m_s` 为 `None` 时，导出把"引导风"标为 `unavailable`，改用近地面风作为来流方向，并写明替代来源。
按 `ClimateCapabilityId` 的约定（"absence must never be inferred from a missing field"），这一判断必须显式记录，不能留给下游猜。

## 4. 区域映射与面积取样

### 4.1 映射参数（作者输入，原样写入导出）

| 参数 | 含义 |
|---|---|
| `anchor_lat_deg`、`anchor_lon_deg` | 游戏世界原点在行星上的位置 |
| `heading_deg` (`ρ`) | 从地理东到游戏 canonical +X 的旋转。约定 `ρ = 0` 时 +X = 东、+Z = 南（Voxim canonical 为 Y-up 右手系，俯视 X→Z 为顺时针；见 Voxim `Docs/Weather.md` §2.2） |
| `scale` (`s`) | 行星米 / 游戏米；1 表示 1:1 |
| `half_extent_m` | 游戏世界半边长，青岚为 8192 |

### 4.2 取样足迹

- 足迹是锚点切平面上、边长 `2·s·half_extent_m`、按 `ρ` 旋转的正方形，通过 gnomonic 投影回球面。
- 若足迹小于 P4 气候工作域的有效分辨率，把足迹扩大为以锚点为中心、边长等于该分辨率的正方形，并在导出中记录 `effective_footprint_m` 和扩大原因。
  这里的"有效分辨率"取 P4 工作域格距；实际值先用生产工作域实测，再写进本规格（§9.0），不另拍常数。
- 青岚 1:1 时边长 16 km，远小于 P4 格距，因此必然扩大。这就是"按面积取样"的含义：取一块代表气候的面积，而不是插值出一个点。

### 4.3 聚合

- 标量：对中心落在足迹内的权威面格，按格面积加权平均。另外报告足迹内的陆地比例、高程均值、最低值和最高值。
- 矢量：先把每格的三维切向矢量投影到**锚点处**的东/北单位矢量上，再按面积加权平均。导出东/北分量，旋转 `ρ` 由下游套用。
- 海陆：P0 不区分海陆格，全部参与平均，同时报告陆地比例（只取陆地格是否更合适，见 §10 开放问题 1）。
- 实现只读已发布字段和现有几何，不复制 P4 的投影或重映射公式（AGENTS SSOT）。如需"点在球面多边形内"或切平面投影，复用 `src/world/spatial/sphere_geometry.rs` 的现有助手；没有对应助手时，新增一个并供测试直接引用。

## 5. 导出字段

### 5.1 直接取样（现在可得）

逐月：气温（°C）、降水（mm/日）、地形雨（mm/日）及其占比、比湿、近地面风（东/北，m/s）、引导风（东/北，m/s，带能力标记）、
垂直风切变（东/北，m/s，带能力标记）。另外导出锚点纬度、足迹陆地比例和高程统计。

### 5.2 由现有产物派生（现在可做，须有出处）

| 量 | 方法 | 出处 | 状态 |
|---|---|---|---|
| 相对湿度 | 比湿 + 气温 + 由足迹平均高程按标准大气推出的气压；饱和水汽压用 Bolton 公式 | Bolton (1980)；U.S. Standard Atmosphere (1976) | 可做 |
| 主来流方位 | 引导风的月均方位（没有引导风时用近地面风，并标明来源） | 引导气流概念：Holton & Hakim, *An Introduction to Dynamic Meteorology* | 可做 |
| 天气尺度活动指数 | Eady 增长率 `σ = 0.31·f·|∂U/∂z| / N`。`∂U/∂z` = 切变 ÷ 两层参考中点高度差；`N² = (g/ρ̄)·Δρ/Δz` 取自 `ClimateLayerLayout` 两层密度。`|f|` 过小（近赤道）时标为 `not_applicable` | Lindzen & Farrell (1980)；Hoskins & Valdes (1990) | 可做；依赖 C2 布局 |
| 湿日比例 `f_wet` | 由月降水量估计湿日频率的经验关系 | 候选：New et al. (2002) CRU CL 2.0 处理缺测站点时使用的降水—湿日频率关系；Geng, Penning de Vries & Supit (1986) | **公式与系数待核对原文、先测后钉**（§9.0、§10 开放问题 2） |
| 湿日转移概率 `p_wd`、`p_ww` | `p_wd = 0.75·f_wet`，`p_ww = 0.25 + p_wd` | Geng et al. (1986)；该关系在 LPJ 系列天气发生器中沿用 | 可做，实施前核对原文 |
| 湿日平均雨量 | 月降水 ÷ (`f_wet` × 当月天数) | 定义 | 可做 |

### 5.3 需要新工作（本规格不做，导出中标为 `unavailable`）

| 量 | 为什么现在不可得 | 可能的来源 |
|---|---|---|
| 对流 / 大尺度降水占比 | P4 不区分降水类型；没有不稳定度诊断 | 新增简化的不稳定度诊断，或采用观测气候学（如 Dai 2001；Schumacher & Houze 2003）的经验分布。§10 开放问题 3 |
| 气温日较差 | 只有月均气温 | 需要地表能量收支的日变化；暂由 Voxim 作者给出 |
| 风速分布、风向稳定度 | 只发布月均矢量，没有方差 | 需要瞬变诊断 |
| 云量 | P6 云与地表能量尚未实现 | P6 |
| 热带气旋气候学 | `TropicalCycloneClimatologyV1` 能力存在；按导出时的实际 availability 原样记录 | 已有能力位 |
| 区域内空间梯度 | 当前无消费者 | 地图放大、Voxim P2 需要时再修订 |

## 6. Schema 与身份

### 6.1 Schema

JSON，`schema_version = 1`。顶层：

```
{
  "schema_version": 1,
  "source": { "surface_fingerprint": hex32, "global_circulation_content_hash": hex32,
              "global_circulation_schema": u16, "world_seed": u64, "sekai_revision": str },
  "region": { "anchor_lat_deg", "anchor_lon_deg", "heading_deg", "scale", "half_extent_m",
              "effective_footprint_m", "footprint_expanded": bool, "cell_count", "land_fraction",
              "elevation_m": { "mean", "min", "max" } },
  "capabilities": [ { "id": str, "availability": "available" | "evaluated_not_applicable" | "unavailable",
                      "substitute": str | null } ],
  "months": [ 12 × { "air_temperature_c", "precipitation_mm_day", "orographic_precipitation_mm_day",
                     "specific_humidity", "relative_humidity", "near_surface_wind_en_m_s": [e, n],
                     "steering_wind_en_m_s": [e, n] | null, "wind_shear_en_m_s": [e, n] | null,
                     "inflow_bearing_deg", "eady_growth_rate_per_day": num | null,
                     "wet_day_fraction", "p_wet_given_dry", "p_wet_given_wet", "mean_wet_day_amount_mm" } ],
  "methods": { "<字段>": "<方法 ID，对应 §5.2 与 §11>" }
}
```

- 所有可能缺项的量都在 `capabilities` 里有显式条目，`null` 只表示该条目已声明为非 `available`。
- 方位角约定：地理方位，北 = 0°，顺时针；`inflow_bearing_deg` 表示气流**去向**。与 Voxim 的去向角约定一致，减少一次符号翻转。

### 6.2 身份

- 导出是纯函数：`(GlobalCirculationSnapshot, 权威面几何, 区域参数) → 字节`。同一输入必须产生逐字节相同的 JSON（键序固定，浮点用 serde_json 默认格式）。
  BLAKE3 作为导出指纹显示在 UI 上，下游 DA 记录它。
- `source` 中的两个哈希直接取现有的 `SurfaceRef` 指纹和 artifact `ContentHash`，不另造身份。
- 跨平台：派生量包含超越函数，逐字节一致只在审计浮点平台上保证；测试按 AGENTS 用 `audited_float_platform()` 金丝雀处理。

### 6.3 代码落点

- 类型放在 `src/world/natural/site_climatology.rs`；
- 取样与派生的纯函数放在 `src/generators/natural/site_climatology.rs`；
- `src/app/` 只负责装配 UI 和写文件。

不新增 stage、artifact key 或缓存条目（理由见 §2）。§5.2 引入的物理常量（重力、标准大气参数、0.31 系数等）归 `src/world/`，并在注释中写明出处。

## 7. UI 接入（AGENTS 验收纪律）

- 在球面视图上点击设置锚点；侧栏输入旋转、尺度、半边长；视图上叠加足迹框（区分原始足迹与扩大后的有效足迹）。
- 新增"站点气候"面板：12 个月的表格，列出气温、降水、地形雨占比、湿日比例、来流方位和活动指数；`unavailable` 项显示为"不可得"，并给出原因。
  面板文案走字段注册表与本地化表，不在调用点硬编码。
- 面板上有"导出"按钮，用于写文件，同时显示导出指纹。
- 用户验证步骤：
  1. `cargo run --release`，生成任意世界；
  2. 在球面上点一个内陆山地；
  3. 看到足迹框，以及面板中 12 个月的数值随季节变化（北半球七月比一月暖）；
  4. 导出后在 Voxim 用 `Docs/Weather/tools/import_site_climatology.py` 导入，DA 记录的指纹与面板显示一致。

## 8. 与目标尺度优化（2026-10-14）的关系

本导出新增了对 `upper_wind_m_s`、`vertical_wind_shear_m_s`、`monthly_specific_humidity`、`monthly_orographic_precipitation_mm_day`
和层布局密度的真实消费。2026-10-14 设计的"消费者与门禁审计"在重新分类 P4 发布字段和层数时，应把本导出计入。
如果优化移除上层，导出按 §3 显式降级（引导风与 Eady 标为 `unavailable`），不需要改 schema。

## 9. 门禁（先测后钉）

### 9.0 Stage 0（只测量，不改产品）

- 用生产工作域实测 P4 格距，写入 §4.2。
- 在代表语料上测量足迹扩大后每个区域的格数分布，确认取样不是只落在一两格上。
- 核对 §5.2 湿日公式的原文与适用范围。若找不到可直接使用的出处，按 AGENTS 出处纪律转为开放问题，交用户裁定。

### 9.1 单元（最低成本层级）

- 面积加权：手算夹具，三格不等面积。
- 切向投影：锚点处正东方向的风应得到 `(1, 0)`；正北得到 `(0, 1)`；赤道以外的点用手算球面例子。
- 足迹扩大：足迹小于格距时得到有效足迹，并标记扩大。
- Eady：两层布局的手算值；`|f|` 低于阈值时为 `not_applicable`。
- 能力降级：C1 布局下，引导风与 Eady 为 `unavailable`，来流方位标注替代来源。
- 确定性：同一输入两次导出逐字节相同（在审计平台上）。

### 9.2 语料（Release，`#[ignore]`，离线）

只在需要验证统计性派生（`f_wet`）时运行：对地球参考气候学（CRU CL 2.0 的湿日频率与降水）做公式检验，记录数据版本与校验和。
不为导出本身建大语料。

## 10. 开放问题（交用户裁定）

1. 足迹内是否只取陆地格。青岚是内陆地图；若足迹扩大后跨到海上，全部平均会把海洋气候混进来。
2. 湿日比例公式与系数的最终出处（§5.2）。
3. 对流占比：新增简化不稳定度诊断（新工作，属 P4 范围），还是接受观测经验分布作为导出派生。在此之前导出为 `unavailable`，由 Voxim 作者给值。
4. 尺度 `s`：1:1 还是放大映射。这是 Voxim 侧的决定（Voxim `Docs/Weather.md` §7 开放问题 5），本导出只原样记录。

## 11. 每项承重技术的出处

- 面积加权聚合、切平面 gnomonic 投影：标准球面几何（Snyder, J. P. (1987), *Map Projections: A Working Manual*, USGS PP 1395）。
- 饱和水汽压：Bolton, D. (1980), *Monthly Weather Review* 108(7)。
- 标准大气气压—高度关系：U.S. Standard Atmosphere (1976)。
- Eady 增长率：Lindzen, R. S. & Farrell, B. (1980), *Journal of the Atmospheric Sciences* 37(7)；Hoskins, B. J. & Valdes, P. J. (1990), *Journal of the Atmospheric Sciences* 47(15)。
- 湿日转移概率与湿日频率：Geng, S., Penning de Vries, F. W. T. & Supit, I. (1986), *Agricultural and Forest Meteorology* 36(4)；New, M., Lister, D., Hulme, M. & Makin, I. (2002), *Climate Research* 21(1)。（实施前逐条核对原文，见 §9.0。）
- 对流降水占比的候选观测依据：Dai, A. (2001), *Journal of Climate* 14(6)；Schumacher, C. & Houze, R. A. (2003), *Journal of Climate* 16(11)。
- 引导气流：Holton, J. R. & Hakim, G. J., *An Introduction to Dynamic Meteorology*（第 5 版，2013）。

## 12. 修订 R1（2026-10-27，实施记录）

以下条目取代正文中与之冲突的部分。

### 12.1 Stage 0 实测（§9.0）

- P4 格距：取 `4πR²/(6N²)` 的平方根（`N` 为生产 `climate_face_resolution`，无新常数）。
  R = 6371 km 时 Draft（N = 24）384.2 km，Standard（32）288.1 km，High（48）192.1 km。
- Draft / seed 42（`causal_formation_fixture`）、青岚映射（1:1、半边长 8192 m）：4916 个陆地锚点，
  扩大后的足迹每个含 7–9 个权威格（最小 7，中位数 7，最大 9），不是只落在一两格上。
- 锚点推荐在该世界上 0.05 s（Release）。前五名均满足条件；第一名 35.4°N，一月 −5.2 °C、七月 10.1 °C。
- 探针：`tests/site_climatology.rs::stage0_measures_draft_footprints_and_recommendations`（`#[ignore]`，Release）。

### 12.2 湿日统计：显式不可得（§5.2）

- 湿日阈值已核对原文：New et al. (2002) §2.3 把湿日定义为当日降水 **> 0.1 mm**，1.0 mm 阈值的站点换算到 0.1 mm。
  导出 `wet_day_threshold_mm = WET_DAY_THRESHOLD_MM`（0.1），Voxim 按同一阈值判湿天气日。
- 由月降水推湿日频率：New et al. (2002) 是对观测湿日频率直接插值，没有给出可复用的"降水 → 湿日"关系。
  Geng et al. (1986) 的 `p_wd = 0.75 f`、`p_ww = 0.25 + p_wd` 本次也未能核对原文。
  按出处纪律，`wet_day_statistics` 标为 `unavailable`（原因 `no_verified_wet_day_relationship`），
  `wet_day_fraction`、`p_wet_given_dry`、`p_wet_given_wet`、`mean_wet_day_amount_mm` 全为 `null`。
  这一项转为 §10 开放问题 2，交用户裁定。在此之前，Voxim 按 `Docs/Weather.md` §4.4 使用作者值。

### 12.3 相对湿度改用 P4 自身的饱和定义（§5.2）

正文原写"标准大气气压 + Bolton"。实现改为逐格计算 `q / saturation_specific_humidity_kg_kg(T)` 后按面积平均，
直接复用 P4 在下层参考气压下的 Bolton 饱和比湿。P4 的比湿本身就是相对这一定义发布的，
另用一套标准大气气压会造出第二个饱和事实源（SSOT）。

### 12.4 Eady 增长率的 N 与赤道处理（§5.2）

- 正文的 `N² = (g/ρ̄)·Δρ/Δz` 在现有布局下恒为零：两层大气用同一个 `P4_REFERENCE_AIR_DENSITY_KG_M3`，模型里没有密度分层。
  实现改用干 Brunt–Väisälä 频率 `N² = (g/T)(g/c_p − Γ)`（Holton & Hakim 2013, §2.7.3）。
  其中 `T` 为足迹月均近地面气温，`Γ = CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M`（U.S. Standard Atmosphere 1976，P4 强迫已在用），
  `c_p = P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K`。方法 ID：`lindzen_farrell_1980_dry_standard_lapse_v1`。
- `σ = 0.31 |f| |∂U/∂z| / N` 随 `f` 线性趋零、处处有限，不需要"`|f|` 过小"的阈值。
  因此不新增阈值，也不使用 `not_applicable`。准地转假设在热带失效，下游应把低纬的小值读作"无天气尺度斜压活动"，而不是精确数值。
- `Δz` 取两层参考厚度中点的高差（C2：5000 m），由 `ClimateLayerLayout` 读取。

### 12.5 Schema 调整（§6.1）

- `source`：
  - `global_circulation_content_hash` 改为 `global_circulation_fingerprint`（`GlobalCirculationFields::fingerprint`，P4 字段状态身份），
    同时记录 `build_result_hash` 与 `graph_contract_version`。应用层拿不到 bundle 内气候的单独 artifact `ContentHash`，这三项都是现成身份，没有另造；
  - `sekai_revision` 改为 `sekai_package_version`；
  - 新增 `climate_profile` 与 `quality_profile`，均为枚举的 snake_case 名。
- `region`：
  - 锚点改为一个权威格的质心，UI 点选单元，记录 `anchor_cell`；
  - 新增 `requested_footprint_side_m`、`climate_grid_spacing_m`、`lake_fraction`；
  - `effective_footprint_m` 改名为 `effective_footprint_side_m`；
  - 朝向约定写死为"+X 自地理东**顺时针**（俯视）旋转"，与 Voxim canonical X→Z 顺时针一致。
- 新增 `anchor_assessment`：离岸距离、内陆门槛、陆地、内陆、足迹纯陆地、湖格数、足迹起伏、是否满足全部条件。
  它对应 Voxim `Docs/Weather.md` §7 第 4 项"选定理由写入导出"，并对任何锚点都记录，不只推荐锚点。
- `months`：
  - `inflow_bearing_deg` 改名为 `steering_flow_toward_bearing_deg`，明确是**去向**；
  - 新增 `specific_humidity_kg_kg`、`month`；
  - 不导出地形雨占比，由下游从两项降水相除得到，避免第二份事实。
- `capabilities`：每项为 `{id, availability, reason, substitute}`。`reason` 是稳定原因码，UI 经本地化表显示中文；
  共 10 项（引导风、风切变、Eady、相对湿度、湿日统计、对流占比、日较差、风变率、云量、热带气旋）。
- 导出指纹是文件字节的纯 BLAKE3（不加域前缀），`b3sum` 可复核。

### 12.6 锚点推荐（Voxim §7 第 4 项）

- 条件：锚点为陆地；离最近的"沿岸海洋格"质心 ≥ `ANCHOR_INLAND_MIN_CLIMATE_SPACINGS`（2）个 P4 格距；足迹内全是陆地（无海、无湖）。
- 按足迹高程起伏降序排名，相邻候选至少相隔一个有效足迹边长，取前 `ANCHOR_RECOMMENDATION_COUNT`（5）个。
- 先用离岸跳数乘最大邻格距离作上界做预筛，再算精确球面距离。

### 12.7 代码落点与 UI（§6.3、§7）

- 类型：`src/world/natural/site_climatology.rs`。
- 取样、派生与推荐：`src/generators/natural/site_climatology.rs`。
- 纬经度与 gnomonic 正反投影：`src/world/spatial/sphere_geometry.rs`，为唯一实现，地图投影 `forward` 改为复用。
- 面板：`src/app/site_climatology.rs`。

用户验证步骤：

1. `cargo run --release`，形成链生成任意世界。
2. 在左侧"站点气候"中：
   - 点"推荐锚点"→"使用"，或在地图/球体上点一个单元后点"以选中单元为锚点"；
   - 画布上出现黄色实线方框（实际取样）和白色虚线小方框（青岚 16 km 请求，已扩大）；
   - 面板显示 12 个月的数值，北半球七月比一月暖；湿日比例显示"—"，上方注明"湿日统计：不可得"及原因。
3. 点"导出"，在 `site-climatology/` 下得到 JSON；面板显示的指纹等于该文件的 `b3sum`。

### 12.8 开放问题更新（§10）

- 问题 1（只取陆地格）：推荐锚点要求足迹纯陆地，问题 1 对推荐锚点不再出现。手选锚点照常全部平均，并在 `land_fraction` 与评估中如实记录。
- 问题 2（湿日公式出处）：见 §12.2，仍待裁定。候选方向：找到 Geng et al. (1986) 原文核对；或用地球 CRU CL 2.0 网格数据自行拟合关系，记录数据版本与校验和后先测后钉。
- 问题 3、4：不变。
