// 地形图元系统
//
// 图元是高级地形单元，封装了一组低级命令。
// 模板通过组合图元来创建复杂地形。

use std::f32::consts::PI;
use super::template::TerrainCommand;

/// 尺寸等级
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    Tiny,
    Small,
    Medium,
    Large,
    Huge,
}

impl Size {
    /// 转换为半径系数 (0.0-1.0)
    pub fn to_radius(&self) -> (f32, f32) {
        match self {
            Size::Tiny => (0.02, 0.05),
            Size::Small => (0.05, 0.10),
            Size::Medium => (0.10, 0.18),
            Size::Large => (0.18, 0.28),
            Size::Huge => (0.28, 0.40),
        }
    }
    
    /// 转换为长度系数 (0.0-1.0)
    pub fn to_length(&self) -> (f32, f32) {
        match self {
            Size::Tiny => (0.05, 0.12),
            Size::Small => (0.12, 0.25),
            Size::Medium => (0.25, 0.40),
            Size::Large => (0.40, 0.60),
            Size::Huge => (0.60, 0.85),
        }
    }
}

/// 高度等级
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Elevation {
    Low,      // 低矮丘陵
    Medium,   // 中等山地
    High,     // 高山
    Extreme,  // 极高峰
}

impl Elevation {
    pub fn to_height(&self) -> (f32, f32) {
        match self {
            Elevation::Low => (40.0, 70.0),
            Elevation::Medium => (70.0, 110.0),
            Elevation::High => (110.0, 150.0),
            Elevation::Extreme => (150.0, 200.0),
        }
    }
    
    pub fn to_depth(&self) -> (f32, f32) {
        match self {
            Elevation::Low => (15.0, 30.0),
            Elevation::Medium => (30.0, 50.0),
            Elevation::High => (50.0, 80.0),
            Elevation::Extreme => (80.0, 120.0),
        }
    }
}

/// 位置约束
#[derive(Debug, Clone, Copy)]
pub struct PositionConstraint {
    pub x: (f32, f32),
    pub y: (f32, f32),
}

impl Default for PositionConstraint {
    fn default() -> Self {
        Self {
            x: (0.0, 1.0),
            y: (0.0, 1.0),
        }
    }
}

impl PositionConstraint {
    pub fn center() -> Self {
        Self { x: (0.3, 0.7), y: (0.3, 0.7) }
    }
    
    pub fn edge() -> Self {
        Self { x: (0.0, 1.0), y: (0.0, 1.0) }
    }
    
    pub fn left() -> Self {
        Self { x: (0.0, 0.3), y: (0.1, 0.9) }
    }
    
    pub fn right() -> Self {
        Self { x: (0.7, 1.0), y: (0.1, 0.9) }
    }
}

/// 地形图元 - 高级地形单元
#[derive(Debug, Clone)]
pub enum TerrainPrimitive {
    // ============ 山地类 ============
    
    /// 单独山峰 - 圆锥形隆起
    MountainPeak {
        size: Size,
        elevation: Elevation,
        position: PositionConstraint,
    },
    
    /// 山脉链 - 线性山脊
    MountainChain {
        size: Size,         // 影响长度
        elevation: Elevation,
        count: u32,         // 山脊数量
        position: PositionConstraint,
    },
    
    /// 火山 - 中心高耸，可选火山口
    Volcano {
        size: Size,
        elevation: Elevation,
        has_crater: bool,
        position: PositionConstraint,
    },
    
    /// 高原 - 平坦的高地
    Plateau {
        size: Size,
        elevation: Elevation,
        position: PositionConstraint,
    },
    
    // ============ 低地/水体类 ============
    
    /// 盆地/湖泊 - 圆形凹陷
    Basin {
        size: Size,
        depth: Elevation,
        position: PositionConstraint,
    },
    
    /// 峡谷/裂谷 - 线性凹陷
    Rift {
        size: Size,         // 影响长度
        depth: Elevation,
        position: PositionConstraint,
    },
    
    /// 峡湾 - 深入陆地的狭长水道
    Fjord {
        size: Size,
        depth: Elevation,
        position: PositionConstraint,
    },
    
    // ============ 大陆类 ============
    
    /// 大陆核心 - 大块陆地基础
    ContinentCore {
        size: Size,
        elevation: Elevation,
        position: PositionConstraint,
    },
    
    /// 群岛 - 多个小岛
    Archipelago {
        island_count: u32,
        island_size: Size,
        spread: Size,       // 分布范围
        position: PositionConstraint,
    },
    
    /// 半岛 - 从陆地延伸出的狭长地带
    Peninsula {
        size: Size,
        elevation: Elevation,
        position: PositionConstraint,
    },
    
    // ============ 海洋类 ============
    
    /// 海沟 - 深海沟槽
    OceanTrench {
        size: Size,
        depth: Elevation,
        position: PositionConstraint,
    },
    
    /// 洋中脊 - 海底山脊
    MidOceanRidge {
        size: Size,
        elevation: Elevation,
        position: PositionConstraint,
    },
    
