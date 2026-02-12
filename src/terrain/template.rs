// 地形模板系统 - 参考 Azgaar's Fantasy Map Generator
//
// 模板是一组操作指令，用于生成特定类型的地形。
// 每个模板定义了一系列的地形修改命令，可以产生可预测但仍具有随机性的地图。

use std::f32::consts::PI;

/// 地形修改命令
#[derive(Debug, Clone)]
pub enum TerrainCommand {
    /// 山脉 - 单个大型中心凸起
    Mountain {
        height: f32, // 高度 (0-255)
        x: f32,      // X 位置 (0.0-1.0)
        y: f32,      // Y 位置 (0.0-1.0)
        radius: f32, // 半径 (0.0-1.0)
    },

    /// 丘陵 - 圆形隆起
    Hill {
        count: u32,         // 数量
        height: (f32, f32), // 高度范围 (min, max)
        x: (f32, f32),      // X 位置范围 (0.0-1.0)
        y: (f32, f32),      // Y 位置范围 (0.0-1.0)
        radius: (f32, f32), // 半径范围 (0.0-1.0)
    },

    /// 有边界的丘陵 - BFS 扩散被限制在指定区域内
    /// 用于生成相互独立的大陆，不会跨越边界融合
    BoundedHill {
        count: u32,
        height: (f32, f32),
        x: (f32, f32),                // 丘陵中心的 X 范围
        y: (f32, f32),                // 丘陵中心的 Y 范围
        bounds: (f32, f32, f32, f32), // 扩散边界 (min_x, max_x, min_y, max_y)
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
        length: (f32, f32), // 长度 (0.0-1.0)
        width: (f32, f32),  // 宽度 (0.0-1.0)
        angle: (f32, f32),  // 角度（弧度）
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
        width: f32, // 宽度 (0.0-1.0)
        direction: StraitDirection,
        position: f32, // 位置 (0.0-1.0)
        depth: f32,    // 深度
    },

    /// 添加 - 为所有单元格添加固定高度值
    Add {
        value: f32, // 可以是负值以降低高度
    },

    /// 乘法 - 将所有高度值乘以系数
    Multiply { factor: f32 },

    /// 平滑 - 平均周围单元格的高度
    Smooth { iterations: u32 },

    /// 侵蚀 - 基于坡度搬运沉积物，模拟水蚀对地形的重塑
    Erode {
        iterations: u32, // 迭代轮数
        rain: f32,       // 每轮降雨量（0.0-1.0）
        capacity: f32,   // 搬运能力系数（0.0-2.0）
        deposition: f32, // 沉积比例（0.0-1.0）
    },

