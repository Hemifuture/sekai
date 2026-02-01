# 地形生成优化研究报告

## 1. 问题分析

### 问题 1: 地形分布太均匀

**根本原因：** Sekai 当前使用的 Hill/Range/Pit 命令是基于**距离衰减函数**生成的，每个地形特征都是独立的圆形或椭圆形。

```rust
// 当前实现 (template_executor.rs)
let falloff = 1.0 - (dist / radius_pixels).powi(2);  // 二次衰减
heights[i] += height * falloff;
```

这会导致：
- 每个丘陵都是完美的圆锥形
- 丘陵之间没有自然的连接
- 缺乏真实地形的不对称性和复杂性

**Azgaar 的解决方案：BFS 扩散算法**

```javascript
// Azgaar heightmap-generator.ts
change[c] = change[q] ** this.blobPower * (Math.random() * 0.2 + 0.9);
if (change[c] > 1) queue.push(c);
```

关键技术：
- `blobPower = 0.93~0.9973`：指数衰减因子
- 高度通过 BFS 从中心向邻居传播
- 每次传播都有 0.8~1.1 的随机扰动
- 这创造了自然不规则的 blob 形状

### 问题 2: 海岸线散点/噪点

**根本原因：** 在模板执行后添加的多层细节噪声没有边界约束：

```rust
// 当前实现 (heightmap.rs)
heights[i] += noise * 35.0;  // 直接添加噪声，没有限制
```

这会导致：
- 海洋中出现孤立的陆地点
- 陆地中出现孤立的海洋点
- 海岸线变得嘈杂不自然

**Azgaar 的解决方案：**
1. **Feature marking** - 识别所有连通区域（海洋、湖泊、岛屿）
2. **清理孤立区域** - 移除太小的岛屿和湖泊
3. **生成时就保证连通性** - BFS 扩散天然产生连通区域

---

## 2. Azgaar 核心算法详解

### 2.1 blobPower 和 linePower

```javascript
// 根据单元格数量调整衰减率
getBlobPower(cells) {
  const blobPowerMap = {
    1000: 0.93,    // 少单元格 = 快速衰减
    10000: 0.98,   // 中等
    100000: 0.9973 // 多单元格 = 慢速衰减
  };
}
```

这确保了无论地图分辨率如何，blob 都能保持相似的相对大小。

### 2.2 Hill 生成（BFS 扩散）

```javascript
addOneHill() {
  const change = new Uint8Array(this.heights.length);
  change[start] = h;  // 起始点获得完整高度
  const queue = [start];
  
  while (queue.length) {
    const q = queue.shift();
    for (const c of neighbors[q]) {
      if (change[c]) continue;
      // 核心：指数衰减 + 随机扰动
      change[c] = change[q] ** this.blobPower * (Math.random() * 0.2 + 0.9);
      if (change[c] > 1) queue.push(c);
    }
  }
  
  this.heights = this.heights.map((h, i) => h + change[i]);
}
```

### 2.3 Range 生成（路径 + 扩散）

```javascript
addOneRange() {
  // 1. 找到从起点到终点的路径
  const range = getRange(startCellId, endCellId);
  
  // 2. 从路径向两侧扩散
  let queue = range.slice();
  while (queue.length) {
    const frontier = queue.slice();
    queue = [];
    frontier.forEach(i => heights[i] += h * (Math.random() * 0.3 + 0.85));
    h = h ** this.linePower - 1;  // 高度衰减
    if (h < 2) break;
    frontier.forEach(f => {
      neighbors[f].forEach(i => {
        if (!used[i]) { queue.push(i); used[i] = 1; }
      });
    });
  }
}
```

### 2.4 Feature Marking（连通区域检测）

```javascript
markupGrid() {
  const queue = [0];
  for (let featureId = 1; queue[0] !== -1; featureId++) {
    const firstCell = queue[0];
    const land = heights[firstCell] >= 20;
    
    // BFS 填充相同类型的区域
    while (queue.length) {
      const cellId = queue.pop();
      for (const neighborId of neighbors[cellId]) {
        if (heights[neighborId] >= 20 === land && !marked[neighborId]) {
          marked[neighborId] = featureId;
          queue.push(neighborId);
        }
      }
    }
    
    features.push({ id: featureId, land, type: land ? 'island' : 'ocean' });
    queue[0] = findUnmarked();
  }
}
```

---

## 3. 改进方案

### 3.1 实现 BFS 扩散式地形生成

**新增文件：** `terrain/blob.rs`

核心功能：
- `add_hill_bfs()` - BFS 扩散式丘陵生成
- `add_range_bfs()` - 路径扩散式山脉生成
- `calculate_blob_power()` - 动态计算衰减因子

### 3.2 添加连通区域检测和清理

**新增文件：** `terrain/features.rs`

核心功能：
- `mark_features()` - 标记所有连通区域
- `cleanup_small_features()` - 清理孤立的小区域
- `get_coastline_cells()` - 获取海岸线单元格

### 3.3 约束细节噪声

**修改文件：** `terrain/heightmap.rs`

改进：
- 添加海岸线安全边距
- 禁止噪声改变海陆类型
- 在内陆/深海区域允许更大噪声

---

## 4. 真实世界地理特征参考

### 4.1 大陆分布不对称性
- 地球 71% 是海洋，陆地集中在北半球
- 大陆形状不规则，有半岛、海湾、内海
- 大陆边缘有大陆架（浅海过渡带）

### 4.2 海岸线分形特性
- 海岸线是分形的，放大后仍然复杂
- 但在单一尺度上，海岸线是**连续**的
- 没有孤立的点状岛屿（除非是珊瑚礁群岛）

### 4.3 山脉线性延伸规律
- 山脉沿板块边界分布，形成长链
- 不是独立的圆锥体，而是相连的山脊
- 有主脊和分支

### 4.4 岛屿聚集分布
- 岛屿通常成群出现（群岛）
- 火山岛呈弧形分布（岛弧）
- 珊瑚环礁有特定结构

---

## 5. 实施计划

### 阶段 1: 核心算法改进（优先级高）
1. 实现 `blob.rs` - BFS 扩散算法
2. 修改 `template_executor.rs` - 使用新算法
3. 测试基本效果

### 阶段 2: 海岸线清理（优先级高）
1. 实现 `features.rs` - 连通区域检测
2. 添加清理孤立区域功能
3. 集成到生成流程

### 阶段 3: 噪声约束（优先级中）
1. 修改细节噪声应用逻辑
2. 添加海岸线安全边距
3. 优化噪声参数

### 阶段 4: 高级特性（可选）
1. 大陆架生成
2. 山脉连通性改进
3. 群岛聚集效果
