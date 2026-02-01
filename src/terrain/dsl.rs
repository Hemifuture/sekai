// 地形模板 DSL 解析器
//
// 支持 Azgaar 式的简洁文本格式定义地形模板
//
// 格式示例：
// ```
// Hill 1 90-100 44-56 40-60
// Range 2-3 30-50 20-80 20-80
// Smooth 3
// Multiply 0.8 land
// Mask 3
// SeaRatio 0.7
// ```

use super::template::{InvertAxis, MaskMode, StraitDirection, TerrainCommand, TerrainTemplate};
use std::f32::consts::PI;

/// DSL 解析错误
#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}

/// 解析数值范围 (如 "40-60" 或 "50")
fn parse_range(s: &str) -> Result<(f32, f32), String> {
    if s.contains('-') {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid range format: {}", s));
        }
        let min: f32 = parts[0]
            .trim()
            .parse()
            .map_err(|_| format!("Invalid number: {}", parts[0]))?;
        let max: f32 = parts[1]
            .trim()
            .parse()
            .map_err(|_| format!("Invalid number: {}", parts[1]))?;
        Ok((min, max))
    } else {
        let val: f32 = s
            .trim()
            .parse()
            .map_err(|_| format!("Invalid number: {}", s))?;
        Ok((val, val))
    }
}

/// 解析单个数值
fn parse_f32(s: &str) -> Result<f32, String> {
    s.trim()
        .parse()
        .map_err(|_| format!("Invalid number: {}", s))
}

/// 解析整数
fn parse_u32(s: &str) -> Result<u32, String> {
    // 支持范围格式，取中间值
    if s.contains('-') {
        let (min, max) = parse_range(s)?;
        Ok(((min + max) / 2.0) as u32)
    } else {
        s.trim()
            .parse()
            .map_err(|_| format!("Invalid integer: {}", s))
    }
}

/// 将百分比范围 (0-100) 转换为 0.0-1.0
fn percent_to_ratio(range: (f32, f32)) -> (f32, f32) {
    (range.0 / 100.0, range.1 / 100.0)
}

