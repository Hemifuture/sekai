// 地形模板系统 - 参考 Azgaar's Fantasy Map Generator
//
// 模板是一组操作指令，用于生成特定类型的地形。
// 每个模板定义了一系列的地形修改命令，可以产生可预测但仍具有随机性的地图。

use eframe::egui::Pos2;
use rand::{Rng, SeedableRng};
use std::f32::consts::PI;

/// 地形修改命令
#[derive(Debug, Clone)]
pub enum TerrainCommand {
    /// 山脉 - 单个大型中心凸起
    Mountain {
        height: f32,      // 高度 (0-255)
        x: f32,           // X 位置 (0.0-1.0)
        y: f32,           // Y 位置 (0.0-1.0)
        radius: f32,      // 半径 (0.0-1.0)
    },

    /// 丘陵 - 圆形隆起
    Hill {
        count: u32,       // 数量
        height: (f32, f32), // 高度范围 (min, max)
        x: (f32, f32),    // X 位置范围 (0.0-1.0)
        y: (f32, f32),    // Y 位置范围 (0.0-1.0)
        radius: (f32, f32), // 半径范围 (0.0-1.0)
    },

    /// 坑洞 - 圆形凹陷（与丘陵相反）
    Pit {
        count: u32,
        depth: (f32, f32), // 深度范围（正值表示下降）
        x: (f32, f32),
        y: (f32, f32),
        radius: (f32, f32),
    },

    /// 山脉 - 细长的隆起区域
    Range {
        count: u32,
        height: (f32, f32),
        x: (f32, f32),
        y: (f32, f32),
        length: (f32, f32),  // 长度 (0.0-1.0)
        width: (f32, f32),   // 宽度 (0.0-1.0)
        angle: (f32, f32),   // 角度（弧度）
    },

    /// 海沟 - 细长的凹陷区域（与山脉相反）
    Trough {
        count: u32,
        depth: (f32, f32),
        x: (f32, f32),
        y: (f32, f32),
        length: (f32, f32),
        width: (f32, f32),
        angle: (f32, f32),
    },

    /// 海峡 - 分割陆地的河道
    Strait {
        width: f32,       // 宽度 (0.0-1.0)
        direction: StraitDirection,
        position: f32,    // 位置 (0.0-1.0)
        depth: f32,       // 深度
    },

    /// 添加 - 为所有单元格添加固定高度值
    Add {
        value: f32,       // 可以是负值以降低高度
    },

    /// 乘法 - 将所有高度值乘以系数
    Multiply {
        factor: f32,
    },

    /// 平滑 - 平均周围单元格的高度
    Smooth {
        iterations: u32,
    },

    /// 遮罩 - 应用边缘或中心渐变效果
    Mask {
        mode: MaskMode,
        strength: f32,    // 强度 (0.0-1.0)
    },

    /// 反转 - 沿 X、Y 或两个轴镜像高度图
    Invert {
        axis: InvertAxis,
        probability: f32, // 执行概率 (0.0-1.0)
    },

    /// 归一化 - 将高度值重新映射到 0-255 范围
    Normalize,

    /// 设置海平面 - 将低于阈值的区域设为海洋
    SetSeaLevel {
        level: f32,       // 海平面高度 (0-255)
    },

    /// 调整海陆比例 - 根据目标海洋比例重新分配高度值
    /// 这是修复海陆比例的关键命令，应在 Normalize 之后使用
    AdjustSeaRatio {
        ocean_ratio: f32, // 目标海洋比例 (0.0-1.0)，例如 0.7 表示 70% 海洋
    },
}

/// 海峡方向
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StraitDirection {
    Vertical,
    Horizontal,
}

/// 遮罩模式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaskMode {
    /// 边缘渐隐（边缘降低，中心保持）
    EdgeFade,
    /// 中心增强（中心升高，边缘降低）
    CenterBoost,
    /// 径向渐变（从中心到边缘线性变化）
    RadialGradient,
}

/// 反转轴
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InvertAxis {
    X,
    Y,
    Both,
}

