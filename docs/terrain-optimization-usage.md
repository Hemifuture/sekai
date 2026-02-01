# 地形生成优化 - 使用指南

## 概述

本次优化解决了两个核心问题：
1. **地形分布太均匀** - 现在使用 BFS 扩散算法创造自然不规则的地形
2. **海岸线散点/噪点** - 现在有特征检测和清理功能

## 新增文件

- `src/terrain/blob.rs` - BFS 扩散式地形生成器
- `src/terrain/features.rs` - 地形特征检测与清理

## 配置选项

在 `TerrainConfig` 中新增了以下选项：

```rust
TerrainConfig {
    // ... 原有选项 ...
    
    // 新增：特征清理
    enable_feature_cleanup: true,  // 是否启用特征清理
    min_island_size: 3,            // 最小岛屿大小（单元格数）
    min_lake_size: 2,              // 最小湖泊大小（单元格数）
    coastline_smoothing: 1,        // 海岸线平滑迭代次数
    use_constrained_noise: true,   // 是否使用约束噪声
}
```

## 使用示例

### 基本使用（使用所有优化）

```rust
use sekai::terrain::{TerrainGenerator, TerrainConfig};

let config = TerrainConfig::default();  // 默认启用所有优化
let generator = TerrainGenerator::new(config);
let (heights, plates, plate_ids) = generator.generate(&cells, &neighbors);
```

### 自定义配置

```rust
let config = TerrainConfig {
    mode: TerrainGenerationMode::Template("earth-like".to_string()),
    enable_feature_cleanup: true,
    min_island_size: 5,     // 更大的岛屿阈值
    min_lake_size: 3,       // 更大的湖泊阈值
    coastline_smoothing: 2, // 更多平滑
    ..Default::default()
};
```

### 禁用优化（回退到旧行为）

```rust
use sekai::terrain::template_executor::GenerationMode;

// 在执行器中使用经典模式
let executor = TemplateExecutor::with_mode(
    width, height, seed,
    GenerationMode::Classic
);
```

或者禁用后处理：

```rust
let config = TerrainConfig {
    enable_feature_cleanup: false,
    coastline_smoothing: 0,
    use_constrained_noise: false,
    ..Default::default()
};
```

## 算法说明

### BFS 扩散算法 (blob.rs)

参考 Azgaar Fantasy Map Generator 的核心算法：

```rust
// 每次扩散时高度按指数衰减
change[n] = change[current].powf(blob_power) * jitter;

// blob_power 根据单元格数量动态调整
// 1000 单元格: 0.93 (快速衰减，小 blob)
// 10000 单元格: 0.98 (慢速衰减，大 blob)
// 100000 单元格: 0.9973 (非常慢衰减，超大 blob)
```

这创造了自然不规则的陆地形状，而不是完美的圆形。

### 特征检测 (features.rs)

使用 BFS flood fill 识别连通区域：

1. **Ocean** - 接触地图边缘的水体
2. **Lake** - 不接触边缘的内陆水体
3. **Island** - 陆地

然后清理太小的区域：
- 小岛屿被淹没（变成海洋）
- 小湖泊被填充（变成陆地）

### 海岸线平滑

移除孤立突出和凹陷的点：
- 如果一个单元格的大多数邻居是不同类型，则转换它

## 效果对比

### 优化前
- 丘陵是完美的圆锥形
- 海岸线有很多孤立的点
- 地形分布均匀、规则

### 优化后
- 丘陵是自然不规则的 blob 形状
- 海岸线干净连续
- 地形分布有真实世界的不对称性

## 调试

启用 debug 编译时会输出调试信息：

```
使用模板生成地形: earth-like
清理了 15 个孤立单元格
平滑了 8 个海岸线单元格
```

## 性能

- BFS 扩散算法的时间复杂度为 O(n)，与传统方法相当
- 特征检测是额外的一次 BFS 遍历，O(n)
- 总体性能影响很小（<10% 增加）

## 未来改进

1. **大陆架生成** - 在海岸线和深海之间创建浅水过渡带
2. **山脉连通性** - 确保山脉形成连续的链条
3. **群岛聚集** - 使岛屿成群分布而不是随机散布