/// 解析单行命令
fn parse_line(line: &str, line_num: usize) -> Result<Option<TerrainCommand>, ParseError> {
    let line = line.trim();

    // 跳过空行和注释
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return Ok(None);
    }

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(None);
    }

    let cmd = parts[0].to_lowercase();
    let args = &parts[1..];

    let make_err = |msg: &str| ParseError {
        line: line_num,
        message: format!("{}: {}", msg, line),
    };

    match cmd.as_str() {
        // Hill count height x y [radius]
        // 示例: Hill 3 80-120 20-80 20-80
        "hill" => {
            if args.len() < 4 {
                return Err(make_err("Hill requires: count height x y [radius]"));
            }
            let count = parse_u32(args[0]).map_err(|e| make_err(&e))?;
            let height = parse_range(args[1]).map_err(|e| make_err(&e))?;
            let x = percent_to_ratio(parse_range(args[2]).map_err(|e| make_err(&e))?);
            let y = percent_to_ratio(parse_range(args[3]).map_err(|e| make_err(&e))?);
            let radius = if args.len() > 4 {
                percent_to_ratio(parse_range(args[4]).map_err(|e| make_err(&e))?)
            } else {
                (0.08, 0.15) // 默认半径
            };

            Ok(Some(TerrainCommand::Hill {
                count,
                height,
                x,
                y,
                radius,
            }))
        }

        // Range count height x y [length] [width] [angle]
        // 示例: Range 2 40-60 20-80 20-80
        "range" => {
            if args.len() < 4 {
                return Err(make_err(
                    "Range requires: count height x y [length] [width] [angle]",
                ));
            }
            let count = parse_u32(args[0]).map_err(|e| make_err(&e))?;
            let height = parse_range(args[1]).map_err(|e| make_err(&e))?;
            let x = percent_to_ratio(parse_range(args[2]).map_err(|e| make_err(&e))?);
            let y = percent_to_ratio(parse_range(args[3]).map_err(|e| make_err(&e))?);
            let length = if args.len() > 4 {
                percent_to_ratio(parse_range(args[4]).map_err(|e| make_err(&e))?)
            } else {
                (0.2, 0.5)
            };
            let width = if args.len() > 5 {
                percent_to_ratio(parse_range(args[5]).map_err(|e| make_err(&e))?)
            } else {
                (0.02, 0.05)
            };
            let angle = if args.len() > 6 {
                parse_range(args[6]).map_err(|e| make_err(&e))?
            } else {
                (0.0, 2.0 * PI)
            };

            Ok(Some(TerrainCommand::Range {
                count,
                height,
                x,
                y,
                length,
                width,
                angle,
            }))
        }

        // Trough count depth x y [length] [width] [angle]
        "trough" => {
            if args.len() < 4 {
                return Err(make_err(
                    "Trough requires: count depth x y [length] [width] [angle]",
                ));
            }
            let count = parse_u32(args[0]).map_err(|e| make_err(&e))?;
            let depth = parse_range(args[1]).map_err(|e| make_err(&e))?;
            let x = percent_to_ratio(parse_range(args[2]).map_err(|e| make_err(&e))?);
            let y = percent_to_ratio(parse_range(args[3]).map_err(|e| make_err(&e))?);
            let length = if args.len() > 4 {
                percent_to_ratio(parse_range(args[4]).map_err(|e| make_err(&e))?)
            } else {
                (0.2, 0.5)
            };
            let width = if args.len() > 5 {
                percent_to_ratio(parse_range(args[5]).map_err(|e| make_err(&e))?)
            } else {
                (0.02, 0.05)
            };
            let angle = if args.len() > 6 {
                parse_range(args[6]).map_err(|e| make_err(&e))?
            } else {
                (0.0, 2.0 * PI)
            };

            Ok(Some(TerrainCommand::Trough {
                count,
                depth,
                x,
                y,
                length,
                width,
                angle,
            }))
        }

        // Pit count depth x y [radius]
        "pit" => {
            if args.len() < 4 {
                return Err(make_err("Pit requires: count depth x y [radius]"));
            }
            let count = parse_u32(args[0]).map_err(|e| make_err(&e))?;
            let depth = parse_range(args[1]).map_err(|e| make_err(&e))?;
            let x = percent_to_ratio(parse_range(args[2]).map_err(|e| make_err(&e))?);
            let y = percent_to_ratio(parse_range(args[3]).map_err(|e| make_err(&e))?);
            let radius = if args.len() > 4 {
                percent_to_ratio(parse_range(args[4]).map_err(|e| make_err(&e))?)
            } else {
                (0.08, 0.15)
            };

            Ok(Some(TerrainCommand::Pit {
                count,
                depth,
                x,
                y,
                radius,
            }))
        }

        // Add value
        "add" => {
            if args.is_empty() {
                return Err(make_err("Add requires: value"));
            }
            let value = parse_f32(args[0]).map_err(|e| make_err(&e))?;
            Ok(Some(TerrainCommand::Add { value }))
        }

        // Multiply factor
        "multiply" | "mult" => {
            if args.is_empty() {
                return Err(make_err("Multiply requires: factor"));
            }
            let factor = parse_f32(args[0]).map_err(|e| make_err(&e))?;
            Ok(Some(TerrainCommand::Multiply { factor }))
        }

        // Smooth iterations
        "smooth" => {
            if args.is_empty() {
                return Err(make_err("Smooth requires: iterations"));
            }
            let iterations = parse_u32(args[0]).map_err(|e| make_err(&e))?;
            Ok(Some(TerrainCommand::Smooth { iterations }))
        }

        // Mask mode [strength]
        // mode: 1=EdgeFade, 2=CenterBoost, 3=RadialGradient (或直接用名字)
        "mask" => {
            if args.is_empty() {
                return Err(make_err("Mask requires: mode [strength]"));
            }
            let mode = match args[0].to_lowercase().as_str() {
                "1" | "edge" | "edgefade" => MaskMode::EdgeFade,
                "2" | "center" | "centerboost" => MaskMode::CenterBoost,
                "3" | "radial" | "radialgradient" | _ => MaskMode::RadialGradient,
            };
            let strength = if args.len() > 1 {
                parse_f32(args[1]).map_err(|e| make_err(&e))?
            } else {
                0.5
            };
            Ok(Some(TerrainCommand::Mask { mode, strength }))
        }

        // Strait width direction position depth
        // direction: v/vertical, h/horizontal
        "strait" => {
            if args.len() < 2 {
                return Err(make_err(
                    "Strait requires: width direction [position] [depth]",
                ));
            }
            let width = parse_f32(args[0]).map_err(|e| make_err(&e))? / 100.0;
            let direction = match args[1].to_lowercase().as_str() {
                "v" | "vertical" => StraitDirection::Vertical,
                "h" | "horizontal" | _ => StraitDirection::Horizontal,
            };
            let position = if args.len() > 2 {
                parse_f32(args[2]).map_err(|e| make_err(&e))? / 100.0
            } else {
                0.5
            };
            let depth = if args.len() > 3 {
                parse_f32(args[3]).map_err(|e| make_err(&e))?
            } else {
                30.0
            };
            Ok(Some(TerrainCommand::Strait {
                width,
                direction,
                position,
                depth,
            }))
        }

        // Invert probability axis
        // axis: x, y, both
        "invert" => {
            let probability = if !args.is_empty() {
                parse_f32(args[0]).map_err(|e| make_err(&e))?
            } else {
                0.5
            };
            let axis = if args.len() > 1 {
                match args[1].to_lowercase().as_str() {
                    "x" => InvertAxis::X,
                    "y" => InvertAxis::Y,
                    "both" | _ => InvertAxis::Both,
                }
            } else {
                InvertAxis::Both
            };
            Ok(Some(TerrainCommand::Invert { axis, probability }))
        }

        // Normalize
        "normalize" | "norm" => Ok(Some(TerrainCommand::Normalize)),

        // SeaRatio ratio (0.0-1.0 或 0-100)
        "searatio" | "sea" | "ocean" => {
            if args.is_empty() {
                return Err(make_err("SeaRatio requires: ratio"));
            }
            let mut ratio = parse_f32(args[0]).map_err(|e| make_err(&e))?;
            if ratio > 1.0 {
                ratio /= 100.0; // 支持百分比格式
            }
            Ok(Some(TerrainCommand::AdjustSeaRatio { ocean_ratio: ratio }))
        }

        // SetSeaLevel level
        "sealevel" => {
            if args.is_empty() {
                return Err(make_err("SeaLevel requires: level"));
            }
            let level = parse_f32(args[0]).map_err(|e| make_err(&e))?;
            Ok(Some(TerrainCommand::SetSeaLevel { level }))
        }

        // Mountain height x y radius (单个大山)
        "mountain" | "mt" => {
            if args.len() < 4 {
                return Err(make_err("Mountain requires: height x y radius"));
            }
            let height = parse_f32(args[0]).map_err(|e| make_err(&e))?;
            let x = parse_f32(args[1]).map_err(|e| make_err(&e))? / 100.0;
            let y = parse_f32(args[2]).map_err(|e| make_err(&e))? / 100.0;
            let radius = parse_f32(args[3]).map_err(|e| make_err(&e))? / 100.0;
            Ok(Some(TerrainCommand::Mountain {
                height,
                x,
                y,
                radius,
            }))
        }

        _ => Err(make_err(&format!("Unknown command: {}", cmd))),
    }
}

