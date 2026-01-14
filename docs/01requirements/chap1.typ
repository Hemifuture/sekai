// 第一章：需求文档

#import "@preview/cetz:0.4.2"
#import "@preview/cheq:0.3.0": checklist

= 需求文档

== 项目概述

=== 项目目标

创建一个功能强大的地图生成与编辑工具，能够：

+ *自动生成拟真地形* - 基于程序化生成算法，创建逼真的地形（山脉、平原、海洋、河流等）
+ *生成政治板块* - 自动划分国家/地区边界，模拟真实的政治地理格局
+ *交互式编辑* - 支持用户对生成的地图进行自定义修改
+ *多平台支持* - 同时支持桌面原生应用和 Web 浏览器运行

=== 技术架构

#figure(
  table(
    columns: (auto, auto, 1fr),
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 8pt,
    [*组件*], [*技术选型*], [*说明*],
    [编程语言], [Rust], [高性能、内存安全],
    [GUI 框架], [egui / eframe], [即时模式 GUI，支持 Web 和原生],
    [GPU 渲染], [wgpu], [跨平台图形 API],
    [三角剖分], [delaunator], [Delaunay 三角剖分库],
    [Web 编译], [Trunk], [WASM 编译与打包工具],
    [并行计算], [rayon], [数据并行计算],
    [噪声生成], [noise / fastnoise-lite], [程序化噪声生成],
  ),
  caption: [技术架构]
)

=== 项目灵感来源

本项目参考了 Fantasy Map Generator 的设计理念，但使用 Rust + GPU 渲染重新实现，以获得更好的性能和跨平台能力。

== 核心概念

=== 几何基础

==== 网格系统 (Grid)

地图的基础是一个*抖动网格* (Jittered Grid)：

- 在规则网格的基础上添加随机偏移，避免人工感
- 每个网格点代表一个潜在的 Voronoi 单元中心
- 参数：
  - `width` / `height`: 地图尺寸（逻辑单位）
  - `spacing`: 网格间距，决定单元格密度
  - `jittering`: 抖动强度（默认为 spacing 的 45%）

==== Delaunay 三角剖分

将网格点连接成三角形网络：

- 保证任意三角形的外接圆内不包含其他点
- 作为 Voronoi 图的对偶图
- 用于地形高度插值和邻接关系计算

==== Voronoi 图

基于 Delaunay 三角剖分生成：

- 每个 Voronoi 单元对应一个网格点
- 单元边界是相邻三角形外心的连线
- 形成自然、不规则的多边形区域
- 作为地图的基本地理单元（Cell）

#figure(
  cetz.canvas({
    import cetz.draw: *

    // 设置画布尺寸和边距
    set-style(
      stroke: (paint: black, thickness: 0.8pt),
      fill: none,
    )

    // 定义 Voronoi 单元格的中心点
    let centers = (
      (1, 0),
      (3, 0),
      (5, 0),
    )

    // 绘制六边形 Voronoi 单元格
    for (i, center) in centers.enumerate() {
      let (cx, cy) = center
      let points = ()
      for j in range(6) {
        let angle = j * 60deg + 30deg
        let x = cx + 0.8 * calc.cos(angle)
        let y = cy + 0.8 * calc.sin(angle)
        points.push((x, y))
      }

      // 绘制六边形
      line(..points, close: true, stroke: (paint: blue, thickness: 1pt))

      // 绘制中心点
      circle(center, radius: 0.08, fill: red, stroke: none)
    }

    // 添加标注
    content((6.5, 0), [· = 单元中心], anchor: "west", padding: 0.1)
  }),
  caption: [Voronoi 单元格示意图],
)

=== 分层数据模型

