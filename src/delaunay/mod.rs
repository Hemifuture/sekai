//! Delaunay 三角剖分与 Voronoi 图模块
//!
//! 本模块提供地图几何基础的核心算法：
//! - **Delaunay 三角剖分**: 将点集划分为三角形网格
//! - **Voronoi 图**: Delaunay 的对偶图，用于划分地理单元
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
//!          ▼
//!   三角形索引 (Vec<usize>)
//!          │
//!          ▼
//! ┌────────────────────────────┐
//! │ compute_indexed_voronoi() │  ── Voronoi 图生成
//! └────────────┬───────────────┘
//!              │
//!              ▼
//!   IndexedVoronoiDiagram
//! ```
//!
//! # 使用示例
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
//!     // ...
//! ];
//!
//! // 执行 Delaunay 三角剖分
//! let triangle_indices = triangulate(&points);
//!
//! // 生成 Voronoi 图
//! let voronoi = compute_indexed_voronoi(&triangle_indices, &points);
//!
//! // 用于渲染
//! let (vertices, indices) = voronoi.get_render_data();
//! ```
//!
//! # 模块结构
//!
//! - `delaunay`: 三角剖分算法实现
//! - `voronoi`: Voronoi 图生成
//! - `triangle`: 三角形数据结构
//! - `utils`: 验证和辅助工具

mod delaunay;
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
pub use delaunay::triangulate;

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
