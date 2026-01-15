# 地形模板系统

## 概述

地形模板系统参考了 [Azgaar's Fantasy Map Generator](https://azgaar.github.io/Fantasy-Map-Generator/)，提供了一种更快速、更可预测的方式来生成不同类型的地形。与基于物理的板块构造模拟不同，模板系统使用一系列预定义的命令来直接构建地形特征。

## 生成模式

系统支持两种地形生成模式：

### 1. 模板生成（Template）
- **优点**：快速、可预测、海陆比例精确控制
- **缺点**：相对缺乏物理真实性
- **适用场景**：需要特定地形类型（如群岛、大陆等）

### 2. 板块构造模拟（TectonicSimulation）
- **优点**：物理真实、自然的板块边界特征
- **缺点**：生成速度较慢、海陆比例难以精确控制
- **适用场景**：需要高度真实的地质特征

## 预设模板

### Earth-like（类地球）
约 30% 陆地，70% 海洋，平衡的大陆和海洋配置。

```rust
let config = TerrainConfig::with_template("earth-like");
```

**特点**：
- 3-4 个大陆核心
- 次级陆块和岛屿
- 海沟和深海盆地
- 平滑的海岸线

### Archipelago（群岛）
约 10-20% 陆地，众多小岛屿分布在广阔海洋中。

```rust
let config = TerrainConfig::with_template("archipelago");
```

**特点**：
- 大量小岛屿（25+ 个）
- 少数较大岛屿
- 深海沟分隔岛屿群
- 适合海岛冒险设定

### Continental（大陆式）
约 40-50% 陆地，一到两个大型大陆。

```rust
let config = TerrainConfig::with_template("continental");
```

**特点**：
- 单个或成对的大陆
- 山脉系统
- 较少的海域
- 适合大陆探险设定

### Volcanic Island（火山岛）
单个高耸的火山岛屿，周围有小岛。

```rust
let config = TerrainConfig::with_template("volcanic_island");
```

**特点**：
- 中心火山峰
- 周围小山丘和岛屿
- 极少的陆地面积（< 5%）
- 适合孤岛生存设定

### Atoll（环礁）
环形珊瑚礁岛屿围绕浅水泻湖。

```rust
let config = TerrainConfig::with_template("atoll");
```

**特点**：
- 环形岛屿排列
- 中央浅泻湖
- 低海拔地形
- 独特的热带岛屿风格

### Peninsula（半岛式）
从地图一侧延伸的半岛。

```rust
let config = TerrainConfig::with_template("peninsula");
```

**特点**：
- 主陆块连接
- 延伸的半岛
- 次级岛屿
- 适合沿海探索设定

### Highland（高地）
约 70% 陆地，高原和山地主导。

```rust
let config = TerrainConfig::with_template("highland");
```

**特点**：
- 大量丘陵和高原
- 山脉系统
- 少量湖泊或内陆海
- 适合山地冒险设定

### Oceanic（深海平原）
约 95% 海洋，极少岛屿。

```rust
let config = TerrainConfig::with_template("oceanic");
```

**特点**：
- 广阔的海洋
- 极少孤立岛屿
- 海底山脉和深海沟
- 适合海洋探险设定

## 使用方法

### 基本使用

```rust
use sekai::terrain::{TerrainConfig, TerrainGenerator};

// 使用预设模板
let config = TerrainConfig::with_template("earth-like");
let generator = TerrainGenerator::new(config);

// 生成地形
let (heights, plates, plate_id) = generator.generate(&cells, &neighbors);
```

### 自定义配置

```rust
let mut config = TerrainConfig::with_template("archipelago");

// 添加细节噪声
config.detail_noise_strength = 0.15;

// 启用侵蚀
config.enable_erosion = true;
config.erosion_iterations = 30;

// 额外平滑
config.smoothing = 2;

let generator = TerrainGenerator::new(config);
```

### 使用板块构造模拟

```rust
use sekai::terrain::{TerrainConfig, TectonicConfig};

let tectonic_config = TectonicConfig {
    plate_count: 12,
    continental_ratio: 0.4,
    iterations: 100,
    ..Default::default()
};

let config = TerrainConfig::with_tectonic_simulation(tectonic_config);
let generator = TerrainGenerator::new(config);
```

## 模板命令参考

模板由一系列命令组成，每个命令修改高度图：

### Mountain（山脉）
创建单个大型中心凸起。

```rust
TerrainCommand::Mountain {
    height: 200.0,      // 高度 (0-255)
    x: 0.5,             // X 位置 (0.0-1.0)
    y: 0.5,             // Y 位置 (0.0-1.0)
    radius: 0.15,       // 半径 (0.0-1.0)
}
```

### Hill（丘陵）
创建随机分布的圆形隆起。

```rust
TerrainCommand::Hill {
    count: 10,                  // 数量
    height: (50.0, 100.0),      // 高度范围
    x: (0.1, 0.9),              // X 位置范围
    y: (0.1, 0.9),              // Y 位置范围
    radius: (0.08, 0.15),       // 半径范围
}
```

### Pit（坑洞）
创建圆形凹陷（与丘陵相反）。

```rust
TerrainCommand::Pit {
    count: 5,
    depth: (20.0, 40.0),        // 深度（正值表示下降）
    x: (0.0, 1.0),
    y: (0.0, 1.0),
    radius: (0.1, 0.2),
}
```

### Range（山脉）
创建细长的隆起区域。

```rust
TerrainCommand::Range {
    count: 3,
    height: (80.0, 120.0),
    x: (0.1, 0.9),
    y: (0.1, 0.9),
    length: (0.4, 0.7),         // 长度
    width: (0.04, 0.08),        // 宽度
    angle: (0.0, 6.28),         // 角度（弧度）
}
```

### Trough（海沟）
创建细长的凹陷区域（与山脉相反）。

### Strait（海峡）
创建垂直或水平的水道。

```rust
TerrainCommand::Strait {
    width: 0.05,
    direction: StraitDirection::Vertical,
    position: 0.5,
    depth: 30.0,
}
```

### Add（加法）
为所有单元格添加固定高度值。

```rust
TerrainCommand::Add { value: 20.0 }     // 提升
TerrainCommand::Add { value: -10.0 }    // 降低
```

### Multiply（乘法）
将所有高度值乘以系数。

```rust
TerrainCommand::Multiply { factor: 1.1 }    // 增强
TerrainCommand::Multiply { factor: 0.8 }    // 减弱
```

### Smooth（平滑）
平均周围单元格的高度。

```rust
TerrainCommand::Smooth { iterations: 3 }
```

### Mask（遮罩）
应用渐变效果。

```rust
TerrainCommand::Mask {
    mode: MaskMode::EdgeFade,       // 边缘渐隐
    strength: 0.3,
}
```

模式选项：
- `EdgeFade`: 边缘降低，中心保持
- `CenterBoost`: 中心升高，边缘降低
- `RadialGradient`: 径向渐变

### Invert（反转）
沿轴镜像高度图。

```rust
TerrainCommand::Invert {
    axis: InvertAxis::X,        // X、Y 或 Both
    probability: 0.5,           // 执行概率
}
```

### Normalize（归一化）
将高度值重新映射到 0-255 范围。

```rust
TerrainCommand::Normalize
```

### SetSeaLevel（设置海平面）
标记并调整海平面以下区域。

```rust
TerrainCommand::SetSeaLevel { level: 20.0 }
```

## 创建自定义模板

```rust
use sekai::terrain::{TerrainTemplate, TerrainCommand};

let my_template = TerrainTemplate::new(
    "My Custom Terrain",
    "A unique terrain configuration"
)
.with_commands(vec![
    TerrainCommand::Add { value: 15.0 },
    TerrainCommand::Hill {
        count: 5,
        height: (80.0, 120.0),
        x: (0.2, 0.8),
        y: (0.2, 0.8),
        radius: (0.1, 0.2),
    },
    TerrainCommand::Smooth { iterations: 2 },
    TerrainCommand::Normalize,
    TerrainCommand::SetSeaLevel { level: 20.0 },
]);
```

## 性能对比

| 模式 | 生成时间 | 海陆控制 | 真实性 |
|------|---------|---------|--------|
| 模板生成 | ~50-100ms | 精确 | 中等 |
| 板块构造 | ~500-1000ms | 近似 | 高 |

## 推荐配置

### 快速原型开发
```rust
let config = TerrainConfig::with_template("earth-like");
config.detail_noise_strength = 0.1;
```

### 高质量地图
```rust
let mut config = TerrainConfig::with_template("continental");
config.detail_noise_strength = 0.2;
config.enable_erosion = true;
config.erosion_iterations = 50;
config.smoothing = 3;
```

### 真实地质特征
```rust
let tectonic_config = TectonicConfig::default();
let config = TerrainConfig::with_tectonic_simulation(tectonic_config);
```

## 参考资料

- [Azgaar's Fantasy Map Generator](https://azgaar.github.io/Fantasy-Map-Generator/)
- [Heightmap Template Editor Wiki](https://github.com/Azgaar/Fantasy-Map-Generator/wiki/Heightmap-template-editor)
- [Heightmap Customization](https://github.com/Azgaar/Fantasy-Map-Generator/wiki/Heightmap-customization)