地图数据采用分层架构，每一层负责特定的地理或政治属性：

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let width = 12
    let row-height = 0.6
    let title-height = 0.8

    // 定义各层数据
    let layers = (
      (
        title: "渲染/可视化层",
        rows: (
          ("标注层 (Labels)", "地名、城市名、区域名称"),
          ("路线层 (Routes)", "道路、航线、贸易路线"),
          ("军事层 (Military)", "军事设施、防御工事"),
          ("城镇层 (Burgs)", "城市、城镇、村庄、定居点"),
        ),
      ),
      (
        title: "政治/社会层",
        rows: (
          ("宗教层 (Religions)", "信仰区域分布"),
          ("省份层 (Provinces)", "行政区划"),
          ("国家层 (States)", "政治实体、国境线"),
          ("文化层 (Cultures)", "文化区域、民族分布"),
        ),
      ),
      (
        title: "自然地理层",
        rows: (
          ("生物群落层 (Biomes)", "生态区域（森林、沙漠等）"),
          ("气候层 (Climate)", "温度、降水、风向"),
          ("水系层 (Rivers)", "河流、湖泊、瀑布"),
          ("海陆层 (Coastline)", "海岸线、岛屿、半岛"),
          ("高度层 (Heightmap)", "地形高度、山脉、平原"),
        ),
      ),
      (
        title: "基础几何层",
        rows: (
          ("Voronoi 单元格", "基本地理单元"),
          ("Delaunay 三角形", "邻接关系"),
          ("网格点 (Grid)", "采样点"),
        ),
      ),
    )

    let y = 0

    for layer in layers {
      // 绘制标题栏
      rect((0, y), (width, y - title-height), fill: rgb("#e0e0e0"), stroke: (paint: black, thickness: 0.8pt))
      content((width / 2, y - title-height / 2), text(weight: "bold", size: 10pt, layer.title))

      y -= title-height

      // 绘制内容行
      for row in layer.rows {
        rect((0, y), (width, y - row-height), stroke: (paint: black, thickness: 0.5pt))

        // 左列（名称）
        content((0.1, y - row-height / 2), text(size: 8pt, row.at(0)), anchor: "west")

        // 分隔线
        line((4, y), (4, y - row-height), stroke: (paint: gray, thickness: 0.3pt))

        // 右列（描述）
        content((4.1, y - row-height / 2), text(size: 8pt, row.at(1)), anchor: "west")

        y -= row-height
      }
    }
  }),
  caption: [分层数据模型],
)

=== 数据模型

==== 单元格数据 (CellsData)

每个 Voronoi 单元存储的属性：

#figure(
  table(
    columns: (auto, auto, 1fr, auto),
    align: (left, left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*属性*], [*类型*], [*说明*], [*生成阶段*],
    [`height`], [`u8`], [高度值 (0-255)，20为海平面], [Phase 1],
    [`temperature`], [`i8`], [温度 (-128°C ~ 127°C)], [Phase 2],
    [`precipitation`], [`u8`], [年降水量 (0-255 映射到 mm)], [Phase 2],
    [`biome`], [`u16`], [生物群落 ID], [Phase 2],
    [`flux`], [`u16`], [水流量（河流计算用）], [Phase 1],
    [`state`], [`u16`], [国家/政治实体 ID], [Phase 3],
    [`culture`], [`u16`], [文化区域 ID], [Phase 3],
    [`religion`], [`u16`], [宗教区域 ID], [Phase 3],
    [`province`], [`u16`], [省份/行政区 ID], [Phase 3],
    [`population`], [`u32`], [人口数量], [Phase 4],
    [`harbor`], [`u8`], [港口等级 (0=无)], [Phase 4],
  ),
  caption: [单元格数据结构]
)

==== 边数据 (EdgesData)

存储 Voronoi 边的属性：

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*属性*], [*类型*], [*说明*],
    [`river_id`], [`u16`], [河流 ID (0=无河流)],
    [`river_width`], [`u8`], [河流宽度],
    [`is_border`], [`bool`], [是否为边界],
    [`border_type`], [`u8`], [边界类型（国境/省界等）],
  ),
  caption: [边数据结构]
)

==== 特征系统 (Feature System)

*区域特征 (RegionFeature)*

```rust
struct RegionFeature {
    id: u16,
    name: String,
    color: Color32,
    cells: Vec<u32>,        // 包含的单元格索引
    center: Option<u32>,    // 中心单元格（如首都所在）
    area: f32,              // 面积
    perimeter: Vec<u32>,    // 边界单元格
}
```

*线性特征 (LinearFeature)*

```rust
struct LinearFeature {
    id: u16,
    name: String,
    feature_type: LinearFeatureType,  // River, Road, Route
    path: Vec<u32>,                   // 经过的单元格/边
    width: f32,
    source: Option<u32>,              // 起点
    mouth: Option<u32>,               // 终点
}
```

