# 数据结构优化建议

本文档分析当前 Delaunay/Voronoi 模块的数据结构，并提出优化建议。

---

## 一、当前数据结构分析

### 1.1 Delaunay 三角剖分

**当前实现：**
```rust
// 输入
points: Vec<Pos2>           // 每个 Pos2 = 8 bytes (2 × f32)

// 输出
indices: Vec<usize>         // 每个 usize = 8 bytes (64-bit)
```

**内存占用估算（10,000 点）：**
- 点数据：10,000 × 8 = 80 KB
- 三角形索引：约 20,000 × 3 × 8 = 480 KB（理论 2n-2-k 个三角形）
- 总计：约 560 KB

**问题：**
1. `usize` 在 64 位系统占 8 字节，但点数很少超过 10 万，用 `u32` 就够了
2. 三角形索引没有存储邻接信息，需要时要重新计算

### 1.2 Voronoi 图

**当前实现：**
```rust
pub struct IndexedVoronoiDiagram {
    pub vertices: Vec<Pos2>,        // Voronoi 顶点（外心）
    pub indices: Vec<usize>,        // 边索引
    pub cells: Vec<VoronoiCell>,    // 单元格信息
}

pub struct VoronoiCell {
    pub site_idx: usize,
    pub vertex_indices: Vec<usize>, // 动态分配！
}
```

**问题：**
1. 每个 `VoronoiCell` 都有一个动态 `Vec`，导致内存碎片化
2. `vertex_indices` 未排序成闭合多边形顺序，无法直接用于填充渲染
3. 边界单元格的顶点不完整（边界 Voronoi 边被忽略）

---

## 二、优化建议

### 2.1 使用紧凑索引类型

**方案：统一使用 `u32` 代替 `usize`**

```rust
// 优化后
pub fn triangulate(points: &[Pos2]) -> Vec<u32> { ... }

pub struct IndexedVoronoiDiagram {
    pub vertices: Vec<Pos2>,
    pub indices: Vec<u32>,      // 节省 50% 内存
    pub cells: Vec<VoronoiCell>,
}
```

**收益：**
- 索引内存减半
- GPU 友好（大多数 GPU 索引缓冲区使用 u32）
- 支持最多 40 亿个点，远超实际需求

**实施难度：低**

### 2.2 半边数据结构 (Half-Edge / DCEL)

**方案：使用 Doubly Connected Edge List 存储拓扑**

```rust
/// 半边数据结构
pub struct HalfEdge {
    /// 半边终点的顶点索引
    pub vertex: u32,
    /// 对偶半边索引
    pub twin: u32,
    /// 同一面内的下一条半边
    pub next: u32,
    /// 所属面（三角形）索引
    pub face: u32,
}

pub struct DelaunayMesh {
    /// 所有顶点
    pub vertices: Vec<Pos2>,
    /// 所有半边
    pub half_edges: Vec<HalfEdge>,
    /// 每个顶点的一条出边索引
    pub vertex_edge: Vec<u32>,
    /// 每个面（三角形）的一条边索引
    pub face_edge: Vec<u32>,
}
```

**收益：**
- O(1) 查询邻接三角形
- O(1) 遍历顶点周围的边/面
- Voronoi 单元格顶点自然有序（沿半边遍历即可）
- 支持局部修改（插入/删除点）

**实施难度：中-高**

### 2.3 扁平化单元格存储

**方案：避免每个单元格单独分配 Vec**

```rust
pub struct FlatVoronoiDiagram {
    /// Voronoi 顶点
    pub vertices: Vec<Pos2>,
    
    /// 边索引（用于渲染）
    pub edge_indices: Vec<u32>,
    
    /// 所有单元格的顶点索引（连续存储）
    pub cell_vertex_indices: Vec<u32>,
    
    /// 每个单元格在 cell_vertex_indices 中的起始位置和长度
    /// cell_offsets[i] = (start, len)
    pub cell_offsets: Vec<(u32, u16)>,
}

impl FlatVoronoiDiagram {
    /// 获取单元格的顶点索引
    pub fn get_cell_vertices(&self, cell_idx: usize) -> &[u32] {
        let (start, len) = self.cell_offsets[cell_idx];
        &self.cell_vertex_indices[start as usize..(start as usize + len as usize)]
    }
}
```

**收益：**
- 单次分配，无内存碎片
- 更好的缓存局部性
- 减少堆分配开销

**实施难度：低**

### 2.4 有序单元格顶点

**方案：在生成时就排序单元格顶点**