/// 从文本解析模板
pub fn parse_template(
    name: &str,
    description: &str,
    text: &str,
) -> Result<TerrainTemplate, ParseError> {
    let mut commands = Vec::new();

    for (i, line) in text.lines().enumerate() {
        if let Some(cmd) = parse_line(line, i + 1)? {
            commands.push(cmd);
        }
    }

    Ok(TerrainTemplate {
        name: name.to_string(),
        description: description.to_string(),
        commands,
    })
}

/// 将模板转换为 DSL 文本
pub fn template_to_dsl(template: &TerrainTemplate) -> String {
    let mut lines = Vec::new();
    lines.push(format!("# {}", template.name));
    lines.push(format!("# {}", template.description));
    lines.push(String::new());

    for cmd in &template.commands {
        let line = match cmd {
            TerrainCommand::Hill {
                count,
                height,
                x,
                y,
                radius,
            } => {
                format!(
                    "Hill {} {}-{} {}-{} {}-{} {}-{}",
                    count,
                    height.0,
                    height.1,
                    (x.0 * 100.0) as i32,
                    (x.1 * 100.0) as i32,
                    (y.0 * 100.0) as i32,
                    (y.1 * 100.0) as i32,
                    (radius.0 * 100.0) as i32,
                    (radius.1 * 100.0) as i32
                )
            }
            TerrainCommand::Range {
                count,
                height,
                x,
                y,
                length,
                width,
                angle: _,
            } => {
                format!(
                    "Range {} {}-{} {}-{} {}-{} {}-{} {}-{}",
                    count,
                    height.0,
                    height.1,
                    (x.0 * 100.0) as i32,
                    (x.1 * 100.0) as i32,
                    (y.0 * 100.0) as i32,
                    (y.1 * 100.0) as i32,
                    (length.0 * 100.0) as i32,
                    (length.1 * 100.0) as i32,
                    (width.0 * 100.0) as i32,
                    (width.1 * 100.0) as i32
                )
            }
            TerrainCommand::Trough {
                count,
                depth,
                x,
                y,
                length,
                width,
                angle: _,
            } => {
                format!(
                    "Trough {} {}-{} {}-{} {}-{} {}-{} {}-{}",
                    count,
                    depth.0,
                    depth.1,
                    (x.0 * 100.0) as i32,
                    (x.1 * 100.0) as i32,
                    (y.0 * 100.0) as i32,
                    (y.1 * 100.0) as i32,
                    (length.0 * 100.0) as i32,
                    (length.1 * 100.0) as i32,
                    (width.0 * 100.0) as i32,
                    (width.1 * 100.0) as i32
                )
            }
            TerrainCommand::Pit {
                count,
                depth,
                x,
                y,
                radius,
            } => {
                format!(
                    "Pit {} {}-{} {}-{} {}-{} {}-{}",
                    count,
                    depth.0,
                    depth.1,
                    (x.0 * 100.0) as i32,
                    (x.1 * 100.0) as i32,
                    (y.0 * 100.0) as i32,
                    (y.1 * 100.0) as i32,
                    (radius.0 * 100.0) as i32,
                    (radius.1 * 100.0) as i32
                )
            }
            TerrainCommand::Mountain {
                height,
                x,
                y,
                radius,
            } => {
                format!(
                    "Mountain {} {} {} {}",
                    height,
                    (x * 100.0) as i32,
                    (y * 100.0) as i32,
                    (radius * 100.0) as i32
                )
            }
            TerrainCommand::Add { value } => format!("Add {}", value),
            TerrainCommand::Multiply { factor } => format!("Multiply {}", factor),
            TerrainCommand::Smooth { iterations } => format!("Smooth {}", iterations),
            TerrainCommand::Mask { mode, strength } => {
                let mode_str = match mode {
                    MaskMode::EdgeFade => "edge",
                    MaskMode::CenterBoost => "center",
                    MaskMode::RadialGradient => "radial",
                };
                format!("Mask {} {}", mode_str, strength)
            }
            TerrainCommand::Strait {
                width,
                direction,
                position,
                depth,
            } => {
                let dir = match direction {
                    StraitDirection::Vertical => "vertical",
                    StraitDirection::Horizontal => "horizontal",
                };
                format!(
                    "Strait {} {} {} {}",
                    (width * 100.0) as i32,
                    dir,
                    (position * 100.0) as i32,
                    depth
                )
            }
            TerrainCommand::Invert { axis, probability } => {
                let axis_str = match axis {
                    InvertAxis::X => "x",
                    InvertAxis::Y => "y",
                    InvertAxis::Both => "both",
                };
                format!("Invert {} {}", probability, axis_str)
            }
            TerrainCommand::Normalize => "Normalize".to_string(),
            TerrainCommand::SetSeaLevel { level } => format!("SeaLevel {}", level),
            TerrainCommand::AdjustSeaRatio { ocean_ratio } => format!("SeaRatio {}", ocean_ratio),
        };
        lines.push(line);
    }

    lines.join("\n")
}