*点特征 (PointFeature)*

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*特征类型*], [*说明*], [*属性*],
    [`City`], [城市], [人口、等级、港口、城堡],
    [`Town`], [城镇], [人口、类型],
    [`Village`], [村庄], [人口],
    [`Marker`], [标记点], [图标、描述],
  ),
  caption: [点特征类型]
)

== 功能需求详细

=== Phase 1: 地形生成 #emoji.construction

==== 高度图生成

*目标*: 生成自然、多样化的地形高度分布

*主要算法：板块构造模拟 + 噪声细节* #emoji.star

采用"板块构造主导 + 噪声细节叠加"的分层生成策略，这是真实性与性能的最佳平衡：

*设计理念*：
- *板块构造*：决定大尺度地形格局（山脉、海沟、高原）— 主导机制
- *噪声叠加*：添加中小尺度细节（褶皱、侵蚀沟壑）— 板块内部变化
- *分层实施*：先宏观后微观，符合地质形成的时序

*与真实地质过程的对应*：

#figure(
  table(
    columns: (auto, auto, 1fr, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*地质过程*], [*空间尺度*], [*现实实例*], [*模拟方法*],
    [板块运动], [数千公里], [喜马拉雅山、马里亚纳海沟], [板块构造模拟],
    [区域构造], [数百公里], [褶皱山系、火山群], [中频噪声],
    [侵蚀沉积], [数公里], [河谷、冲积扇、沙丘], [高频噪声 + 可选物理侵蚀],
  ),
  caption: [多尺度地形生成对应表]
)

*优势对比*：

#figure(
  table(
    columns: (auto, auto, auto, auto, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*方案*], [*真实性*], [*性能*], [*可控性*], [*适用场景*],
    [纯噪声], [低], [高], [高], [快速原型],
    [板块构造+噪声], [*高*], [*中*], [*高*], [*推荐方案*],
    [完整物理模拟], [极高], [低], [低], [科学研究],
  ),
  caption: [不同地形生成方案对比]
)

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let width = 14
    let title-height = 0.8
    let step-height = 0.6
    let content-height = 1.8

    // 绘制标题
    rect((0, 0), (width, -title-height), fill: rgb("#d0d0d0"), stroke: (paint: black, thickness: 0.8pt))
    content((width / 2, -title-height / 2), text(weight: "bold", size: 11pt, "板块构造模拟流程"))

    let y = -title-height

    // 定义各步骤
    let steps = (
      (
        title: "1. 板块生成",
        items: (
          "• 随机放置 N 个板块种子点（默认 12-15 个）",
          "• 使用 Voronoi 划分板块区域",
          "• 分配板块类型（大陆板块/海洋板块）",
        ),
      ),
      (
        title: "2. 运动分配",
        items: (
          "• 为每个板块分配运动方向和速度",
          "• 可选：板块旋转",
        ),
      ),
      (
        title: "3. 边界分析",
        items: (
          "• 汇聚边界（碰撞）→ 造山/俯冲带",
          "• 分离边界（张裂）→ 裂谷/洋脊",
          "• 转换边界（错动）→ 断层",
        ),
      ),
      (
        title: "4. 高度更新（多次迭代模拟地质时间）",
        items: (
          "• 碰撞区域隆起形成山脉",
          "• 俯冲区域下沉形成海沟",
          "• 分离区域产生裂谷",
          "• 应用地壳均衡调整",
        ),
      ),
      (
        title: "5. 后处理",
        items: (
          "• 添加噪声细节",
          "• 平滑处理",
          "• 可选：侵蚀模拟",
        ),
      ),
    )

    for step in steps {
      // 计算内容高度
      let items-height = step.items.len() * 0.45

      // 绘制步骤标题
      rect((0, y), (width, y - step-height), fill: rgb("#f0f0f0"), stroke: (paint: black, thickness: 0.5pt))
      content((0.3, y - step-height / 2), text(weight: "bold", size: 9pt, step.title), anchor: "west")

      y -= step-height

      // 绘制步骤内容
      rect((0, y), (width, y - items-height), stroke: (paint: black, thickness: 0.5pt))

      for (i, item) in step.items.enumerate() {
        content((0.5, y - 0.225 - i * 0.45), text(size: 8pt, item), anchor: "west")
      }

      y -= items-height
    }
  }),
  caption: [板块构造模拟流程],
)

