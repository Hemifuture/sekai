# Sekai 初步气候基底设计

**状态：** 自审通过；依据用户已授权的例行设计确认，进入实施计划  
**日期：** 2026-07-29  
**上位设计：** `docs/superpowers/specs/2026-07-28-sekai-current-slice-world-design.md`  
**直接上游：** `docs/superpowers/specs/2026-07-29-geologic-substrate-design.md`

## 1. 摘要

本切片在正式空间、高程与海陆产物之后生成水文和侵蚀所需的**初步月度气候强迫**：

```text
规则包 + ClimateSpec
  → 已解析气候模型输入
  → 低分辨率月度能量、风与水汽输运
  → 投影到正式空间单元
  → 初步温度、降水、风、纬度与海洋性字段
  → 只读字段显示
```

它生成形成当前世界状态所需的气候输入，不模拟逐日天气，不生成气候历史，也不把尚未经过水文、侵蚀和冰雪反馈的结果伪装成最终气候。

本切片解决四个具体问题：

1. 水文阶段获得带单位的 12 个月降水输入；
2. 后续侵蚀获得与纬度、海拔、海洋性、风和地形抬升一致的水量强迫；
3. 月度温度和风拥有稳定、可验证的正式契约；
4. 气候参数通过规则投影进入负责阶段，不由 UI、旧 `terrain` 模块或渲染器直接改写。

## 2. 已确认的因果位置

上位设计已经确认：

```text
空间
  → 板块/地壳
  → 地幔强迫
  → 地貌
  → 地质基底
  → 初步气候
  → 水文、侵蚀与沉积
  → 最终气候、积雪与冰川
```

因此本切片不需要新的产品方向选择。它只实现已经批准的“初步气候”位置，并保留下游替换边界。