/// 地形模板
#[derive(Debug, Clone)]
pub struct TerrainTemplate {
    pub name: String,
    pub description: String,
    pub commands: Vec<TerrainCommand>,
}

impl TerrainTemplate {
    /// 创建新模板
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            commands: Vec::new(),
        }
    }

    /// 添加命令
    pub fn with_command(mut self, command: TerrainCommand) -> Self {
        self.commands.push(command);
        self
    }

    /// 添加多个命令
    pub fn with_commands(mut self, commands: Vec<TerrainCommand>) -> Self {
        self.commands.extend(commands);
        self
    }

    /// 从图元列表创建模板
    pub fn from_primitives(
        name: impl Into<String>,
        description: impl Into<String>,
        primitives: Vec<super::primitive::TerrainPrimitive>,
    ) -> Self {
        let mut commands = Vec::new();
        for primitive in primitives {
            commands.extend(primitive.to_commands());
        }
        Self {
            name: name.into(),
            description: description.into(),
            commands,
        }
    }
    
    /// 添加图元
    pub fn with_primitive(mut self, primitive: super::primitive::TerrainPrimitive) -> Self {
        self.commands.extend(primitive.to_commands());
        self
    }
    
    /// 添加多个图元
    pub fn with_primitives(mut self, primitives: Vec<super::primitive::TerrainPrimitive>) -> Self {
        for primitive in primitives {
            self.commands.extend(primitive.to_commands());
        }
        self
    }
    
    /// 从 DSL 文本创建模板
    pub fn from_dsl(name: &str, description: &str, dsl: &str) -> Result<Self, super::dsl::ParseError> {
        super::dsl::parse_template(name, description, dsl)
    }
    
    /// 导出为 DSL 文本
    pub fn to_dsl(&self) -> String {
        super::dsl::template_to_dsl(self)
    }

    // ============================================================================
    // 预设模板
    // ============================================================================

    /// 地球式 - 平衡的大陆和海洋（约 30% 陆地，70% 海洋）
    pub fn earth_like() -> Self {
        Self::new(
            "Earth-like",
            "平衡的大陆和海洋配置，约 30% 陆地",
        )
        .with_commands(vec![
            // ====== 简化版：减少命令数量，避免碎片化 ======
            
            // 主大陆核心 - 少量大型
            TerrainCommand::Hill {
                count: 2,
                height: (100.0, 130.0),
                x: (0.2, 0.8),
                y: (0.25, 0.75),
                radius: (0.25, 0.35),
            },
            
            // 次级大陆
            TerrainCommand::Hill {
                count: 3,
                height: (70.0, 100.0),
                x: (0.1, 0.9),
                y: (0.15, 0.85),
                radius: (0.15, 0.22),
            },
            
            // 主要山脉 - 少量长山脉
            TerrainCommand::Range {
                count: 3,
                height: (90.0, 140.0),
                x: (0.15, 0.85),
                y: (0.2, 0.8),
                length: (0.4, 0.6),
                width: (0.03, 0.05),
                angle: (0.0, 2.0 * PI),
            },
            
            // 少量岛屿
            TerrainCommand::Hill {
                count: 5,
                height: (40.0, 70.0),
                x: (0.05, 0.95),
                y: (0.1, 0.9),
                radius: (0.04, 0.08),
            },
            
            // 海沟
            TerrainCommand::Trough {
                count: 2,
                depth: (30.0, 50.0),
                x: (0.1, 0.9),
                y: (0.1, 0.9),
                length: (0.3, 0.5),
                width: (0.02, 0.04),
                angle: (0.0, 2.0 * PI),
            },
            
            // 后处理
            TerrainCommand::Smooth { iterations: 2 },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.7 },
        ])
    }

    /// 群岛 - 许多小岛屿（约 10-20% 陆地）
    pub fn archipelago() -> Self {
        Self::new(
            "Archipelago",
            "众多小岛屿分布在广阔海洋中",
        )
        .with_commands(vec![
            // 大量小丘陵（岛屿）
            TerrainCommand::Hill {
                count: 25,
                height: (40.0, 80.0),
                x: (0.1, 0.9),
                y: (0.1, 0.9),
                radius: (0.03, 0.08),
            },
            // 几个稍大的岛屿
            TerrainCommand::Hill {
                count: 5,
                height: (60.0, 100.0),
                x: (0.2, 0.8),
                y: (0.2, 0.8),
                radius: (0.08, 0.12),
            },
            // 海沟分隔岛屿群
            TerrainCommand::Trough {
                count: 4,
                depth: (25.0, 40.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                length: (0.4, 0.7),
                width: (0.03, 0.06),
                angle: (0.0, 2.0 * PI),
            },
            // 深海区域
            TerrainCommand::Pit {
                count: 10,
                depth: (20.0, 35.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                radius: (0.08, 0.15),
            },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：85% 海洋，15% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.85 },
        ])
    }

    /// 大陆式 - 一两个大陆块（约 40-50% 陆地）
    pub fn continental() -> Self {
        Self::new(
            "Continental",
            "一到两个大型大陆",
        )
        .with_commands(vec![
            // 主大陆核心
            TerrainCommand::Mountain {
                height: 150.0,
                x: 0.5,
                y: 0.5,
                radius: 0.3,
            },
            // 大陆扩展
            TerrainCommand::Hill {
                count: 12,
                height: (70.0, 110.0),
                x: (0.2, 0.8),
                y: (0.2, 0.8),
                radius: (0.12, 0.22),
            },
            // 山脉
            TerrainCommand::Range {
                count: 3,
                height: (80.0, 120.0),
                x: (0.1, 0.9),
                y: (0.1, 0.9),
                length: (0.4, 0.7),
                width: (0.04, 0.08),
                angle: (0.0, 2.0 * PI),
            },
            // 少量海沟
            TerrainCommand::Trough {
                count: 2,
                depth: (30.0, 50.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                length: (0.3, 0.5),
                width: (0.03, 0.05),
                angle: (0.0, 2.0 * PI),
            },
            // 平滑处理
            TerrainCommand::Smooth { iterations: 3 },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：55% 海洋，45% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.55 },
        ])
    }

    /// 火山岛 - 单个高山岛屿
    pub fn volcanic_island() -> Self {
        Self::new(
            "Volcanic Island",
            "单个高耸的火山岛",
        )
        .with_commands(vec![
            // 中心火山
            TerrainCommand::Mountain {
                height: 200.0,
                x: 0.5,
                y: 0.5,
                radius: 0.15,
            },
            // 周围小山丘
            TerrainCommand::Hill {
                count: 5,
                height: (40.0, 80.0),
                x: (0.35, 0.65),
                y: (0.35, 0.65),
                radius: (0.05, 0.1),
            },
            // 小岛屿
            TerrainCommand::Hill {
                count: 3,
                height: (30.0, 60.0),
                x: (0.2, 0.8),
                y: (0.2, 0.8),
                radius: (0.03, 0.06),
            },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：90% 海洋，10% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.90 },
        ])
    }

    /// 环礁 - 环形岛屿围绕中央泻湖
    pub fn atoll() -> Self {
        Self::new(
            "Atoll",
            "环形珊瑚礁岛屿围绕浅水泻湖",
        )
        .with_commands(vec![
            // 中央浅泻湖（微微凹陷）
            TerrainCommand::Pit {
                count: 1,
                depth: (5.0, 8.0),
                x: (0.45, 0.55),
                y: (0.45, 0.55),
                radius: (0.15, 0.2),
            },
            // 环形岛屿
            TerrainCommand::Hill {
                count: 12,
                height: (35.0, 55.0),
                x: (0.3, 0.7),
                y: (0.3, 0.7),
                radius: (0.04, 0.07),
            },
            // 一些突出点
            TerrainCommand::Hill {
                count: 4,
                height: (50.0, 70.0),
                x: (0.35, 0.65),
                y: (0.35, 0.65),
                radius: (0.05, 0.08),
            },
            // 遮罩使边缘更深
            TerrainCommand::Mask {
                mode: MaskMode::CenterBoost,
                strength: 0.3,
            },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：92% 海洋，8% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.92 },
        ])
    }

    /// 半岛式 - 从一侧延伸的陆地
    pub fn peninsula() -> Self {
        Self::new(
            "Peninsula",
            "从地图一侧延伸的半岛",
        )
        .with_commands(vec![
            // 主陆块（从左侧）
            TerrainCommand::Hill {
                count: 8,
                height: (80.0, 120.0),
                x: (0.0, 0.4),
                y: (0.1, 0.9),
                radius: (0.15, 0.25),
            },
            // 延伸半岛
            TerrainCommand::Range {
                count: 2,
                height: (70.0, 100.0),
                x: (0.2, 0.7),
                y: (0.3, 0.7),
                length: (0.4, 0.6),
                width: (0.08, 0.12),
                angle: (0.0, 0.5),
            },
            // 次级岛屿
            TerrainCommand::Hill {
                count: 6,
                height: (50.0, 80.0),
                x: (0.5, 1.0),
                y: (0.0, 1.0),
                radius: (0.06, 0.12),
            },
            // 海沟
            TerrainCommand::Trough {
                count: 2,
                depth: (25.0, 40.0),
                x: (0.3, 0.9),
                y: (0.0, 1.0),
                length: (0.3, 0.5),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 2 },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：65% 海洋，35% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.65 },
        ])
    }

    /// 高地 - 大部分是陆地（约 70% 陆地）
    pub fn highland() -> Self {
        Self::new(
            "Highland",
            "高原和山地主导的地形",
        )
        .with_commands(vec![
            // 大量丘陵
            TerrainCommand::Hill {
                count: 20,
                height: (60.0, 100.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                radius: (0.1, 0.2),
            },
            // 山脉
            TerrainCommand::Range {
                count: 5,
                height: (80.0, 120.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                length: (0.3, 0.6),
                width: (0.05, 0.1),
                angle: (0.0, 2.0 * PI),
            },
            // 少量低地（湖泊或小海）
            TerrainCommand::Pit {
                count: 4,
                depth: (30.0, 50.0),
                x: (0.1, 0.9),
                y: (0.1, 0.9),
                radius: (0.08, 0.15),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 2 },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：30% 海洋，70% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.30 },
        ])
    }

    /// 深海平原 - 主要是海洋（约 5% 陆地）
    pub fn oceanic() -> Self {
        Self::new(
            "Oceanic",
            "广阔的海洋，少量岛屿",
        )
        .with_commands(vec![
            // 极少岛屿
            TerrainCommand::Hill {
                count: 5,
                height: (50.0, 90.0),
                x: (0.1, 0.9),
                y: (0.1, 0.9),
                radius: (0.04, 0.08),
            },
            // 海底山脉
            TerrainCommand::Range {
                count: 3,
                height: (20.0, 40.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                length: (0.5, 0.8),
                width: (0.02, 0.04),
                angle: (0.0, 2.0 * PI),
            },
            // 深海沟
            TerrainCommand::Trough {
                count: 2,
                depth: (15.0, 25.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                length: (0.4, 0.7),
                width: (0.03, 0.06),
                angle: (0.0, 2.0 * PI),
            },
            // 深海盆地
            TerrainCommand::Pit {
                count: 8,
                depth: (10.0, 20.0),
                x: (0.0, 1.0),
                y: (0.0, 1.0),
                radius: (0.1, 0.2),
            },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：95% 海洋，5% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.95 },
        ])
    }
    
    // ============================================================================
    // 基于图元的新模板 (Primitive-based templates)
    // ============================================================================
    
    /// 板块碰撞地形 - 使用图元组合
    pub fn tectonic_collision() -> Self {
        use super::primitive::*;
        
        Self::new(
            "Tectonic Collision",
            "板块碰撞形成的山脉和海沟",
        )
        .with_primitives(vec![
            // 两个大陆核心
            TerrainPrimitive::ContinentCore {
                size: Size::Large,
                elevation: Elevation::Medium,
                position: PositionConstraint { x: (0.1, 0.4), y: (0.2, 0.8) },
            },
            TerrainPrimitive::ContinentCore {
                size: Size::Large,
                elevation: Elevation::Medium,
                position: PositionConstraint { x: (0.6, 0.9), y: (0.2, 0.8) },
            },
            // 碰撞形成的山脉（喜马拉雅式）
            TerrainPrimitive::MountainChain {
                size: Size::Large,
                elevation: Elevation::Extreme,
                count: 2,
                position: PositionConstraint { x: (0.4, 0.6), y: (0.2, 0.8) },
            },
            // 次级山脉
            TerrainPrimitive::MountainChain {
                size: Size::Medium,
                elevation: Elevation::High,
                count: 4,
                position: PositionConstraint::default(),
            },
            // 高原
            TerrainPrimitive::Plateau {
                size: Size::Medium,
                elevation: Elevation::Medium,
                position: PositionConstraint::center(),
            },
            // 深海沟
            TerrainPrimitive::OceanTrench {
                size: Size::Large,
                depth: Elevation::High,
                position: PositionConstraint { x: (0.0, 0.2), y: (0.0, 1.0) },
            },
        ])
        .with_command(TerrainCommand::Normalize)
        .with_command(TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.55 })
    }
    
    /// 火山群岛 - 使用图元组合
    pub fn volcanic_archipelago() -> Self {
        use super::primitive::*;
        
        Self::new(
            "Volcanic Archipelago",
            "火山活动形成的岛链",
        )
        .with_primitives(vec![
            // 主火山岛
            TerrainPrimitive::Volcano {
                size: Size::Large,
                elevation: Elevation::Extreme,
                has_crater: true,
                position: PositionConstraint::center(),
            },
            // 次级火山
            TerrainPrimitive::Volcano {
                size: Size::Medium,
                elevation: Elevation::High,
                has_crater: true,
                position: PositionConstraint::default(),
            },
            TerrainPrimitive::Volcano {
                size: Size::Small,
                elevation: Elevation::Medium,
                has_crater: false,
                position: PositionConstraint::default(),
            },
            // 周围小岛
            TerrainPrimitive::Archipelago {
                island_count: 12,
                island_size: Size::Tiny,
                spread: Size::Large,
                position: PositionConstraint::default(),
            },
            // 深海沟（俯冲带）
            TerrainPrimitive::OceanTrench {
                size: Size::Large,
                depth: Elevation::High,
                position: PositionConstraint::edge(),
            },
            // 深海平原
            TerrainPrimitive::AbyssalPlain {
                size: Size::Large,
                count: 6,
                position: PositionConstraint::default(),
            },
        ])
        .with_command(TerrainCommand::Normalize)
        .with_command(TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.85 })
    }
    
    /// 峡湾海岸 - 使用图元组合
    pub fn fjord_coast() -> Self {
        use super::primitive::*;
        
        Self::new(
            "Fjord Coast",
            "冰川侵蚀形成的峡湾海岸",
        )
        .with_primitives(vec![
            // 沿海山脉
            TerrainPrimitive::MountainChain {
                size: Size::Large,
                elevation: Elevation::High,
                count: 3,
                position: PositionConstraint { x: (0.0, 0.5), y: (0.0, 1.0) },
            },
            // 多条峡湾
            TerrainPrimitive::Fjord {
                size: Size::Medium,
                depth: Elevation::Medium,
                position: PositionConstraint { x: (0.2, 0.6), y: (0.1, 0.3) },
            },
            TerrainPrimitive::Fjord {
                size: Size::Medium,
                depth: Elevation::Medium,
                position: PositionConstraint { x: (0.2, 0.6), y: (0.4, 0.6) },
            },
            TerrainPrimitive::Fjord {
                size: Size::Small,
                depth: Elevation::Low,
                position: PositionConstraint { x: (0.2, 0.6), y: (0.7, 0.9) },
            },
            // 高原内陆
            TerrainPrimitive::Plateau {
                size: Size::Large,
                elevation: Elevation::Medium,
                position: PositionConstraint { x: (0.0, 0.4), y: (0.2, 0.8) },
            },
            // 近海岛屿
            TerrainPrimitive::Archipelago {
                island_count: 8,
                island_size: Size::Small,
                spread: Size::Medium,
                position: PositionConstraint { x: (0.5, 0.8), y: (0.0, 1.0) },
            },
        ])
        .with_command(TerrainCommand::Normalize)
        .with_command(TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.60 })
    }
    
    /// 大裂谷 - 使用图元组合
    pub fn rift_valley() -> Self {
        use super::primitive::*;
        
        Self::new(
            "Rift Valley",
            "大陆裂谷和火山活动",
        )
        .with_primitives(vec![
            // 大陆高原
            TerrainPrimitive::ContinentCore {
                size: Size::Huge,
                elevation: Elevation::Medium,
                position: PositionConstraint::center(),
            },
            // 中央裂谷
            TerrainPrimitive::Rift {
                size: Size::Large,
                depth: Elevation::Medium,
                position: PositionConstraint::center(),
            },
            // 裂谷两侧的山脉
            TerrainPrimitive::MountainChain {
                size: Size::Medium,
                elevation: Elevation::High,
                count: 2,
                position: PositionConstraint { x: (0.3, 0.45), y: (0.1, 0.9) },
            },
            TerrainPrimitive::MountainChain {
                size: Size::Medium,
                elevation: Elevation::High,
                count: 2,
                position: PositionConstraint { x: (0.55, 0.7), y: (0.1, 0.9) },
            },
            // 火山
            TerrainPrimitive::Volcano {
                size: Size::Medium,
                elevation: Elevation::High,
                has_crater: true,
                position: PositionConstraint::center(),
            },
            // 裂谷湖泊
            TerrainPrimitive::Basin {
                size: Size::Medium,
                depth: Elevation::Low,
                position: PositionConstraint { x: (0.45, 0.55), y: (0.3, 0.5) },
            },
            TerrainPrimitive::Basin {
                size: Size::Small,
                depth: Elevation::Low,
                position: PositionConstraint { x: (0.45, 0.55), y: (0.6, 0.8) },
            },
        ])
        .with_command(TerrainCommand::Normalize)
        .with_command(TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.25 })
    }
}