    /// 深海盆地
    AbyssalPlain {
        size: Size,
        count: u32,
        position: PositionConstraint,
    },
}

impl TerrainPrimitive {
    /// 将图元展开为低级命令列表
    pub fn to_commands(&self) -> Vec<TerrainCommand> {
        match self {
            // ============ 山地类 ============
            
            TerrainPrimitive::MountainPeak { size, elevation, position } => {
                let radius = size.to_radius();
                let height = elevation.to_height();
                vec![
                    TerrainCommand::Hill {
                        count: 1,
                        height,
                        x: position.x,
                        y: position.y,
                        radius,
                    },
                ]
            }
            
            TerrainPrimitive::MountainChain { size, elevation, count, position } => {
                let length = size.to_length();
                let height = elevation.to_height();
                vec![
                    TerrainCommand::Range {
                        count: *count,
                        height,
                        x: position.x,
                        y: position.y,
                        length,
                        width: (0.02, 0.05),
                        angle: (0.0, 2.0 * PI),
                    },
                ]
            }
            
            TerrainPrimitive::Volcano { size, elevation, has_crater, position } => {
                let radius = size.to_radius();
                let height = elevation.to_height();
                let mut commands = vec![
                    // 火山主体
                    TerrainCommand::Hill {
                        count: 1,
                        height,
                        x: position.x,
                        y: position.y,
                        radius,
                    },
                ];
                
                if *has_crater {
                    // 火山口（小坑）
                    let crater_radius = (radius.0 * 0.3, radius.1 * 0.3);
                    let crater_depth = (height.0 * 0.2, height.1 * 0.3);
                    commands.push(TerrainCommand::Pit {
                        count: 1,
                        depth: crater_depth,
                        x: position.x,
                        y: position.y,
                        radius: crater_radius,
                    });
                }
                
                commands
            }
            
            TerrainPrimitive::Plateau { size, elevation, position } => {
                let radius = size.to_radius();
                let height = elevation.to_height();
                // 高原：多个重叠的较平缓隆起
                vec![
                    TerrainCommand::Hill {
                        count: 3,
                        height: (height.0 * 0.8, height.1 * 0.9),
                        x: position.x,
                        y: position.y,
                        radius: (radius.0 * 1.2, radius.1 * 1.5),
                    },
                ]
            }
            
            // ============ 低地/水体类 ============
            
            TerrainPrimitive::Basin { size, depth, position } => {
                let radius = size.to_radius();
                let d = depth.to_depth();
                vec![
                    TerrainCommand::Pit {
                        count: 1,
                        depth: d,
                        x: position.x,
                        y: position.y,
                        radius,
                    },
                ]
            }
            
            TerrainPrimitive::Rift { size, depth, position } => {
                let length = size.to_length();
                let d = depth.to_depth();
                vec![
                    TerrainCommand::Trough {
                        count: 1,
                        depth: d,
                        x: position.x,
                        y: position.y,
                        length,
                        width: (0.02, 0.04),
                        angle: (0.0, 2.0 * PI),
                    },
                ]
            }
            
            TerrainPrimitive::Fjord { size, depth, position } => {
                let length = size.to_length();
                let d = depth.to_depth();
                // 峡湾：窄而深的凹槽
                vec![
                    TerrainCommand::Trough {
                        count: 1,
                        depth: (d.0 * 1.2, d.1 * 1.5),
                        x: position.x,
                        y: position.y,
                        length,
                        width: (0.01, 0.025),
                        angle: (0.0, 2.0 * PI),
                    },
                ]
            }
            
            // ============ 大陆类 ============
            
            TerrainPrimitive::ContinentCore { size, elevation, position } => {
                let radius = size.to_radius();
                let height = elevation.to_height();
                vec![
                    TerrainCommand::Hill {
                        count: 1,
                        height,
                        x: position.x,
                        y: position.y,
                        radius: (radius.0 * 1.5, radius.1 * 2.0),
                    },
                ]
            }
            
            TerrainPrimitive::Archipelago { island_count, island_size, spread: _, position } => {
                let radius = island_size.to_radius();
                // 群岛：多个小岛散布
                vec![
                    TerrainCommand::Hill {
                        count: *island_count,
                        height: (45.0, 80.0),
                        x: position.x,
                        y: position.y,
                        radius,
                    },
                ]
            }
            
            TerrainPrimitive::Peninsula { size, elevation, position } => {
                let length = size.to_length();
                let height = elevation.to_height();
                // 半岛：细长的陆地延伸
                vec![
                    TerrainCommand::Range {
                        count: 1,
                        height,
                        x: position.x,
                        y: position.y,
                        length,
                        width: (0.06, 0.12),
                        angle: (0.0, 2.0 * PI),
                    },
                ]
            }
            
            // ============ 海洋类 ============
            
            TerrainPrimitive::OceanTrench { size, depth, position } => {
                let length = size.to_length();
                let d = depth.to_depth();
                vec![
                    TerrainCommand::Trough {
                        count: 1,
                        depth: d,
                        x: position.x,
                        y: position.y,
                        length,
                        width: (0.015, 0.03),
                        angle: (0.0, 2.0 * PI),
                    },
                ]
            }
            
            TerrainPrimitive::MidOceanRidge { size, elevation, position } => {
                let length = size.to_length();
                let height = elevation.to_height();
                // 洋中脊：海底的低矮山脊
                vec![
                    TerrainCommand::Range {
                        count: 1,
                        height: (height.0 * 0.3, height.1 * 0.4),
                        x: position.x,
                        y: position.y,
                        length,
                        width: (0.02, 0.04),
                        angle: (0.0, 2.0 * PI),
                    },
                ]
            }
            
            TerrainPrimitive::AbyssalPlain { size, count, position } => {
                let radius = size.to_radius();
                vec![
                    TerrainCommand::Pit {
                        count: *count,
                        depth: (12.0, 25.0),
                        x: position.x,
                        y: position.y,
                        radius,
                    },
                ]
            }
        }
    }
}