NASA 对地球能量收支的说明指出，太阳能输入随纬度和季节显著变化，轴倾使两个半球交替获得更直接、持续时间更长的日照。[NASA: Climate and Earth’s Energy Budget](https://science.nasa.gov/earth/earth-observatory/climate-and-earths-energy-budget/)

NOAA 对全球环流的说明给出低纬信风、中纬西风和高纬极地东风的稳定大尺度结构；这些结构适合作为世界生成器的低分辨率盛行风基线，而不是逐日天气预报。[NOAA: Prevailing Winds](https://www.weather.gov/source/zhu/ZHU_Training_Page/winds/Wx_Terms/Flight_Environment.htm)

NOAA 资料明确说明迎风坡抬升增强降水、背风坡形成雨影，且海洋邻近性影响区域气候。[NOAA educational climate reference](https://repository.library.noaa.gov/view/noaa/37707/noaa_37707_DS2.pdf)

公开气象资料常用约 `6.5 °C/km` 的自由大气平均递减率作为区域温度估计基线，同时也强调近地表实际值会因地形、湿度和环流变化。本模型把它作为有界近似，不声称复原真实地区。[NOAA repository: temperature gradient with elevation](https://repository.library.noaa.gov/view/noaa/52894/noaa_52894_DS1.pdf)

## 3. 方案比较

### 3.1 方案 A：直接按纬度贴温度和随机降水

优点：

- 实现很快；
- 字段容易显示。

缺点：

- 降水没有水汽来源、风向、迎风坡或雨影因果；
- 水文会消费装饰噪声；
- 后续侵蚀无法解释。

**结论：不采用。**

### 3.2 方案 B：在全部 Voronoi 单元上运行完整大气环流

优点：

- 可以保留更细的空间变化；
- 理论上可继续扩展复杂流体模型。

缺点：

- 默认 20,000、最大 200,000 单元上的 12 月迭代成本过高；
- 容易把天气数值模拟误当作气候生成；
- 与第一阶段的当前切片目标不成比例。

**结论：不采用。**

### 3.3 方案 C：有界低分辨率月度气候网格，再投影到正式单元

做法：

- 从正式空间、海陆和高程聚合一个内部气候网格；
- 在该网格上计算月度日照代理、盛行风和有界水汽输运；
- 通过地形抬升消耗水汽并形成迎风降水和背风雨影；
- 将结果插值回每个正式空间单元，并应用单元级海拔修正；
- 正式快照保存逐月值和年度摘要。

优点：

- 计算预算与世界单元数量解耦；
- 气候算法不依赖 Voronoi、egui 或 GPU；
- 下游仍获得与 `CellId` 对齐的密集字段；
- 低分辨率尺度与“气候而非天气”的语义一致；
- 可以独立替换内部模型而不改变水文输入契约。

成本：

- 需要明确聚合、空格填充和投影规则；
- 当前不表达洋流和局地天气。

**结论：采用。**

## 4. 范围

### 4.1 本切片包含

- `ClimateSpec` 及安全预算；
- 唯一世界法则能力 `sekai.core.natural.climate-model@1`；
- 完整规则解析审计与最小气候输入投影；
- 12 个月温度、降水和二维盛行风；
- 每单元纬度、海洋性、年均温、温度季节性、年降水和年平均盛行风；
- 正式 `PreliminaryClimateArtifact` 与阶段图、缓存、诊断；
- 正式字段注册、应用文档和现有标量显示；
- 契约、性质、缓存、质量、金图、性能、native/WASM 和桌面验收。

### 4.2 明确排除

- 逐日天气、风暴、洪水年份或气候历史；
- 洋流、海冰、积雪和冰川；
- 河流、湖泊、地下水、侵蚀和沉积；
- 最终气候和水文反馈；
- 土壤、生态、生物群落和农业；
- 魔法气候强迫；
- 月份选择 UI、气候参数编辑 UI 和向量箭头渲染；
- 旧 `terrain::hydrology` 或其他旧地形原型作为正式输入。

这些能力必须由真实下游契约驱动，不在本切片预建空接口。

## 5. 模块与职责

```text
src/world/natural/climate_spec.rs
  只拥有气候规格、固定点参数和验证

src/rules/climate.rs
  只拥有规则解析审计

src/generators/natural/climate_rule_input.rs
  只拥有引擎传输、规则阶段和最小输入投影

src/world/natural/climate.rs
  只拥有初步气候快照、月度字段和不变量

src/generators/natural/climate.rs
  只拥有纯低分辨率气候生成算法

src/generators/natural/climate_stage.rs
  只拥有正式产物、阶段依赖和错误映射

src/world/natural/fields.rs
  只注册字段 schema

src/app/natural_display.rs
  只做正式产物验证与零拷贝显示适配
```

禁止的依赖：

- `world` 不依赖 `engine`、`rules`、`generators`、`app` 或 `ui`；
- `rules` 不依赖生成器；
- 气候生成器不读取 UI、GPU、旧 `terrain`、魔法或社会模块；
- 显示模块不执行气候计算；
- 水文尚未实现，因此气候阶段不引用未来水文类型。

## 6. 气候规格

`ClimateSpec` 使用固定点整数，避免规则哈希、跨平台序列化和边界比较依赖浮点文本：

```rust
pub struct ClimateSpec {
    pub schema_version: u16,
    pub south_latitude_centideg: i16,
    pub north_latitude_centideg: i16,
    pub axial_tilt_centideg: u16,
    pub temperature_offset_deci_c: i16,
    pub moisture_scale_permille: u16,
}
```

默认值：

| 参数 | 默认 | V1 范围 | 语义 |
|---|---:|---:|---|
| 南边纬度 | -70.00° | -90.00°..89.00° | 地图下边界纬度 |
| 北边纬度 | 70.00° | -89.00°..90.00° | 地图上边界纬度 |
| 纬度跨度 | 140.00° | 10.00°..180.00° | 必须严格向北递增 |
| 轴倾 | 23.40° | 0°..60.00° | 控制季节日照差 |
| 温度偏移 | 0.0 °C | -30.0..30.0 °C | 受信任模型的全局热偏移 |
| 水分倍率 | 1000‰ | 250..2500‰ | 月度水汽源强度 |

经纬度映射是显式气候输入，不从世界米制高度偷偷推导。将来若引入天体与投影产物，只需让规则投影产生同一个最小输入，气候快照契约不变。

## 7. 规则能力

新增：

```text
sekai.core.natural.climate-model@1
cardinality = UniqueRequired
minimum_pack_kind = WorldLaw
author_allowed = false
```

封闭模型枚举：

```rust
pub enum ClimateModel {
    SeasonalEnergyMoistureV1,
}
```

内置 `sekai.builtin.earthlike` 同时贡献构造、地质和气候三个唯一模型。普通规则包不能替换气候模型，作者也不能绕过规格验证直接写气候字段。

规则路径：

```text
ClimateSpecArtifact + RulePackSetArtifact
  → RuleClimateResolutionStage
  → ClimateRuleResolutionArtifact
  → ResolvedClimateInputStage
  → ResolvedClimateInputArtifact
```

完整审计包含参与规则包、模型和最终规格；气候生成器只读取无审计噪声的模型与规格。

## 8. 正式快照

### 8.1 月度布局

```rust
pub const CLIMATE_MONTH_COUNT: usize = 12;

pub struct MonthlyScalarField(Vec<[f32; CLIMATE_MONTH_COUNT]>);
pub struct MonthlyVectorField(Vec<[[f32; 2]; CLIMATE_MONTH_COUNT]>);

pub struct PreliminaryClimateSnapshot {
    schema_version: u16,
    cell_count: u32,
    latitude_degrees: Vec<f32>,
    maritime_influence: Vec<f32>,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_precipitation_mm: MonthlyScalarField,
    monthly_wind_m_s: MonthlyVectorField,
    mean_annual_air_temperature_c: Vec<f32>,
    temperature_seasonality_c: Vec<f32>,
    annual_precipitation_mm: Vec<f32>,
    prevailing_wind_m_s: Vec<[f32; 2]>,
}
```

布局按单元存一组 12 个月值，便于水文按 `CellId` 连续读取。年度摘要被正式存储，以便字段显示零拷贝借用；验证器保证摘要与月度值恒等。

### 8.2 数值范围

| 字段 | 范围 | 单位 |
|---|---:|---|
| 纬度 | -90..90 | degree |
| 海洋性 | 0..1 | unitless |
| 月温/年均温 | -100..70 | °C |
| 月降水 | 0..4000 | mm/month |
| 年降水 | 0..20000 | mm/year |
| 温度季节性 | 0..120 | °C peak-to-trough |
| 月风/年平均风分量 | -80..80 | m/s |

所有值有限；所有数组长度等于 `cell_count`；反序列化后重新验证。

### 8.3 恒等式

对每个单元：

- 年均温等于 12 个月温度算术平均；
- 温度季节性等于月最高温减月最低温；
- 年降水等于 12 个月降水之和；
- 年平均盛行风等于 12 个月风向量算术平均。

使用固定容差处理 `f32` 累加，不允许任意漂移。

## 9. 生成算法

### 9.1 内部气候网格

- 网格只存在于生成器内部，不成为世界真值；
- 根据世界长宽比和正式单元数量选择分辨率；
- 网格单元总数最少 16、最多 4096；
- 分辨率不超过正式单元数量所能支持的尺度；
- 聚合按正式空间单元面积加权；
- 每格保存陆地比例、平均高程和代表性高程；
- 空格通过稳定四邻域波前从最近已占用格填充；
- 遍历和并列规则固定，禁止哈希表迭代决定结果。

### 9.2 纬度与月度能量

- 单元 `y` 在空间 bounds 内线性映射到规格的南北纬度；
- 每个月使用月中相位；
- 太阳赤纬由轴倾和固定年相位计算；
- 使用有界日平均日照几何代理处理极昼和极夜；
- 不保存年份、日期或历法事件。

温度由以下可解释分量组成：

```text
纬度年平均基线
  + 月度日照异常
  × 陆地/海洋热惯性
  + 全局温度偏移
  - 海拔递减修正
```

海洋和高海洋性陆地的季节振幅更小；高地使用有界 `6.5 °C/km` 近似修正。

### 9.3 盛行风

- 环流带随月度太阳赤纬小幅南北移动；
- 低纬使用向西的信风基线；
- 中纬使用向东的西风基线；
- 高纬使用向西的极地东风基线；
- 带间使用平滑过渡；
- 子午向分量指向相应的大尺度辐合/辐散带；
- 风速为气候盛行风，不是某一时刻的阵风。

### 9.4 海洋性

- 海洋格海洋性为 1；
- 陆地格从最近海洋格的世界距离得到指数衰减；
- 全海洋安全返回 1，全陆地安全返回 0；
- 该字段只表达海洋热量与水汽调节程度，不是地下水或土壤湿度。

### 9.5 有界水汽输运

每个月在内部网格运行固定上限的同步松弛：

1. 海洋按温度和水分倍率提供水汽；
2. 背景水汽保证封闭内陆边界不会产生数值真空；
3. 每格从盛行风上游位置双线性采样水汽；
4. 传输有固定衰减；
5. 赤道辐合和暖季对流提供有界凝结；
6. 上游到本格的正高程差增加迎风凝结；
7. 已凝结水分从继续传输的水汽中扣除，因此下风坡自然变干；
8. 固定迭代上限和固定遍历保证时间、内存与结果确定。

最后将归一化凝结率映射到 `mm/month` 并钳制到契约范围。

### 9.6 投影回正式单元

- 从内部网格双线性采样月温、降水和风；
- 使用正式单元高程相对网格平均高程做最后一级温度递减修正；
- 纬度直接由正式单元坐标计算；
- 海洋性与气候值对齐同一 `CellId`；
- 不修改 `ReliefSnapshot` 或任何上游字段。

## 10. 阶段与缓存

新增三个阶段：

| 阶段 | 输入 | 输出 |
|---|---|---|
| `natural.resolve-climate-rules` | climate spec, packs | climate audit |
| `natural.project-climate-input` | climate audit | minimal climate input |
| `natural.preliminary-climate` | resolved climate input, spatial, relief | preliminary climate |

生产图由 9 个阶段扩展到 12 个，外部产物由 5 个扩展到 6 个。

缓存边界：

- 改变气候规格只失效气候规则解析、投影和初步气候；
- 改变气候规则审计但得到相同最小输入时，气候阶段命中；
- 改变地貌失效地质和气候，但地质与气候彼此没有伪依赖；
- 改变地质规格可改变地貌时才通过 `ReliefArtifact` 影响气候；
- 仅地表地质物性变化不得使气候阶段读取 `GeologicArtifact`；
- 气候变化绝不失效空间、构造、地幔、地貌或地质。

## 11. 字段与显示

新增正式字段：

| 字段 ID | 类型 | 单位 | 依赖 |
|---|---|---|---|
| `latitude_degrees@1` | ScalarF32 | degree | spatial position |
| `maritime_influence@1` | ScalarF32 | unitless | land/ocean |
| `preliminary_prevailing_wind_m_s@1` | Vector2F32 | m/s | latitude |
| `preliminary_mean_air_temperature_c@1` | ScalarF32 | °C | latitude, elevation, maritime |
| `preliminary_temperature_seasonality_c@1` | ScalarF32 | °C | latitude, elevation, maritime |
| `preliminary_annual_precipitation_mm@1` | ScalarF32 | mm/year | temperature, elevation, maritime, wind |

现有 V1 显示系统可直接填色五个标量字段。风字段可检查但暂不做单元填色；向量箭头必须等真实网络/向量叠加切片设计后再实现。

逐月字段不注册为 36 个平铺 UI 项。月度数据已经在正式快照中；未来月份控件应作为显示适配，不改变世界契约。

应用文档原子持有：

```text
Spatial + Tectonic + Mantle + Relief + Geology + PreliminaryClimate
```

任一产物验证失败都不发布半成品文档。

## 12. 诊断

结构化代码：

- `natural.invalid-climate-spec`
- `rules.invalid-climate-resolution`
- `natural.invalid-resolved-climate-input`
- `natural.climate-build-failed`
- `natural.invalid-preliminary-climate`

若内部网格输入退化为全陆或全海，不视为错误；算法按明确安全分支生成。只有非有限值、范围错误、字段错位或无法满足快照恒等式才阻止发布。

## 13. 性能与内存预算

- 内部气候网格最多 4096 格；
- 12 个月、固定水汽迭代上限；
- 时间复杂度为 `O(cell_count + months × grid_cells × iterations)`；
- 正式月度温度、降水和风在最大 200,000 单元约占 38.4 MB；
- 年度摘要和解释字段约再占 6.4 MB；
- 默认 20,000 单元气候密集字段约 4.5 MB；
- 静态 UI 帧不重新生成气候或重建空间网格。

性能测试记录阶段耗时和新增密集字节，不以机器相关的硬毫秒阈值制造脆弱测试。

## 14. 验证

### 14.1 契约

- 规格边界和反序列化重验证；
- 快照长度、范围、有限性和月/年恒等式；
- 空间、地貌对齐；
- artifact key、stage ID、版本与依赖精确。

### 14.2 性质

固定种子集必须满足：

- 两半球季节相位相反；
- 高纬年均温整体低于低纬；
- 同纬高海拔整体更冷；
- 海洋及高海洋性区域季节振幅较小；
- 年降水非退化且同时存在湿润和干燥陆地区域；
- 迎风抬升不会比相同上游水汽的背风下降更干；
- 所有月度和年度值有限且在范围内；
- 同种子、同输入原生重复构建逐字节一致；
- 原生与 WASM 分类/字段范围一致。

### 14.3 缓存与架构

- 12 阶段第二次构建全部命中；
- 气候规格变化只重跑 3 个气候阶段；
- 构造变化按真实依赖重跑地貌及气候；
- 气候模块不导入 `app`、`ui`、`gpu`、旧 `terrain`、魔法、社会或历史；
- `world` 仍不依赖 `engine` 和 `rules`。

### 14.4 视觉

人工审阅：

- 年均温形成纬向基线，但山地与海洋性打破机械条纹；
- 年降水沿水汽来向、海岸和迎风坡组织，背风侧可见雨影；
- 海洋性从海岸向内陆平滑衰减；
- 温度季节性在高纬内陆更强、海洋更弱；
- 不出现椭圆大陆、随机椒盐点或整图单色。

## 15. 迁移边界

- `natural_foundation_graph` 名称暂时保留，语义继续扩展为当前正式自然基础图；
- 本切片不发布完整 `NaturalSnapshot`；
- “preliminary” 出现在 artifact、类型和字段 ID 中，避免以后最终气候发生双写；
- 水文只消费月度初步降水和正式地貌/地质，不读取气候生成器内部网格；
- 最终气候将拥有不同正式字段，并显式读取水文/侵蚀结果；
- 旧 `terrain::hydrology` 保持编译，但不得进入生产阶段图。

## 16. 自审

### 16.1 正交性

- 气候规格、规则审计、生成、阶段适配、字段 schema 和显示适配各自单责；
- 气候只读空间与地貌，不读不需要的地质物性；
- 地质和气候作为地貌之后的两个独立消费者，没有横向调用；
- 月度世界真值与年度显示摘要由同一气候阶段唯一写入。

### 16.2 完整性

- 没有空接口、占位 artifact 或未来水文类型；
- 延后能力有明确所有者；
- 规则、缓存、验证、显示和性能均包含在同一可验收切片。

### 16.3 当前切片边界

- 月份是同一当前气候常态的季节维度，不是历史时间线；
- 不生成某年某月的天气事件；
- 固定月序列不携带虚构年份、纪年或事件。

## 17. 完成定义

只有同时满足以下条件，本切片才完成：

- 世界法则唯一选择气候模型；
- 生产图发布通过验证的初步气候产物；
- 月度温度、降水和风满足契约与年度恒等式；
- 纬度、海洋性和年度摘要可独立查看；
- 多种子质量门和人工金图通过；
- 缓存失效证明没有上游或横向污染；
- workspace tests、release tests、fmt、Clippy、native、WASM 和 Trunk 通过；
- 合并后的桌面应用完成字段切换和新种子重建验收；
- 变更提交、合并并推送到主分支；
- 主分支 release 再次启动供用户查看。