/// 获取所有预设模板
pub fn get_preset_templates() -> Vec<TerrainTemplate> {
    vec![
        // 传统模板
        TerrainTemplate::earth_like(),
        TerrainTemplate::archipelago(),
        TerrainTemplate::continental(),
        TerrainTemplate::volcanic_island(),
        TerrainTemplate::atoll(),
        TerrainTemplate::peninsula(),
        TerrainTemplate::highland(),
        TerrainTemplate::oceanic(),
        // 基于图元的新模板
        TerrainTemplate::tectonic_collision(),
        TerrainTemplate::volcanic_archipelago(),
        TerrainTemplate::fjord_coast(),
        TerrainTemplate::rift_valley(),
    ]
}

/// 根据名称获取预设模板
pub fn get_template_by_name(name: &str) -> Option<TerrainTemplate> {
    match name.to_lowercase().as_str() {
        // 传统模板
        "earth-like" | "earth_like" => Some(TerrainTemplate::earth_like()),
        "archipelago" => Some(TerrainTemplate::archipelago()),
        "continental" => Some(TerrainTemplate::continental()),
        "volcanic_island" | "volcanic-island" => Some(TerrainTemplate::volcanic_island()),
        "atoll" => Some(TerrainTemplate::atoll()),
        "peninsula" => Some(TerrainTemplate::peninsula()),
        "highland" => Some(TerrainTemplate::highland()),
        "oceanic" => Some(TerrainTemplate::oceanic()),
        // 基于图元的新模板
        "tectonic_collision" | "tectonic-collision" => Some(TerrainTemplate::tectonic_collision()),
        "volcanic_archipelago" | "volcanic-archipelago" => Some(TerrainTemplate::volcanic_archipelago()),
        "fjord_coast" | "fjord-coast" => Some(TerrainTemplate::fjord_coast()),
        "rift_valley" | "rift-valley" => Some(TerrainTemplate::rift_valley()),
        _ => None,
    }
}
