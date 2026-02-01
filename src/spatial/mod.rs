//! 空间索引模块
//!
//! 提供高效的空间查询功能，用于：
//! - 点击测试（查找鼠标位置所在的 Voronoi 单元格）
//! - 视口裁剪（快速获取视口内的边/点）
//! - 邻居查询（查找某点附近的其他点）
//!
//! # 主要类型
//! - [`GridIndex`][]: 基于均匀网格的点索引
//! - [`EdgeIndex`][]: 基于均匀网格的边索引（用于视口裁剪）

mod edge_index;
mod grid_index;

pub use edge_index::EdgeIndex;
pub use grid_index::GridIndex;