// ============================================================================
// 预设 DSL 模板 (Azgaar 风格)
// ============================================================================

/// 预设模板集合
pub mod presets {
    pub const VOLCANO: &str = r#"
Hill 1 90-100 44-56 40-60
Multiply 0.8
Range 1 30-55 45-55 40-60 20-40 2-5
Smooth 3
Hill 1 35-45 25-30 20-75
Hill 1 35-55 75-80 25-75
Hill 1 20-25 10-15 20-25 5-10
Mask radial 0.5
Normalize
SeaRatio 0.85
"#;

    pub const HIGH_ISLAND: &str = r#"
Hill 1 90-100 65-75 47-53
Add 7
Hill 5-6 20-30 25-55 45-55
Range 1 40-50 45-55 45-55
Multiply 0.8
Mask radial 0.5
Smooth 2
Trough 2-3 20-30 20-30 20-30
Trough 2-3 20-30 60-80 70-80
Hill 1 10-15 60-60 50-50
Range 1 30-40 15-85 30-40 20-40 2-5
Range 1 30-40 15-85 60-70 20-40 2-5
Pit 3-5 10-30 15-85 20-80
Normalize
SeaRatio 0.75
"#;

    pub const CONTINENTS: &str = r#"
# 大陆模板 - 多块大陆
Hill 1 80-85 60-80 40-60 15-25
Hill 1 80-85 20-30 40-60 15-25
Hill 6-7 15-30 25-75 15-85 8-15
Multiply 0.6
Hill 8-10 5-10 15-85 20-80 5-10
Range 1-2 30-60 5-15 25-75 30-50 3-6
Range 1-2 30-60 80-95 25-75 30-50 3-6
Range 0-3 30-60 80-90 20-80 25-45 2-5
Strait 2 vertical 50 30
Strait 1 vertical 30 25
Smooth 3
Trough 3-4 15-20 15-85 20-80 20-40 2-4
Trough 3-4 5-10 45-55 45-55 15-30 2-4
Pit 3-4 10-20 15-85 20-80 8-15
Mask radial 0.3
Normalize
SeaRatio 0.7
"#;