*板块类型*:

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*类型*], [*密度*], [*特点*],
    [大陆板块], [2.7 g/cm³], [较轻，浮力大，形成大陆],
    [海洋板块], [3.0 g/cm³], [较重，易俯冲到大陆板块下],
  ),
  caption: [板块类型]
)

*边界类型与地形效果*:

#figure(
  table(
    columns: (auto, auto, auto, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*边界类型*], [*相对运动*], [*地形效果*], [*典型实例*],
    [汇聚（大陆-大陆）], [相向], [高大褶皱山脉], [喜马拉雅山],
    [汇聚（海洋-大陆）], [相向], [海沟+火山弧], [日本海沟],
    [汇聚（海洋-海洋）], [相向], [海沟+岛弧], [马里亚纳海沟],
    [分离], [背向], [裂谷/洋中脊], [大西洋中脊],
    [转换], [平行错动], [断层], [圣安德烈斯断层],
  ),
  caption: [板块边界类型与地形效果]
)

*实现要点*:
#show: checklist
- [ ] *阶段 1：板块构造模拟*
  - [ ] 板块生成与 Voronoi 划分
  - [ ] 板块运动向量分配
  - [ ] 边界类型分析（汇聚/分离/转换）
  - [ ] 碰撞隆起与俯冲下沉计算
  - [ ] 地壳均衡（Isostasy）调整
  - [ ] 迭代模拟（100-300次代表地质时间）
- [ ] *阶段 2：中尺度噪声*（模拟区域构造）
  - [ ] 低频噪声叠加（3层，频率0.01）
  - [ ] 噪声强度受板块类型约束（大陆0.3，海洋0.1）
  - [ ] 边界附近噪声抑制
- [ ] *阶段 3：侵蚀模拟*（可选）
  - [ ] 热力侵蚀
  - [ ] 水力侵蚀
- [ ] *阶段 4：小尺度噪声*（模拟表面细节）
  - [ ] 高频噪声叠加（5层，频率0.05）
  - [ ] 高度调制（高山区增强）
- [ ] *阶段 5：后处理*
  - [ ] 高度值归一化到 0-255
  - [ ] 可选平滑处理
#show: checklist.with()

*用户参数*:

#figure(
  table(
    columns: (auto, 1fr, auto, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*参数分类*], [*参数*], [*说明*], [*默认值*],
    [板块构造], [`plate_count`], [板块数量], [12],
    [], [`continental_ratio`], [大陆板块比例], [0.4],
    [], [`iterations`], [模拟迭代次数], [100],
    [], [`collision_uplift_rate`], [碰撞隆起速率], [0.5],
    [], [`subduction_depth_rate`], [俯冲下沉速率], [0.3],
    [噪声细节], [`medium_noise_strength`], [中尺度噪声强度], [0.2],
    [], [`detail_noise_strength`], [小尺度噪声强度], [0.1],
    [], [`continental_noise_mult`], [大陆噪声倍数], [1.5],
    [], [`oceanic_noise_mult`], [海洋噪声倍数], [0.5],
    [侵蚀（可选）], [`enable_erosion`], [启用侵蚀模拟], [false],
    [], [`erosion_iterations`], [侵蚀迭代次数], [50],
    [通用], [`seed`], [随机种子], [-],
    [], [`smoothing`], [平滑强度（0=关闭）], [0],
  ),
  caption: [完整地形生成参数]
)

*预设配置*:

#figure(
  table(
    columns: (auto, auto, auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*预设*], [*板块数*], [*大陆比例*], [*迭代次数*], [*效果*],
    [类地球], [15], [0.3], [200], [均衡的大陆分布],
    [多山地], [20], [0.5], [300], [复杂的山脉系统],
    [群岛], [25], [0.2], [150], [分散的岛屿],
    [超级大陆], [8], [0.6], [250], [一个巨大的连续大陆],
  ),
  caption: [板块构造预设配置]
)

==== 海陆分布

*目标*: 基于高度图划分海洋与陆地

*规则*:
- 海平面阈值：height = 20（可配置）
- height < 20: 海洋/水体
- height >= 20: 陆地

*特殊处理*:
#show: checklist
- [ ] 检测并标记孤立水体（湖泊候选）
- [ ] 检测并标记孤立陆地（岛屿）
- [ ] 计算海岸线（海陆交界的边）
- [ ] 分离大陆与岛屿
#show: checklist.with()

