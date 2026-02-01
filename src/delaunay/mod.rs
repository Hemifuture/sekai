//! Delaunay 三角剖分与 Voronoi 图模块
//!
//! 本模块提供地图几何基础的核心算法：
//! - **Delaunay 三角剖分**: 将点集划分为三角形网格
//! - **Voronoi 图**: Delaunay 的对偶图，用于划分地理单元
//! - **半边数据结构**: 高效的网格拓扑表示
//!
//! # 架构概览
//!
//! ```text
//! 输入点集 (Vec<Pos2>)
//!        │
//!        ▼
//! ┌──────────────────┐
//! │   triangulate()  │  ── Delaunay 三角剖分
//! └────────┬─────────┘
//!          │
//!          ├────────────────────────┐
//!          ▼                        ▼
//!   三角形索引 (Vec<u32>)     DelaunayMesh (半边)
//!          │                        │
//!          ▼                        ▼
//! ┌────────────────────┐   ┌─────────────────────┐
//! │compute_indexed_    │   │ voronoi_cell_       │
//! │    voronoi()       │   │   vertices() (有序) │
//! └────────┬───────────┘   └─────────────────────┘
//!          ▼
//!   IndexedVoronoiDiagram
//! ```
//!
//! # 使用示例
//!
//! ## 基本用法
//!
//! ```ignore
//! use sekai::delaunay::{triangulate, voronoi::compute_indexed_voronoi};
//! use egui::Pos2;
//!
//! // 创建点集
//! let points = vec![
//!     Pos2::new(0.0, 0.0),
//!     Pos2::new(100.0, 0.0),
//!     Pos2::new(50.0, 100.0),
//! ];
//!
//! // 执行 Delaunay 三角剖分
//! let triangle_indices = triangulate(&points);
//!
//! // 生成 Voronoi 图
//! let voronoi = compute_indexed_voronoi(&triangle_indices, &points);
//! ```
//!
//! ## 使用半边结构（推荐）
//!
//! ```ignore
//! use sekai::delaunay::{triangulate_mesh, DelaunayMesh};
//!
//! let points = vec![...];
//! let mesh = triangulate_mesh(points);
//!
//! // 获取有序的 Voronoi 单元格顶点
//! for v in 0..mesh.point_count() as u32 {
//!     let (vertices, is_closed) = mesh.voronoi_cell_vertices(v);
//!     // vertices 已经是逆时针有序的！
//! }
//!
//! // 用于渲染
//! let (voronoi_vertices, voronoi_indices) = mesh.voronoi_render_data();
//! ```
//!
//! # 模块结构
//!
//! - `delaunay`: 三角剖分算法实现
//! - `half_edge`: 半边数据结构（高效拓扑查询）
//! - `voronoi`: Voronoi 图生成
//! - `triangle`: 三角形数据结构
//! - `utils`: 验证和辅助工具

#[allow(clippy::module_inception)]
mod delaunay;
pub mod half_edge;
mod triangle;
mod utils;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod voronoi_tests;

// ============================================================================
// 公开 API
// ============================================================================

/// Delaunay 三角剖分函数
///
/// 将点集划分为满足 Delaunay 性质的三角形网格。
/// 返回三角形索引列表（`Vec<u32>`）。
pub use delaunay::triangulate;

/// 创建带半边结构的 Delaunay 网格
///
/// 这是推荐的方式，提供完整的拓扑信息和高效的查询。
pub use delaunay::triangulate_mesh;

/// 半边网格数据结构
///
/// 提供 O(1) 邻接查询和有序的 Voronoi 单元格遍历。
pub use half_edge::DelaunayMesh;

/// 无效索引常量
pub use half_edge::EMPTY;

/// 三角形数据结构
///
/// 用于几何计算和验证。
pub use triangle::Triangle;

/// Delaunay 验证函数
///
/// 验证三角剖分结果是否满足 Delaunay 性质。
pub use utils::validate_delaunay;

/// Voronoi 图模块
///
/// 提供 Voronoi 图生成相关的类型和函数。
pub mod voronoi;