    pub const ARCHIPELAGO: &str = r#"
# 群岛模板
Add 11
Range 2-3 40-60 20-80 20-80 30-50 3-6
Hill 5 15-20 10-90 30-70 5-10
Hill 2 10-15 10-30 20-80 4-8
Hill 2 10-15 60-90 20-80 4-8
Smooth 3
Trough 10 20-30 5-95 5-95 25-45 2-5
Strait 2 vertical 50 30
Strait 2 horizontal 50 30
Normalize
SeaRatio 0.85
"#;

    pub const PANGEA: &str = r#"
# 盘古大陆 - 单一超级大陆
Hill 1-2 25-40 15-50 0-10 15-25
Hill 1-2 5-40 50-85 0-10 15-25
Hill 1-2 25-40 50-85 90-100 15-25
Hill 1-2 5-40 15-50 90-100 15-25
Hill 8-12 20-40 20-80 48-52 10-18
Smooth 2
Multiply 0.7
Trough 3-4 25-35 5-95 10-20 25-40 2-4
Trough 3-4 25-35 5-95 80-90 25-40 2-4
Range 5-6 30-40 10-90 35-65 30-50 3-6
Normalize
SeaRatio 0.55
"#;

    pub const MEDITERRANEAN: &str = r#"
# 地中海式 - 内海被陆地包围
Range 4-6 30-80 0-100 0-10 40-60 4-8
Range 4-6 30-80 0-100 90-100 40-60 4-8
Hill 6-8 30-50 10-90 0-5 10-18
Hill 6-8 30-50 10-90 95-100 10-18
Multiply 0.9
Mask edge -0.3
Smooth 1
Hill 2-3 30-70 0-5 20-80 8-15
Hill 2-3 30-70 95-100 20-80 8-15
Trough 3-6 40-50 0-100 0-10 30-50 3-6
Trough 3-6 40-50 0-100 90-100 30-50 3-6
Normalize
SeaRatio 0.5
"#;