==== 水系生成

*目标*: 生成自然的河流网络和湖泊

*河流生成算法*:

+ *降水分配*: 每个陆地单元格根据高度和气候接收降水
+ *水流汇聚*: 水从高处流向低处，累计流量
+ *河流阈值*: 流量超过阈值的路径成为河流
+ *河口确定*: 河流汇入海洋或湖泊的位置

*实现步骤*:
#show: checklist
- [ ] 计算每个单元格的流向（指向最低邻居）
- [ ] 累积流量计算（从高到低遍历）
- [ ] 河流路径提取
- [ ] 河流宽度计算（基于流量）
- [ ] 河流合并（支流汇入干流）
#show: checklist.with()

*湖泊生成*:
#show: checklist
- [ ] 检测地形凹陷（四周都比中心高）
- [ ] 填充凹陷形成湖泊
- [ ] 湖泊溢出（找到最低出口点）
- [ ] 冰川湖（高海拔低温区域）
#show: checklist.with()

=== Phase 2: 气候与生物群落

==== 温度系统

*影响因素*:

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*因素*], [*权重*], [*说明*],
    [纬度], [高], [赤道热，两极冷],
    [海拔], [中], [每升高1000m降约6.5°C],
    [洋流], [低], [暖流升温，寒流降温],
    [大陆性], [低], [内陆温差大],
  ),
  caption: [温度影响因素]
)

*计算公式*:
```
base_temp = 30 - abs(latitude) * 0.6
altitude_effect = -height * 0.026
temp = base_temp + altitude_effect + ocean_current_effect
```

==== 降水系统

*影响因素*:

#figure(
  table(
    columns: (auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*因素*], [*说明*],
    [季风/信风], [主导风向决定降水分布],
    [地形], [迎风坡多雨，背风坡干燥（雨影效应）],
    [距海距离], [沿海湿润，内陆干燥],
    [洋流], [暖流增加降水],
  ),
  caption: [降水影响因素]
)

==== 生物群落分配

基于温度和降水自动确定生物群落：

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let width = 10
    let height = 8
    let margin-left = 1.5
    let margin-bottom = 1

    // 绘制坐标轴
    line(
      (margin-left, margin-bottom),
      (margin-left, margin-bottom + height),
      mark: (end: "stealth"),
      stroke: (thickness: 1pt),
    )
    line(
      (margin-left, margin-bottom),
      (margin-left + width, margin-bottom),
      mark: (end: "stealth"),
      stroke: (thickness: 1pt),
    )

    // Y轴标签（温度）
    content((margin-left - 0.3, margin-bottom + height / 2), text(size: 10pt, "温度"), anchor: "east")
    content((margin-left - 0.6, margin-bottom + height * 0.875), "高", anchor: "east")
    content((margin-left - 0.6, margin-bottom + height * 0.625), "中", anchor: "east")
    content((margin-left - 0.6, margin-bottom + height * 0.375), "低", anchor: "east")
    content((margin-left - 0.6, margin-bottom + height * 0.125), "极低", anchor: "east")

    // X轴标签（降水量）
    content((margin-left + width / 2, margin-bottom - 0.5), text(size: 10pt, "降水量"))
    content((margin-left + width * 0.17, margin-bottom - 0.3), "低")
    content((margin-left + width * 0.5, margin-bottom - 0.3), "中")
    content((margin-left + width * 0.83, margin-bottom - 0.3), "高")

    // 定义生物群落区域 (x, y, width, height, name, color)
    let biomes = (
      // 高温
      (0, 0.75, 0.33, 0.25, "沙漠", rgb("#EDC9AF")),
      (0.33, 0.75, 0.33, 0.25, "热带草原", rgb("#C4B454")),
      (0.66, 0.75, 0.17, 0.25, "热带季雨林", rgb("#4B6F44")),
      (0.83, 0.75, 0.17, 0.25, "热带雨林", rgb("#0B6623")),
      // 中温
      (0, 0.5, 0.5, 0.25, "温带草原", rgb("#D5C96E")),
      (0.5, 0.5, 0.33, 0.25, "温带落叶林", rgb("#6B8E23")),
      (0.83, 0.5, 0.17, 0.25, "温带雨林", rgb("#228B22")),
      // 低温
      (0, 0.25, 0.33, 0.25, "寒漠", rgb("#D3D3D3")),
      (0.33, 0.25, 0.67, 0.25, "针叶林（泰加）", rgb("#1E6B52")),
      // 极低温
      (0, 0, 0.5, 0.25, "冰原", rgb("#F0F8FF")),
      (0.5, 0, 0.5, 0.25, "苔原", rgb("#96C8A2")),
    )

    // 绘制生物群落区域
    for biome in biomes {
      let (rel-x, rel-y, rel-w, rel-h, name, color) = biome
      let x = margin-left + rel-x * width
      let y = margin-bottom + rel-y * height
      let w = rel-w * width
      let h = rel-h * height

      // 绘制矩形
      rect((x, y), (x + w, y + h), fill: color.transparentize(30%), stroke: (paint: gray, thickness: 0.5pt))

      // 添加文字标签
      content((x + w / 2, y + h / 2), text(size: 8pt, name))
    }

    // 添加标题
    content((margin-left + width / 2, margin-bottom + height + 0.7), text(weight: "bold", size: 11pt, "生物群落分布图"))
  }),
  caption: [Whittaker 生物群落分类],
)