/// 预设图元组合
pub mod presets {
    use super::*;
    
    /// 喜马拉雅式山脉 - 极高的连续山脉
    pub fn himalayan_range() -> Vec<TerrainPrimitive> {
        vec![
            TerrainPrimitive::MountainChain {
                size: Size::Huge,
                elevation: Elevation::Extreme,
                count: 2,
                position: PositionConstraint::default(),
            },
            TerrainPrimitive::MountainChain {
                size: Size::Large,
                elevation: Elevation::High,
                count: 3,
                position: PositionConstraint::default(),
            },
        ]
    }
    
    /// 环太平洋火山带
    pub fn volcanic_arc() -> Vec<TerrainPrimitive> {
        vec![
            TerrainPrimitive::Volcano {
                size: Size::Medium,
                elevation: Elevation::High,
                has_crater: true,
                position: PositionConstraint::default(),
            },
            TerrainPrimitive::Volcano {
                size: Size::Small,
                elevation: Elevation::Medium,
                has_crater: true,
                position: PositionConstraint::default(),
            },
            TerrainPrimitive::OceanTrench {
                size: Size::Large,
                depth: Elevation::High,
                position: PositionConstraint::default(),
            },
        ]
    }
    
    /// 斯堪的纳维亚式峡湾海岸
    pub fn fjord_coast() -> Vec<TerrainPrimitive> {
        vec![
            TerrainPrimitive::MountainChain {
                size: Size::Medium,
                elevation: Elevation::Medium,
                count: 2,
                position: PositionConstraint::edge(),
            },
            TerrainPrimitive::Fjord {
                size: Size::Medium,
                depth: Elevation::Medium,
                position: PositionConstraint::edge(),
            },
            TerrainPrimitive::Fjord {
                size: Size::Small,
                depth: Elevation::Low,
                position: PositionConstraint::edge(),
            },
        ]
    }
    
    /// 太平洋式群岛
    pub fn pacific_islands() -> Vec<TerrainPrimitive> {
        vec![
            TerrainPrimitive::Archipelago {
                island_count: 15,
                island_size: Size::Small,
                spread: Size::Large,
                position: PositionConstraint::default(),
            },
            TerrainPrimitive::Volcano {
                size: Size::Small,
                elevation: Elevation::High,
                has_crater: true,
                position: PositionConstraint::center(),
            },
            TerrainPrimitive::AbyssalPlain {
                size: Size::Large,
                count: 5,
                position: PositionConstraint::default(),
            },
        ]
    }
    
    /// 东非大裂谷式地形
    pub fn rift_valley() -> Vec<TerrainPrimitive> {
        vec![
            TerrainPrimitive::Plateau {
                size: Size::Large,
                elevation: Elevation::Medium,
                position: PositionConstraint::default(),
            },
            TerrainPrimitive::Rift {
                size: Size::Large,
                depth: Elevation::Medium,
                position: PositionConstraint::center(),
            },
            TerrainPrimitive::Volcano {
                size: Size::Medium,
                elevation: Elevation::High,
                has_crater: true,
                position: PositionConstraint::default(),
            },
            TerrainPrimitive::Basin {
                size: Size::Medium,
                depth: Elevation::Low,
                position: PositionConstraint::default(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_primitive_to_commands() {
        let peak = TerrainPrimitive::MountainPeak {
            size: Size::Large,
            elevation: Elevation::High,
            position: PositionConstraint::center(),
        };
        
        let commands = peak.to_commands();
        assert_eq!(commands.len(), 1);
    }
    
    #[test]
    fn test_volcano_with_crater() {
        let volcano = TerrainPrimitive::Volcano {
            size: Size::Medium,
            elevation: Elevation::High,
            has_crater: true,
            position: PositionConstraint::default(),
        };
        
        let commands = volcano.to_commands();
        assert_eq!(commands.len(), 2); // 主体 + 火山口
    }
}