```rust
/// 为单元格顶点排序，使其形成闭合多边形
fn sort_cell_vertices(
    cell_vertices: &mut [u32],
    edges: &[(u32, u32)],  // 边列表
    vertices: &[Pos2],
) {
    if cell_vertices.len() < 3 {
        return;
    }
    
    // 方法1：使用边连接关系排序
    // 从第一个顶点开始，找到连接的下一个顶点
    
    // 方法2：按极角排序
    // 计算每个顶点相对于单元格中心的极角
    let center = compute_cell_center(cell_vertices, vertices);
    cell_vertices.sort_by(|&a, &b| {
        let angle_a = (vertices[a as usize].y - center.y)
            .atan2(vertices[a as usize].x - center.x);
        let angle_b = (vertices[b as usize].y - center.y)
            .atan2(vertices[b as usize].x - center.x);
        angle_a.partial_cmp(&angle_b).unwrap()
    });
}
```

**收益：**
- 可直接用于多边形填充渲染
- 支持计算单元格面积
- 支持点击测试（判断点在哪个单元格内）

**实施难度：低**

### 2.5 空间索引

**方案：添加网格或四叉树索引**

```rust
/// 简单的网格索引
pub struct GridIndex {
    cell_size: f32,
    width: usize,
    height: usize,
    /// 每个网格格子包含的点索引
    cells: Vec<Vec<u32>>,
}

impl GridIndex {
    /// 查找包含指定点的 Voronoi 单元格
    pub fn find_cell(&self, pos: Pos2, points: &[Pos2]) -> Option<u32> {
        // 先查找网格格子
        let candidates = self.get_nearby_points(pos);
        // 在候选点中找最近的
        candidates.iter()
            .min_by(|&&a, &&b| {
                let da = (points[a as usize] - pos).length_sq();
                let db = (points[b as usize] - pos).length_sq();
                da.partial_cmp(&db).unwrap()
            })
            .copied()
    }
}
```

**收益：**
- O(1) 点击测试（查找点所在单元格）
- 加速视口裁剪
- 支持高效的邻居查询

**实施难度：中**

---

## 三、推荐实施优先级

| 优先级 | 优化项 | 收益 | 难度 | 状态 |
|-------|--------|-----|------|------|
| 1 | 使用 `u32` 索引 | 内存减半，GPU 友好 | 低 | ✅ **已完成** |
| 2 | 半边数据结构 | 完整拓扑信息，有序顶点 | 中 | ✅ **已完成** |
| 3 | 扁平化单元格存储 | 减少内存碎片 | 低 | ✅ 已通过半边实现 |
| 4 | 有序单元格顶点 | 支持填充渲染 | 低 | ✅ 已通过半边实现 |
| 5 | 空间索引 | 加速查询 | 中 | 🔲 交互功能需要时实施 |

---

## 四、重构后的推荐结构

```rust
/// 紧凑的 Delaunay 三角剖分结果
pub struct DelaunayTriangulation {
    /// 输入点（可选存储）
    pub points: Vec<Pos2>,
    
    /// 三角形索引（每3个构成一个三角形）
    pub triangles: Vec<u32>,
    
    /// 三角形邻接信息（可选）
    /// adjacency[i*3+j] = 三角形 i 的边 j 的相邻三角形索引
    pub adjacency: Option<Vec<u32>>,
}

/// 紧凑的 Voronoi 图
pub struct VoronoiDiagram {
    /// Voronoi 顶点（三角形外心）
    pub vertices: Vec<Pos2>,
    
    /// 边索引（每2个构成一条边，用于线框渲染）
    pub edge_indices: Vec<u32>,
    
    /// 单元格顶点索引（所有单元格连续存储）
    pub cell_vertices: Vec<u32>,
    
    /// 单元格偏移 (start, len)
    pub cell_offsets: Vec<(u32, u16)>,
    
    /// 原始点数量（= 单元格数量）
    pub site_count: usize,
}

impl VoronoiDiagram {
    /// 获取单元格 i 的顶点索引
    pub fn cell(&self, i: usize) -> &[u32] {
        let (start, len) = self.cell_offsets[i];
        &self.cell_vertices[start as usize..][..len as usize]
    }
    
    /// 获取单元格 i 的顶点坐标
    pub fn cell_vertices(&self, i: usize) -> impl Iterator<Item = Pos2> + '_ {
        self.cell(i).iter().map(|&idx| self.vertices[idx as usize])
    }
}
```

---

## 五、性能对比预估

| 场景 | 当前内存 | 优化后内存 | 改善 |
|------|---------|-----------|------|
| 10,000 点 Delaunay | ~560 KB | ~360 KB | -36% |
| 10,000 点 Voronoi | ~800 KB | ~400 KB | -50% |
| 50,000 点总计 | ~6.8 MB | ~3.8 MB | -44% |

---

*文档版本: 1.0*
*最后更新: 2026-01-11*