*生物群落列表*:

#let patch_width = 2cm
#let patch_height = 0.5cm

#figure(
  table(
    columns: (auto, auto, auto, auto, auto, auto),
    stroke: 0.5pt,
    inset: 5pt,
    [*ID*], [*名称*], [*温度范围*], [*降水范围*], [*颜色*], [*填充色*],
    [1], [冰原], [< -10], [任意], "#FFFFFF", box(width: patch_width, height: patch_height, fill: rgb("#DDDDDD")),
    [2], [苔原], [-10 ~ 0], [< 250mm], "#96C8A2", box(width: patch_width, height: patch_height, fill: rgb("#96C8A2")),
    [3], [针叶林], [0 ~ 10], [> 250mm], "#1E6B52", box(width: patch_width, height: patch_height, fill: rgb("#1E6B52")),
    [4], [温带草原], [5 ~ 20], [250-500mm], "#D5C9E6E", box(width: patch_width, height: patch_height, fill: rgb("#D5C96E")),
    [5], [温带落叶林], [5 ~ 20], [500-1500mm], "#6B8E23", box(width: patch_width, height: patch_height, fill: rgb("#6B8E23")),
    [6], [温带雨林], [10 ~ 20], [> 1500mm], "#228B22", box(width: patch_width, height: patch_height, fill: rgb("#228B22")),
    [7], [沙漠], [> 15], [< 250mm], "#EDC9AF", box(width: patch_width, height: patch_height, fill: rgb("#EDC9AF")),
    [8], [热带草原], [> 20], [500-1500mm], "#C4B454", box(width: patch_width, height: patch_height, fill: rgb("#C4B454")),
    [9], [热带季雨林], [> 20], [1500-2500mm], "#4B6F44", box(width: patch_width, height: patch_height, fill: rgb("#4B6F44")),
    [10], [热带雨林], [> 25], [> 2500mm], "#0B6623", box(width: patch_width, height: patch_height, fill: rgb("#0B6623")),
    [11], [湿地], [任意], [特殊], "#4A6741", box(width: patch_width, height: patch_height, fill: rgb("#4A6741")),
    [12], [红树林], [> 20], [沿海], "#3D5229", box(width: patch_width, height: patch_height, fill: rgb("#3D5229")),
  ),
  caption: [生物群落列表]
)

=== Phase 3: 政治地理

==== 文化区域生成

*生成逻辑*:
+ 随机放置文化种子点（考虑人口分布）
+ 文化扩张（类似 Voronoi 但受地形影响）
+ 地理屏障限制扩张（山脉、大河、海洋）
+ 文化边界稳定化

```rust
struct Culture {
    id: u16,
    name: String,
    type_: CultureType,     // Nomadic, River, Coastal, Highland, etc.
    color: Color32,
    expansionism: f32,      // 扩张性 0-1
    cells: Vec<u32>,
    origin_cell: u32,       // 起源地
}

enum CultureType {
    Nomadic,    // 游牧文化（草原）
    River,      // 河流文化
    Coastal,    // 沿海文化
    Highland,   // 高原文化
    Hunting,    // 狩猎文化（森林/苔原）
    Agricultural, // 农耕文化
}
```