    pub const FRACTURED: &str = r#"
# 破碎地形 - 多岛多海
Hill 12-15 50-80 5-95 5-95 8-15
Mask edge -0.2
Mask radial 0.5
Add -20
Range 6-8 40-50 5-95 10-90 35-55 3-6
Trough 8-12 30-50 10-90 10-90 20-40 2-5
Normalize
SeaRatio 0.65
"#;

    pub const RIFT_VALLEY: &str = r#"
# 大裂谷
Hill 2 60-80 20-80 30-70 20-30
Trough 1 40-60 45-55 10-90 60-80 3-6
Range 2 50-70 30-40 20-80 40-60 3-5
Range 2 50-70 60-70 20-80 40-60 3-5
Hill 1 70-90 50 50 8-12
Pit 2-3 20-35 45-55 30-70 8-12
Smooth 1
Normalize
SeaRatio 0.25
"#;
}

/// 从文件加载模板
pub fn load_template_from_file(path: &std::path::Path) -> Result<TerrainTemplate, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

    // 从文件名提取模板名
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    // 从第一行注释提取描述
    let description = content
        .lines()
        .find(|line| line.starts_with('#') && !line.starts_with("# "))
        .or_else(|| content.lines().find(|line| line.starts_with('#')))
        .map(|line| line.trim_start_matches('#').trim())
        .unwrap_or("Custom template");

    parse_template(name, description, &content).map_err(|e| format!("Parse error: {}", e))
}

/// 从目录加载所有模板
pub fn load_templates_from_dir(dir: &std::path::Path) -> Vec<TerrainTemplate> {
    let mut templates = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "terrain").unwrap_or(false) {
                match load_template_from_file(&path) {
                    Ok(template) => templates.push(template),
                    Err(e) => eprintln!("Warning: Failed to load {}: {}", path.display(), e),
                }
            }
        }
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range() {
        assert_eq!(parse_range("40-60").unwrap(), (40.0, 60.0));
        assert_eq!(parse_range("50").unwrap(), (50.0, 50.0));
    }

    #[test]
    fn test_parse_hill() {
        let cmd = parse_line("Hill 3 80-120 20-80 20-80", 1).unwrap().unwrap();
        match cmd {
            TerrainCommand::Hill {
                count,
                height,
                x,
                y,
                ..
            } => {
                assert_eq!(count, 3);
                assert_eq!(height, (80.0, 120.0));
                assert_eq!(x, (0.2, 0.8));
                assert_eq!(y, (0.2, 0.8));
            }
            _ => panic!("Expected Hill command"),
        }
    }

    #[test]
    fn test_parse_template() {
        let template = parse_template(
            "Test",
            "A test template",
            "Hill 2 50-80 20-80 20-80\nSmooth 2\nNormalize\nSeaRatio 0.7",
        )
        .unwrap();

        assert_eq!(template.commands.len(), 4);
    }

    #[test]
    fn test_preset_volcano() {
        let template = parse_template("Volcano", "Volcanic island", presets::VOLCANO).unwrap();
        assert!(!template.commands.is_empty());
    }
}