    /// 遮罩 - 应用边缘或中心渐变效果
    Mask {
        mode: MaskMode,
        strength: f32, // 强度 (0.0-1.0)
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
        level: f32, // 海平面高度 (0-255)
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
    pub fn from_dsl(
        name: &str,
        description: &str,
        dsl: &str,
    ) -> Result<Self, super::dsl::ParseError> {
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
        Self::new("Earth-like", "平衡的大陆和海洋配置，约 30% 陆地").with_commands(vec![
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
            TerrainCommand::Erode {
                iterations: 4,
                rain: 0.32,
                capacity: 0.7,
                deposition: 0.5,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.7 },
        ])
    }

    /// 群岛 - 许多小岛屿（约 10-20% 陆地）
    pub fn archipelago() -> Self {
        Self::new("Archipelago", "众多小岛屿分布在广阔海洋中").with_commands(vec![
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
        Self::new("Continental", "一到两个大型大陆").with_commands(vec![
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
            TerrainCommand::Erode {
                iterations: 3,
                rain: 0.28,
                capacity: 0.6,
                deposition: 0.45,
            },
            // 归一化
            TerrainCommand::Normalize,
            // 调整海陆比例：55% 海洋，45% 陆地
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.55 },
        ])
    }

    /// 火山岛 - 单个高山岛屿
    pub fn volcanic_island() -> Self {
        Self::new("Volcanic Island", "单个高耸的火山岛").with_commands(vec![
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
        Self::new("Atoll", "环形珊瑚礁岛屿围绕浅水泻湖").with_commands(vec![
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
        Self::new("Peninsula", "从地图一侧延伸的半岛").with_commands(vec![
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
        Self::new("Highland", "高原和山地主导的地形").with_commands(vec![
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
        Self::new("Oceanic", "广阔的海洋，少量岛屿").with_commands(vec![
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
    // Azgaar 风格模板 (Azgaar-style templates)
    // 参考 Azgaar's Fantasy Map Generator
    // ============================================================================

    /// 火山岛 - 中心高耸的火山（参考 Azgaar volcano 模板）
    pub fn volcano() -> Self {
        Self::new("Volcano", "孤立的火山岛，中央高耸").with_commands(vec![
            // 中央主火山 - 非常高
            TerrainCommand::Mountain {
                height: 220.0,
                x: 0.5,
                y: 0.5,
                radius: 0.12,
            },
            // 火山外沿的较低区域
            TerrainCommand::Multiply { factor: 0.8 },
            // 周围山脊
            TerrainCommand::Range {
                count: 1,
                height: (80.0, 100.0),
                x: (0.3, 0.55),
                y: (0.45, 0.55),
                length: (0.25, 0.35),
                width: (0.05, 0.08),
                angle: (0.0, PI),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 3 },
            // 次级丘陵
            TerrainCommand::Hill {
                count: 1,
                height: (60.0, 80.0),
                x: (0.35, 0.45),
                y: (0.25, 0.30),
                radius: (0.06, 0.1),
            },
            TerrainCommand::Hill {
                count: 1,
                height: (50.0, 70.0),
                x: (0.75, 0.80),
                y: (0.25, 0.75),
                radius: (0.04, 0.08),
            },
            TerrainCommand::Hill {
                count: 1,
                height: (40.0, 60.0),
                x: (0.10, 0.15),
                y: (0.20, 0.25),
                radius: (0.03, 0.06),
            },
            // 遮罩 - 边缘降低
            TerrainCommand::Mask {
                mode: MaskMode::EdgeFade,
                strength: 0.6,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.88 },
        ])
    }

    /// 高岛 - 有山脉的大岛（参考 Azgaar highIsland 模板）
    pub fn high_island() -> Self {
        Self::new("High Island", "大型岛屿，有复杂的山脉系统").with_commands(vec![
            // 主丘陵核心
            TerrainCommand::Mountain {
                height: 120.0,
                x: 0.65,
                y: 0.5,
                radius: 0.18,
            },
            // 添加基础高度
            TerrainCommand::Add { value: 7.0 },
            // 多个山丘
            TerrainCommand::Hill {
                count: 5,
                height: (50.0, 80.0),
                x: (0.25, 0.55),
                y: (0.45, 0.55),
                radius: (0.08, 0.12),
            },
            // 山脉
            TerrainCommand::Range {
                count: 1,
                height: (80.0, 100.0),
                x: (0.45, 0.55),
                y: (0.45, 0.55),
                length: (0.3, 0.4),
                width: (0.04, 0.06),
                angle: (0.0, PI),
            },
            // 降低陆地
            TerrainCommand::Multiply { factor: 0.8 },
            // 遮罩
            TerrainCommand::Mask {
                mode: MaskMode::EdgeFade,
                strength: 0.5,
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 2 },
            // 海沟
            TerrainCommand::Trough {
                count: 2,
                depth: (30.0, 45.0),
                x: (0.20, 0.30),
                y: (0.20, 0.30),
                length: (0.15, 0.25),
                width: (0.02, 0.04),
                angle: (0.0, PI),
            },
            TerrainCommand::Trough {
                count: 2,
                depth: (30.0, 45.0),
                x: (0.60, 0.80),
                y: (0.70, 0.80),
                length: (0.15, 0.25),
                width: (0.02, 0.04),
                angle: (0.0, PI),
            },
            // 额外的丘陵
            TerrainCommand::Hill {
                count: 1,
                height: (45.0, 60.0),
                x: (0.60, 0.60),
                y: (0.50, 0.50),
                radius: (0.06, 0.09),
            },
            TerrainCommand::Hill {
                count: 1,
                height: (50.0, 65.0),
                x: (0.15, 0.20),
                y: (0.20, 0.75),
                radius: (0.05, 0.08),
            },
            // 次级山脉
            TerrainCommand::Range {
                count: 1,
                height: (60.0, 80.0),
                x: (0.15, 0.85),
                y: (0.30, 0.40),
                length: (0.25, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            TerrainCommand::Range {
                count: 1,
                height: (60.0, 80.0),
                x: (0.15, 0.85),
                y: (0.60, 0.70),
                length: (0.25, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 坑洞/湖泊
            TerrainCommand::Pit {
                count: 4,
                depth: (25.0, 40.0),
                x: (0.15, 0.85),
                y: (0.20, 0.80),
                radius: (0.04, 0.08),
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.65 },
        ])
    }

    /// 低岛 - 平坦的岛屿（参考 Azgaar lowIsland 模板）
    pub fn low_island() -> Self {
        Self::new("Low Island", "低矮平坦的大型岛屿").with_commands(vec![
            // 主体
            TerrainCommand::Mountain {
                height: 100.0,
                x: 0.7,
                y: 0.5,
                radius: 0.2,
            },
            // 侧翼丘陵
            TerrainCommand::Hill {
                count: 2,
                height: (50.0, 70.0),
                x: (0.10, 0.30),
                y: (0.10, 0.90),
                radius: (0.08, 0.12),
            },
            TerrainCommand::Smooth { iterations: 2 },
            // 主体丘陵
            TerrainCommand::Hill {
                count: 7,
                height: (55.0, 75.0),
                x: (0.20, 0.70),
                y: (0.30, 0.70),
                radius: (0.08, 0.12),
            },
            // 山脉
            TerrainCommand::Range {
                count: 1,
                height: (70.0, 90.0),
                x: (0.45, 0.55),
                y: (0.45, 0.55),
                length: (0.3, 0.4),
                width: (0.04, 0.06),
                angle: (0.0, PI),
            },
            // 海沟
            TerrainCommand::Trough {
                count: 2,
                depth: (30.0, 45.0),
                x: (0.15, 0.85),
                y: (0.20, 0.30),
                length: (0.15, 0.25),
                width: (0.02, 0.04),
                angle: (0.0, PI),
            },
            TerrainCommand::Trough {
                count: 2,
                depth: (30.0, 45.0),
                x: (0.15, 0.85),
                y: (0.70, 0.80),
                length: (0.15, 0.25),
                width: (0.02, 0.04),
                angle: (0.0, PI),
            },
            // 边缘丘陵
            TerrainCommand::Hill {
                count: 1,
                height: (45.0, 60.0),
                x: (0.05, 0.15),
                y: (0.20, 0.80),
                radius: (0.05, 0.08),
            },
            TerrainCommand::Hill {
                count: 1,
                height: (45.0, 60.0),
                x: (0.85, 0.95),
                y: (0.70, 0.80),
                radius: (0.05, 0.08),
            },
            // 坑洞/湖泊
            TerrainCommand::Pit {
                count: 6,
                depth: (25.0, 40.0),
                x: (0.15, 0.85),
                y: (0.20, 0.80),
                radius: (0.04, 0.08),
            },
            // 大幅降低高度使其变得平坦
            TerrainCommand::Multiply { factor: 0.4 },
            TerrainCommand::Mask {
                mode: MaskMode::EdgeFade,
                strength: 0.6,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.70 },
        ])
    }

    /// 多大陆 - 多个大陆块（使用 BoundedHill 实现自然分离）
    pub fn continents() -> Self {
        Self::new("Continents", "多个分散的大陆").with_commands(vec![
            // === 西大陆（左侧 0.0-0.45）===
            // 核心
            TerrainCommand::Mountain {
                height: 85.0,
                x: 0.22,
                y: 0.50,
                radius: 0.10,
            },
            // 扩展丘陵（限制在左侧区域内）
            TerrainCommand::BoundedHill {
                count: 8,
                height: (50.0, 75.0),
                x: (0.08, 0.38),
                y: (0.15, 0.85),
                bounds: (0.0, 0.45, 0.0, 1.0), // 不会扩散到 x > 0.45
            },
            // === 东大陆（右侧 0.55-1.0）===
            // 核心
            TerrainCommand::Mountain {
                height: 85.0,
                x: 0.78,
                y: 0.50,
                radius: 0.10,
            },
            // 扩展丘陵（限制在右侧区域内）
            TerrainCommand::BoundedHill {
                count: 8,
                height: (50.0, 75.0),
                x: (0.62, 0.92),
                y: (0.15, 0.85),
                bounds: (0.55, 1.0, 0.0, 1.0), // 不会扩散到 x < 0.55
            },
            // 降低整体
            TerrainCommand::Multiply { factor: 0.55 },
            // === 细节丘陵（同样带边界）===
            // 西大陆细节
            TerrainCommand::BoundedHill {
                count: 5,
                height: (25.0, 45.0),
                x: (0.05, 0.40),
                y: (0.10, 0.90),
                bounds: (0.0, 0.45, 0.0, 1.0),
            },
            // 东大陆细节
            TerrainCommand::BoundedHill {
                count: 5,
                height: (25.0, 45.0),
                x: (0.60, 0.95),
                y: (0.10, 0.90),
                bounds: (0.55, 1.0, 0.0, 1.0),
            },
            // === 山脉 ===
            // 西大陆山脉
            TerrainCommand::Range {
                count: 2,
                height: (55.0, 75.0),
                x: (0.10, 0.35),
                y: (0.25, 0.75),
                length: (0.15, 0.28),
                width: (0.02, 0.03),
                angle: (PI * 0.3, PI * 0.7),
            },
            // 东大陆山脉
            TerrainCommand::Range {
                count: 2,
                height: (55.0, 75.0),
                x: (0.65, 0.90),
                y: (0.25, 0.75),
                length: (0.15, 0.28),
                width: (0.02, 0.03),
                angle: (PI * 0.3, PI * 0.7),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 2 },
            // 边缘淡出
            TerrainCommand::Mask {
                mode: MaskMode::EdgeFade,
                strength: 0.5,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.58 },
        ])
    }

    /// 群岛增强版 - 很多小岛（参考 Azgaar archipelago 模板）
    pub fn archipelago_azgaar() -> Self {
        Self::new("Archipelago (Azgaar)", "众多分散的小岛，复杂的岛链").with_commands(vec![
            // 基础高度
            TerrainCommand::Add { value: 11.0 },
            // 主要山脉
            TerrainCommand::Range {
                count: 2,
                height: (80.0, 120.0),
                x: (0.20, 0.80),
                y: (0.20, 0.80),
                length: (0.35, 0.5),
                width: (0.04, 0.06),
                angle: (0.0, PI),
            },
            // 主丘陵
            TerrainCommand::Hill {
                count: 5,
                height: (50.0, 70.0),
                x: (0.10, 0.90),
                y: (0.30, 0.70),
                radius: (0.06, 0.1),
            },
            // 左侧丘陵
            TerrainCommand::Hill {
                count: 2,
                height: (45.0, 60.0),
                x: (0.10, 0.30),
                y: (0.20, 0.80),
                radius: (0.05, 0.08),
            },
            // 右侧丘陵
            TerrainCommand::Hill {
                count: 2,
                height: (45.0, 60.0),
                x: (0.60, 0.90),
                y: (0.20, 0.80),
                radius: (0.05, 0.08),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 3 },
            // 深海沟分隔
            TerrainCommand::Trough {
                count: 10,
                depth: (40.0, 60.0),
                x: (0.05, 0.95),
                y: (0.05, 0.95),
                length: (0.25, 0.4),
                width: (0.03, 0.05),
                angle: (0.0, 2.0 * PI),
            },
            // 垂直海峡
            TerrainCommand::Strait {
                width: 0.05,
                direction: StraitDirection::Vertical,
                position: 0.5,
                depth: 35.0,
            },
            // 水平海峡
            TerrainCommand::Strait {
                width: 0.05,
                direction: StraitDirection::Horizontal,
                position: 0.5,
                depth: 35.0,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.80 },
        ])
    }

    /// 环礁增强版 - 环形岛屿（参考 Azgaar atoll 模板）
    pub fn atoll_azgaar() -> Self {
        Self::new("Atoll (Azgaar)", "环形珊瑚岛围绕浅泻湖").with_commands(vec![
            // 中央凸起
            TerrainCommand::Mountain {
                height: 85.0,
                x: 0.55,
                y: 0.5,
                radius: 0.12,
            },
            // 环形丘陵
            TerrainCommand::Hill {
                count: 2,
                height: (60.0, 90.0),
                x: (0.25, 0.75),
                y: (0.30, 0.70),
                radius: (0.12, 0.18),
            },
            // 西侧延伸
            TerrainCommand::Hill {
                count: 1,
                height: (60.0, 90.0),
                x: (0.25, 0.35),
                y: (0.30, 0.70),
                radius: (0.1, 0.15),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 1 },
            // 降低外环使其变得很低
            TerrainCommand::Multiply { factor: 0.2 },
            // 中央泻湖小凸起
            TerrainCommand::Hill {
                count: 1,
                height: (30.0, 50.0),
                x: (0.50, 0.55),
                y: (0.48, 0.52),
                radius: (0.04, 0.07),
            },
            TerrainCommand::Mask {
                mode: MaskMode::CenterBoost,
                strength: 0.4,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.92 },
        ])
    }

    /// 地中海式 - 内海（参考 Azgaar mediterranean 模板）
    pub fn mediterranean() -> Self {
        Self::new("Mediterranean", "被陆地包围的内海").with_commands(vec![
            // 北部海岸山脉
            TerrainCommand::Range {
                count: 5,
                height: (70.0, 120.0),
                x: (0.0, 1.0),
                y: (0.0, 0.10),
                length: (0.25, 0.45),
                width: (0.04, 0.08),
                angle: (PI * 0.4, PI * 0.6),
            },
            // 南部海岸山脉
            TerrainCommand::Range {
                count: 5,
                height: (70.0, 120.0),
                x: (0.0, 1.0),
                y: (0.90, 1.0),
                length: (0.25, 0.45),
                width: (0.04, 0.08),
                angle: (PI * 0.4, PI * 0.6),
            },
            // 北部丘陵
            TerrainCommand::Hill {
                count: 7,
                height: (70.0, 100.0),
                x: (0.10, 0.90),
                y: (0.0, 0.05),
                radius: (0.1, 0.15),
            },
            // 南部丘陵
            TerrainCommand::Hill {
                count: 7,
                height: (70.0, 100.0),
                x: (0.10, 0.90),
                y: (0.95, 1.0),
                radius: (0.1, 0.15),
            },
            // 降低陆地
            TerrainCommand::Multiply { factor: 0.9 },
            // 边缘遮罩（反向 - 中央凹陷）
            TerrainCommand::Mask {
                mode: MaskMode::RadialGradient,
                strength: -0.4,
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 1 },
            // 西部延伸
            TerrainCommand::Hill {
                count: 2,
                height: (65.0, 95.0),
                x: (0.0, 0.05),
                y: (0.20, 0.80),
                radius: (0.1, 0.15),
            },
            // 东部延伸
            TerrainCommand::Hill {
                count: 2,
                height: (65.0, 95.0),
                x: (0.95, 1.0),
                y: (0.20, 0.80),
                radius: (0.1, 0.15),
            },
            // 内海海沟（加深中央区域）
            TerrainCommand::Trough {
                count: 4,
                depth: (50.0, 70.0),
                x: (0.0, 1.0),
                y: (0.0, 0.10),
                length: (0.3, 0.5),
                width: (0.03, 0.06),
                angle: (PI * 0.4, PI * 0.6),
            },
            TerrainCommand::Trough {
                count: 4,
                depth: (50.0, 70.0),
                x: (0.0, 1.0),
                y: (0.90, 1.0),
                length: (0.3, 0.5),
                width: (0.03, 0.06),
                angle: (PI * 0.4, PI * 0.6),
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.55 },
        ])
    }

    /// 半岛增强版 - 延伸的陆地（参考 Azgaar peninsula 模板）
    pub fn peninsula_azgaar() -> Self {
        Self::new("Peninsula (Azgaar)", "从大陆延伸的细长半岛").with_commands(vec![
            // 北部主体山脉
            TerrainCommand::Range {
                count: 2,
                height: (60.0, 80.0),
                x: (0.40, 0.50),
                y: (0.0, 0.15),
                length: (0.25, 0.4),
                width: (0.06, 0.1),
                angle: (0.0, PI * 0.3),
            },
            // 基础高度
            TerrainCommand::Add { value: 5.0 },
            // 北部大陆
            TerrainCommand::Mountain {
                height: 120.0,
                x: 0.5,
                y: 0.03,
                radius: 0.25,
            },
            // 添加更多高度
            TerrainCommand::Add { value: 13.0 },
            // 南部延伸丘陵
            TerrainCommand::Hill {
                count: 4,
                height: (30.0, 50.0),
                x: (0.05, 0.95),
                y: (0.80, 1.0),
                radius: (0.04, 0.07),
            },
            // 中部连接丘陵
            TerrainCommand::Hill {
                count: 2,
                height: (30.0, 50.0),
                x: (0.05, 0.95),
                y: (0.40, 0.60),
                radius: (0.04, 0.07),
            },
            // 海沟分隔
            TerrainCommand::Trough {
                count: 5,
                depth: (35.0, 55.0),
                x: (0.05, 0.95),
                y: (0.05, 0.95),
                length: (0.2, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 3 },
            // 反转（上下翻转使半岛向下延伸）
            TerrainCommand::Invert {
                axis: InvertAxis::Y,
                probability: 0.4,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.65 },
        ])
    }

    /// 盘古大陆 - 单一超级大陆（参考 Azgaar pangea 模板）
    pub fn pangea() -> Self {
        Self::new("Pangea", "单一的超级大陆").with_commands(vec![
            // 西北角大陆
            TerrainCommand::Hill {
                count: 2,
                height: (60.0, 90.0),
                x: (0.15, 0.50),
                y: (0.0, 0.10),
                radius: (0.1, 0.16),
            },
            // 东北角大陆
            TerrainCommand::Hill {
                count: 2,
                height: (30.0, 70.0),
                x: (0.50, 0.85),
                y: (0.0, 0.10),
                radius: (0.08, 0.14),
            },
            // 东南角大陆
            TerrainCommand::Hill {
                count: 2,
                height: (60.0, 90.0),
                x: (0.50, 0.85),
                y: (0.90, 1.0),
                radius: (0.1, 0.16),
            },
            // 西南角大陆
            TerrainCommand::Hill {
                count: 2,
                height: (30.0, 70.0),
                x: (0.15, 0.50),
                y: (0.90, 1.0),
                radius: (0.08, 0.14),
            },
            // 中央大陆核心
            TerrainCommand::Hill {
                count: 10,
                height: (60.0, 90.0),
                x: (0.20, 0.80),
                y: (0.48, 0.52),
                radius: (0.1, 0.16),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 2 },
            // 降低陆地
            TerrainCommand::Multiply { factor: 0.7 },
            // 北部海沟
            TerrainCommand::Trough {
                count: 3,
                depth: (45.0, 65.0),
                x: (0.05, 0.95),
                y: (0.10, 0.20),
                length: (0.25, 0.4),
                width: (0.03, 0.05),
                angle: (PI * 0.3, PI * 0.7),
            },
            // 南部海沟
            TerrainCommand::Trough {
                count: 3,
                depth: (45.0, 65.0),
                x: (0.05, 0.95),
                y: (0.80, 0.90),
                length: (0.25, 0.4),
                width: (0.03, 0.05),
                angle: (PI * 0.3, PI * 0.7),
            },
            // 中央山脉
            TerrainCommand::Range {
                count: 5,
                height: (70.0, 90.0),
                x: (0.10, 0.90),
                y: (0.35, 0.65),
                length: (0.25, 0.4),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.45 },
        ])
    }

    /// 地峡 - 连接两块陆地的狭窄地带（参考 Azgaar isthmus 模板）
    pub fn isthmus() -> Self {
        Self::new("Isthmus", "连接两块大陆的狭窄地峡").with_commands(vec![
            // 西北大陆块
            TerrainCommand::Hill {
                count: 8,
                height: (50.0, 75.0),
                x: (0.0, 0.30),
                y: (0.0, 0.20),
                radius: (0.08, 0.14),
            },
            // 西侧大陆延伸
            TerrainCommand::Hill {
                count: 8,
                height: (50.0, 75.0),
                x: (0.10, 0.50),
                y: (0.20, 0.40),
                radius: (0.08, 0.14),
            },
            // 中央连接带
            TerrainCommand::Hill {
                count: 8,
                height: (50.0, 75.0),
                x: (0.30, 0.70),
                y: (0.40, 0.60),
                radius: (0.08, 0.14),
            },
            // 东侧大陆延伸
            TerrainCommand::Hill {
                count: 8,
                height: (50.0, 75.0),
                x: (0.50, 0.90),
                y: (0.60, 0.80),
                radius: (0.08, 0.14),
            },
            // 东南大陆块
            TerrainCommand::Hill {
                count: 8,
                height: (50.0, 75.0),
                x: (0.70, 1.0),
                y: (0.80, 1.0),
                radius: (0.08, 0.14),
            },
            // 平滑
            TerrainCommand::Smooth { iterations: 2 },
            // 海沟分隔（西北）
            TerrainCommand::Trough {
                count: 5,
                depth: (40.0, 60.0),
                x: (0.0, 0.30),
                y: (0.0, 0.20),
                length: (0.2, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 海沟分隔（中西）
            TerrainCommand::Trough {
                count: 5,
                depth: (40.0, 60.0),
                x: (0.10, 0.50),
                y: (0.20, 0.40),
                length: (0.2, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 海沟分隔（中央）
            TerrainCommand::Trough {
                count: 5,
                depth: (40.0, 60.0),
                x: (0.30, 0.70),
                y: (0.40, 0.60),
                length: (0.2, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 海沟分隔（中东）
            TerrainCommand::Trough {
                count: 5,
                depth: (40.0, 60.0),
                x: (0.50, 0.90),
                y: (0.60, 0.80),
                length: (0.2, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 海沟分隔（东南）
            TerrainCommand::Trough {
                count: 5,
                depth: (40.0, 60.0),
                x: (0.70, 1.0),
                y: (0.80, 1.0),
                length: (0.2, 0.35),
                width: (0.03, 0.05),
                angle: (0.0, PI),
            },
            // 反转（沿X轴）
            TerrainCommand::Invert {
                axis: InvertAxis::X,
                probability: 0.25,
            },
            TerrainCommand::Normalize,
            TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.55 },
        ])
    }

    // ============================================================================
    // 基于图元的新模板 (Primitive-based templates)
    // ============================================================================

    /// 板块碰撞地形 - 使用图元组合
    pub fn tectonic_collision() -> Self {
        use super::primitive::*;

        Self::new("Tectonic Collision", "板块碰撞形成的山脉和海沟")
            .with_primitives(vec![
                // 两个大陆核心
                TerrainPrimitive::ContinentCore {
                    size: Size::Large,
                    elevation: Elevation::Medium,
                    position: PositionConstraint {
                        x: (0.1, 0.4),
                        y: (0.2, 0.8),
                    },
                },
                TerrainPrimitive::ContinentCore {
                    size: Size::Large,
                    elevation: Elevation::Medium,
                    position: PositionConstraint {
                        x: (0.6, 0.9),
                        y: (0.2, 0.8),
                    },
                },
                // 碰撞形成的山脉（喜马拉雅式）
                TerrainPrimitive::MountainChain {
                    size: Size::Large,
                    elevation: Elevation::Extreme,
                    count: 2,
                    position: PositionConstraint {
                        x: (0.4, 0.6),
                        y: (0.2, 0.8),
                    },
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
                    position: PositionConstraint {
                        x: (0.0, 0.2),
                        y: (0.0, 1.0),
                    },
                },
            ])
            .with_command(TerrainCommand::Normalize)
            .with_command(TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.55 })
    }

    /// 火山群岛 - 使用图元组合
    pub fn volcanic_archipelago() -> Self {
        use super::primitive::*;

        Self::new("Volcanic Archipelago", "火山活动形成的岛链")
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

        Self::new("Fjord Coast", "冰川侵蚀形成的峡湾海岸")
            .with_primitives(vec![
                // 沿海山脉
                TerrainPrimitive::MountainChain {
                    size: Size::Large,
                    elevation: Elevation::High,
                    count: 3,
                    position: PositionConstraint {
                        x: (0.0, 0.5),
                        y: (0.0, 1.0),
                    },
                },
                // 多条峡湾
                TerrainPrimitive::Fjord {
                    size: Size::Medium,
                    depth: Elevation::Medium,
                    position: PositionConstraint {
                        x: (0.2, 0.6),
                        y: (0.1, 0.3),
                    },
                },
                TerrainPrimitive::Fjord {
                    size: Size::Medium,
                    depth: Elevation::Medium,
                    position: PositionConstraint {
                        x: (0.2, 0.6),
                        y: (0.4, 0.6),
                    },
                },
                TerrainPrimitive::Fjord {
                    size: Size::Small,
                    depth: Elevation::Low,
                    position: PositionConstraint {
                        x: (0.2, 0.6),
                        y: (0.7, 0.9),
                    },
                },
                // 高原内陆
                TerrainPrimitive::Plateau {
                    size: Size::Large,
                    elevation: Elevation::Medium,
                    position: PositionConstraint {
                        x: (0.0, 0.4),
                        y: (0.2, 0.8),
                    },
                },
                // 近海岛屿
                TerrainPrimitive::Archipelago {
                    island_count: 8,
                    island_size: Size::Small,
                    spread: Size::Medium,
                    position: PositionConstraint {
                        x: (0.5, 0.8),
                        y: (0.0, 1.0),
                    },
                },
            ])
            .with_command(TerrainCommand::Normalize)
            .with_command(TerrainCommand::AdjustSeaRatio { ocean_ratio: 0.60 })
    }

    /// 大裂谷 - 使用图元组合
    pub fn rift_valley() -> Self {
        use super::primitive::*;

        Self::new("Rift Valley", "大陆裂谷和火山活动")
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
                    position: PositionConstraint {
                        x: (0.3, 0.45),
                        y: (0.1, 0.9),
                    },
                },
                TerrainPrimitive::MountainChain {
                    size: Size::Medium,
                    elevation: Elevation::High,
                    count: 2,
                    position: PositionConstraint {
                        x: (0.55, 0.7),
                        y: (0.1, 0.9),
                    },
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
                    position: PositionConstraint {
                        x: (0.45, 0.55),
                        y: (0.3, 0.5),
                    },
                },
                TerrainPrimitive::Basin {
                    size: Size::Small,
                    depth: Elevation::Low,
                    position: PositionConstraint {
                        x: (0.45, 0.55),
                        y: (0.6, 0.8),
                    },
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
        // Azgaar 风格模板
        TerrainTemplate::volcano(),
        TerrainTemplate::high_island(),
        TerrainTemplate::low_island(),
        TerrainTemplate::continents(),
        TerrainTemplate::archipelago_azgaar(),
        TerrainTemplate::atoll_azgaar(),
        TerrainTemplate::mediterranean(),
        TerrainTemplate::peninsula_azgaar(),
        TerrainTemplate::pangea(),
        TerrainTemplate::isthmus(),
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
        // Azgaar 风格模板
        "volcano" => Some(TerrainTemplate::volcano()),
        "high_island" | "high-island" | "high island" => Some(TerrainTemplate::high_island()),
        "low_island" | "low-island" | "low island" => Some(TerrainTemplate::low_island()),
        "continents" => Some(TerrainTemplate::continents()),
        "archipelago_azgaar" | "archipelago-azgaar" | "archipelago (azgaar)" => {
            Some(TerrainTemplate::archipelago_azgaar())
        }
        "atoll_azgaar" | "atoll-azgaar" | "atoll (azgaar)" => Some(TerrainTemplate::atoll_azgaar()),
        "mediterranean" => Some(TerrainTemplate::mediterranean()),
        "peninsula_azgaar" | "peninsula-azgaar" | "peninsula (azgaar)" => {
            Some(TerrainTemplate::peninsula_azgaar())
        }
        "pangea" => Some(TerrainTemplate::pangea()),
        "isthmus" => Some(TerrainTemplate::isthmus()),
        // 基于图元的新模板
        "tectonic_collision" | "tectonic-collision" => Some(TerrainTemplate::tectonic_collision()),
        "volcanic_archipelago" | "volcanic-archipelago" => {
            Some(TerrainTemplate::volcanic_archipelago())
        }
        "fjord_coast" | "fjord-coast" => Some(TerrainTemplate::fjord_coast()),
        "rift_valley" | "rift-valley" => Some(TerrainTemplate::rift_valley()),
        _ => None,
    }
}

/// 判断模板是否应该使用新的分层生成系统
/// 所有模板都使用分层系统以获得板块驱动的自然地形
pub fn should_use_layered_generation(_template_name: &str) -> bool {
    true
}

/// 获取模板建议的板块数量
/// - 群岛类型：8-10 个小板块
/// - 大陆类型：12-15 个板块  
/// - 超级大陆：6-8 个大板块
pub fn get_suggested_plate_count(template_name: &str) -> usize {
    match template_name.to_lowercase().as_str() {
        // === 超级大陆类型 (6-8 个大板块) ===
        "pangea" => 6,
        "rift_valley" | "rift-valley" => 8,

        // === 群岛类型 (8-10 个小板块) ===
        "archipelago" => 10,
        "archipelago_azgaar" | "archipelago-azgaar" | "archipelago (azgaar)" => 10,
        "volcanic_archipelago" | "volcanic-archipelago" => 10,
        "volcanic_island" | "volcanic-island" => 8,
        "volcano" => 8,
        "atoll" => 8,
        "atoll_azgaar" | "atoll-azgaar" | "atoll (azgaar)" => 8,
        "oceanic" => 8,

        // === 单岛/半岛类型 (8-10 个板块) ===
        "high_island" | "high-island" | "high island" => 10,
        "low_island" | "low-island" | "low island" => 10,
        "peninsula" => 10,
        "peninsula_azgaar" | "peninsula-azgaar" | "peninsula (azgaar)" => 10,
        "highland" => 10,
        "isthmus" => 10,

        // === 大陆类型 (10-14 个板块) ===
        "earth-like" | "earth_like" => 12,
        "continental" => 10,
        "continents" => 14,
        "mediterranean" => 12,
        "tectonic_collision" | "tectonic-collision" => 12,
        "fjord_coast" | "fjord-coast" => 10,

        // 默认：中等数量
        _ => 10,
    }
}

/// 获取模板建议的海洋比例
pub fn get_suggested_ocean_ratio(template_name: &str) -> f32 {
    match template_name.to_lowercase().as_str() {
        // 高海洋比例 (80-95%)
        "oceanic" => 0.85,
        "atoll" | "atoll_azgaar" | "atoll-azgaar" => 0.92,
        "volcanic_island" | "volcanic-island" => 0.85,
        "volcano" => 0.88,
        "archipelago" | "archipelago_azgaar" | "archipelago-azgaar" => 0.80,
        "volcanic_archipelago" | "volcanic-archipelago" => 0.85,

        // 中高海洋比例 (65-75%)
        "low_island" | "low-island" => 0.70,
        "earth-like" | "earth_like" => 0.70,
        "high_island" | "high-island" => 0.65,
        "isthmus" => 0.70,

        // 中等海洋比例 (55-65%)
        "peninsula" | "peninsula_azgaar" | "peninsula-azgaar" => 0.65,
        "continents" => 0.65,
        "mediterranean" => 0.60,
        "fjord_coast" | "fjord-coast" => 0.60,
        "continental" => 0.55,
        "tectonic_collision" | "tectonic-collision" => 0.55,
        "rift_valley" | "rift-valley" => 0.55,

        // 低海洋比例 (30-50%)
        "highland" => 0.50,
        "pangea" => 0.40,

        // 默认值
        _ => 0.65,
    }
}