==== 国家生成

*生成步骤*:

+ *放置首都候选*:
  - 优先选择港口、河流交汇处、平原地带
  - 考虑现有人口分布
  - 避开极端气候区域

+ *初始领土*:
  - 以首都为中心分配初始领土
  - 受文化边界影响

+ *国家扩张*:
  - 模拟历史扩张过程
  - 扩张成本计算（地形、距离、文化差异）
  - 自然边界形成（山脉、河流）

+ *边界优化*:
  - 消除飞地
  - 稳定边界线
  - 处理包围地

=== Phase 4: 城镇与路网

==== 城市放置

*选址因素*:

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*因素*], [*权重*], [*说明*],
    [河流], [高], [靠近河流加分],
    [港口], [高], [天然港口位置],
    [地形], [中], [平原优于山区],
    [资源], [中], [矿产、农业用地],
    [交通], [中], [交叉路口、关隘],
    [安全], [低], [易守难攻的位置],
  ),
  caption: [城市选址因素]
)

*城市等级*:

#figure(
  table(
    columns: (auto, auto, auto, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*等级*], [*类型*], [*人口范围*], [*设施*],
    [1], [首都], [> 500,000], [城堡、港口、市场],
    [2], [大城市], [100,000-500,000], [可选城堡、港口],
    [3], [城市], [20,000-100,000], [市场],
    [4], [城镇], [5,000-20,000], [-],
    [5], [村庄], [< 5,000], [-],
  ),
  caption: [城市等级分类]
)

==== 道路网络

*道路类型*:

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*类型*], [*优先级*], [*说明*],
    [主干道], [1], [连接首都与大城市],
    [次干道], [2], [连接城市],
    [地方道路], [3], [连接城镇],
    [小径], [4], [连接村庄],
    [贸易路线], [特殊], [跨国贸易路线],
  ),
  caption: [道路类型分类]
)

*路径算法*:
- 使用 A\* 算法寻找最短路径
- 成本函数考虑地形（山地高、平原低）
- 优先使用现有道路
- 避开水体（除非有桥/渡口）

=== Phase 5: 渲染系统 #emoji.checkmark 进行中

==== 基础渲染 #emoji.checkmark

已完成：
#show: checklist
- [x] 网格点渲染
- [x] Delaunay 三角剖分线框渲染
- [x] Voronoi 边界渲染
- [x] GPU 加速渲染 (wgpu)
- [x] 着色器 (WGSL)
#show: checklist.with()

==== 图层系统

#figure(
  table(
    columns: (auto, 1fr, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*图层*], [*渲染内容*], [*Z-Order*],
    [基础网格], [Voronoi/Delaunay 边], [0],
    [地形高度], [单元格填充色], [10],
    [海洋], [海洋区域填充], [11],
    [河流湖泊], [河流线、湖泊填充], [20],
    [生物群落], [单元格着色], [30],
    [政治边界], [国境线、省界], [40],
    [文化区域], [虚线边界], [41],
    [道路], [道路线], [50],
    [城市图标], [点精灵], [60],
    [标注], [文字], [70],
  ),
  caption: [图层系统]
)

=== Phase 6: 交互功能 #emoji.checkmark 部分完成

==== 画布操作 #emoji.checkmark

已完成：
#show: checklist
- [x] 平移 (Pan) - 空格键 + 拖拽 或 滚轮
- [x] 缩放 (Zoom) - 鼠标滚轮 / 触控板捏合
- [x] 缩放限制 (0.1x - 100x)
#show: checklist.with()

==== 编辑工具

#figure(
  table(
    columns: (auto, 1fr, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*工具*], [*功能*], [*快捷键*],
    [笔刷], [绘制地形高度], [B],
    [橡皮], [降低地形/删除], [E],
    [选择], [选中单元格], [S],
    [填充], [区域填充], [F],
    [吸管], [拾取属性], [I],
    [河流], [绘制河流], [R],
    [路线], [绘制道路], [T],
  ),
  caption: [编辑工具]
)

=== Phase 7: 导入导出

==== 项目文件

待实现：
#show: checklist
- [ ] 保存为项目文件（.sekai）
- [ ] 自动保存
- [ ] 版本迁移
#show: checklist.with()

==== 导出格式

#figure(
  table(
    columns: (auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*格式*], [*用途*],
    [PNG], [静态地图图片],
    [SVG], [矢量地图],
    [JSON], [数据交换],
    [GeoJSON], [GIS 兼容],
    [Heightmap PNG], [高度图灰度图],
  ),
  caption: [导出格式]
)

== 性能需求

=== 渲染性能

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*指标*], [*目标值*], [*说明*],
    [帧率], [≥ 60 FPS], [正常交互情况下],
    [单元格数量], [支持 100,000+], [不明显卡顿],
    [初始加载], [< 5 秒], [包含完整地图生成],
    [图层切换], [< 100ms], [即时响应],
  ),
  caption: [渲染性能指标]
)

=== 内存使用

#figure(
  table(
    columns: (auto, auto),
    stroke: 0.5pt,
    inset: 6pt,
    [*场景*], [*预期内存*],
    [10,000 单元格], [< 50 MB],
    [50,000 单元格], [< 200 MB],
    [100,000 单元格], [< 500 MB],
  ),
  caption: [内存使用预期]
)

=== 优化策略

*已实现*:
#show: checklist
- [x] 视口裁剪 - 只渲染可见区域的几何体
- [x] GPU 加速 - 使用 wgpu 进行渲染
- [x] 并行计算 - 使用 rayon 加速 CPU 计算
- [x] 空间索引 - 加速单元格查询和视口裁剪
#show: checklist.with()

*待实现*:
#show: checklist
- [ ] LOD (Level of Detail) - 缩小时减少细节
- [ ] 分块加载 - 超大地图按需加载
- [ ] 增量更新 - 只更新变化的部分
- [ ] Buffer 复用 - 避免每帧重新分配 GPU Buffer
- [ ] 计算着色器 - GPU 加速生成算法
#show: checklist.with()

== 开发路线图

=== Phase 1: 地形生成 (当前目标)

#show: checklist
- [ ] 高度图生成算法（噪声叠加）
- [ ] 海陆分布检测
- [ ] 高度着色渲染
- [ ] 海岸线提取
- [ ] 河流生成算法
- [ ] 湖泊检测与生成
- [ ] 水系渲染
#show: checklist.with()

=== Phase 2: 气候与生物群落

#show: checklist
- [ ] 温度计算
- [ ] 降水计算
- [ ] 生物群落分配
- [ ] 生物群落可视化
#show: checklist.with()

=== Phase 3: 政治地理

#show: checklist
- [ ] 文化区域生成
- [ ] 国家生成
- [ ] 省份划分
- [ ] 边界渲染
- [ ] 宗教分布
#show: checklist.with()

=== Phase 4: 城镇与路网

#show: checklist
- [ ] 城市放置算法
- [ ] 人口分布计算
- [ ] 道路网络生成
- [ ] 标注系统
#show: checklist.with()

=== Phase 5: 编辑功能

#show: checklist
- [ ] 笔刷工具系统
- [ ] 选择工具
- [ ] 撤销/重做系统
- [ ] 特征编辑面板
#show: checklist.with()

=== Phase 6: 完善与导出

#show: checklist
- [ ] 导出功能
- [ ] 项目保存/加载
- [ ] 性能优化
- [ ] UI 美化
#show: checklist.with()

== 术语表

#figure(
  table(
    columns: (auto, auto, 1fr),
    stroke: 0.5pt,
    inset: 6pt,
    [*术语*], [*英文*], [*说明*],
    [单元格], [Cell], [Voronoi 图中的一个多边形区域],
    [三角剖分], [Triangulation], [将点集划分为不重叠三角形的过程],
    [外心], [Circumcenter], [三角形外接圆的圆心],
    [生物群落], [Biome], [具有相似气候和生态特征的区域],
    [图层], [Layer], [地图的一个可视化层级],
    [抖动], [Jittering], [向规则网格添加随机偏移],
    [水系], [Hydrography], [河流、湖泊等水体系统],
    [通量], [Flux], [水流量，用于河流计算],
    [雨影], [Rain Shadow], [山脉背风坡降水减少的效应],
    [城镇], [Burg], [城市、城镇等定居点的统称],
  ),
  caption: [术语表]
)
