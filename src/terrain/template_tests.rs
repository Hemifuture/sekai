// 地形模板测试模块
//
// 测试所有地形模板的解析、配置验证和生成功能

#[cfg(test)]
mod tests {
    use crate::terrain::dsl::{load_template_from_file, load_templates_from_dir, parse_template};
    use crate::terrain::heightmap::SEA_LEVEL;
    use crate::terrain::plate::TectonicConfig;
    use crate::terrain::template::{TerrainCommand, TerrainTemplate};
    use crate::terrain::template_executor::{GenerationMode, TemplateExecutor};
    use crate::terrain::{TerrainConfig, TerrainGenerator};
    use eframe::egui::Pos2;
    use std::path::Path;

    // ============================================================================
    // 模板文件发现和解析测试
    // ============================================================================

    #[test]
    fn test_discover_all_template_files() {
        let template_dir = Path::new("templates");
        assert!(template_dir.exists(), "templates/ directory should exist");

        let entries: Vec<_> = std::fs::read_dir(template_dir)
            .expect("Should be able to read templates directory")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "terrain")
                    .unwrap_or(false)
            })
            .collect();

        println!("Found {} template files:", entries.len());
        for entry in &entries {
            println!("  - {}", entry.path().display());
        }

        assert!(entries.len() >= 4, "Should have at least 4 template files");
    }

    #[test]
    fn test_parse_all_template_files() {
        let template_dir = Path::new("templates");
        let templates = load_templates_from_dir(template_dir);

        println!("Successfully parsed {} templates:", templates.len());
        for template in &templates {
            println!(
                "  - {} ({} commands): {}",
                template.name,
                template.commands.len(),
                template.description
            );
        }

        assert!(templates.len() >= 4, "Should parse at least 4 templates");

        // 验证每个模板至少有一些命令
        for template in &templates {
            assert!(
                !template.commands.is_empty(),
                "Template '{}' should have at least one command",
                template.name
            );
        }
    }

    #[test]
    fn test_parse_archipelago_template() {
        let path = Path::new("templates/archipelago.terrain");
        let template = load_template_from_file(path).expect("Should parse archipelago.terrain");

        assert_eq!(template.name, "archipelago");
        assert!(!template.commands.is_empty());

        // 验证有预期的命令类型
        let has_hill = template
            .commands
            .iter()
            .any(|c| matches!(c, TerrainCommand::Hill { .. }));
        let has_normalize = template
            .commands
            .iter()
            .any(|c| matches!(c, TerrainCommand::Normalize));

        assert!(has_hill, "Archipelago should have Hill commands");
        assert!(has_normalize, "Archipelago should have Normalize command");
    }

    #[test]
    fn test_parse_continents_template() {
        let path = Path::new("templates/continents.terrain");
        let template = load_template_from_file(path).expect("Should parse continents.terrain");

        assert_eq!(template.name, "continents");
        assert!(!template.commands.is_empty());
    }

    #[test]
    fn test_parse_earth_like_template() {
        let path = Path::new("templates/earth-like.terrain");
        let template = load_template_from_file(path).expect("Should parse earth-like.terrain");

        assert_eq!(template.name, "earth-like");
        assert!(!template.commands.is_empty());
    }

    #[test]
    fn test_parse_volcano_template() {
        let path = Path::new("templates/volcano.terrain");
        let template = load_template_from_file(path).expect("Should parse volcano.terrain");

        assert_eq!(template.name, "volcano");
        assert!(!template.commands.is_empty());
    }

    // ============================================================================
    // 模板配置验证测试
    // ============================================================================

    #[test]
    fn test_template_command_parameters_valid() {
        let template_dir = Path::new("templates");
        let templates = load_templates_from_dir(template_dir);

        for template in &templates {
            for (idx, cmd) in template.commands.iter().enumerate() {
                validate_command(cmd, &template.name, idx);
            }
        }
    }

    fn validate_command(cmd: &TerrainCommand, template_name: &str, idx: usize) {
        match cmd {
            TerrainCommand::Hill {
                count,
                height,
                x,
                y,
                radius,
            } => {
                assert!(
                    *count > 0,
                    "{} cmd {}: Hill count should be > 0",
                    template_name,
                    idx
                );
                assert!(
                    height.0 <= height.1,
                    "{} cmd {}: Hill height min <= max",
                    template_name,
                    idx
                );
                assert!(
                    x.0 >= 0.0 && x.1 <= 1.0,
                    "{} cmd {}: Hill x in [0,1]",
                    template_name,
                    idx
                );
                assert!(
                    y.0 >= 0.0 && y.1 <= 1.0,
                    "{} cmd {}: Hill y in [0,1]",
                    template_name,
                    idx
                );
                assert!(
                    radius.0 >= 0.0,
                    "{} cmd {}: Hill radius >= 0",
                    template_name,
                    idx
                );
            }
            TerrainCommand::Range {
                count,
                height,
                x: _,
                y: _,
                length,
                width,
                angle: _,
            } => {
                assert!(
                    *count > 0,
                    "{} cmd {}: Range count should be > 0",
                    template_name,
                    idx
                );
                assert!(
                    height.0 <= height.1,
                    "{} cmd {}: Range height min <= max",
                    template_name,
                    idx
                );
                assert!(
                    length.0 >= 0.0,
                    "{} cmd {}: Range length >= 0",
                    template_name,
                    idx
                );
                assert!(
                    width.0 >= 0.0,
                    "{} cmd {}: Range width >= 0",
                    template_name,
                    idx
                );
            }
            TerrainCommand::Trough {
                count,
                depth,
                x: _,
                y: _,
                length: _,
                width: _,
                angle: _,
            } => {
                assert!(
                    *count > 0,
                    "{} cmd {}: Trough count should be > 0",
                    template_name,
                    idx
                );
                assert!(
                    depth.0 <= depth.1,
                    "{} cmd {}: Trough depth min <= max",
                    template_name,
                    idx
                );
            }
            TerrainCommand::Pit {
                count,
                depth: _,
                x: _,
                y: _,
                radius: _,
            } => {
                assert!(
                    *count > 0,
                    "{} cmd {}: Pit count should be > 0",
                    template_name,
                    idx
                );
            }
            TerrainCommand::Smooth { iterations } => {
                assert!(
                    *iterations > 0,
                    "{} cmd {}: Smooth iterations > 0",
                    template_name,
                    idx
                );
            }
            TerrainCommand::Erode {
                iterations,
                rain,
                capacity,
                deposition,
            } => {
                assert!(
                    *iterations > 0,
                    "{} cmd {}: Erode iterations > 0",
                    template_name,
                    idx
                );
                assert!(
                    *rain >= 0.0 && *rain <= 1.0,
                    "{} cmd {}: Erode rain in [0,1]",
                    template_name,
                    idx
                );
                assert!(
                    *capacity >= 0.0,
                    "{} cmd {}: Erode capacity >= 0",
                    template_name,
                    idx
                );
                assert!(
                    *deposition >= 0.0 && *deposition <= 1.0,
                    "{} cmd {}: Erode deposition in [0,1]",
                    template_name,
                    idx
                );
            }
            TerrainCommand::Multiply { factor } => {
                assert!(
                    *factor > 0.0,
                    "{} cmd {}: Multiply factor > 0",
                    template_name,
                    idx
                );
            }
            TerrainCommand::AdjustSeaRatio { ocean_ratio } => {
                assert!(
                    *ocean_ratio >= 0.0 && *ocean_ratio <= 1.0,
                    "{} cmd {}: SeaRatio in [0,1]",
                    template_name,
                    idx
                );
            }
            // 其他命令不需要特殊验证
            _ => {}
        }
    }

    // ============================================================================
    // 模板生成执行测试
    // ============================================================================

    /// 创建简单的测试网格
    fn create_test_grid(width: u32, height: u32, cell_count: usize) -> (Vec<Pos2>, Vec<Vec<u32>>) {
        let mut cells = Vec::with_capacity(cell_count);
        let mut neighbors = Vec::with_capacity(cell_count);

        // 创建规则网格
        let cols = (cell_count as f32).sqrt() as u32;
        let rows = (cell_count as u32).div_ceil(cols);
        let cell_width = width as f32 / cols as f32;
        let cell_height = height as f32 / rows as f32;

        for i in 0..cell_count {
            let row = i as u32 / cols;
            let col = i as u32 % cols;
            let x = (col as f32 + 0.5) * cell_width;
            let y = (row as f32 + 0.5) * cell_height;
            cells.push(Pos2::new(x, y));

            // 创建邻居列表（简单的4邻居）
            let mut cell_neighbors = Vec::new();
            if col > 0 {
                cell_neighbors.push((i - 1) as u32);
            }
            if col < cols - 1 && i + 1 < cell_count {
                cell_neighbors.push((i + 1) as u32);
            }
            if row > 0 {
                cell_neighbors.push(i as u32 - cols);
            }
            if row < rows - 1 && i + cols as usize <= cell_count {
                cell_neighbors.push(i as u32 + cols);
            }
            neighbors.push(cell_neighbors);
        }

        (cells, neighbors)
    }

    fn assert_tectonic_quality(
        config: TectonicConfig,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (f32, i32) {
        let terrain = TerrainConfig::with_tectonic_simulation(config);
        let generator = TerrainGenerator::new(terrain);
        let (heights, _plates, _plate_id) = generator.generate(cells, neighbors);

        let ocean =
            heights.iter().filter(|&&h| h <= SEA_LEVEL).count() as f32 / heights.len() as f32;

        let mut sorted = heights.clone();
        sorted.sort_unstable();
        let p10 = sorted[(sorted.len() as f32 * 0.10) as usize] as i32;
        let p90 = sorted[(sorted.len() as f32 * 0.90) as usize] as i32;
        let relief = p90 - p10;

        (ocean, relief)
    }

    #[test]
    fn test_execute_all_templates_classic_mode() {
        let template_dir = Path::new("templates");
        let templates = load_templates_from_dir(template_dir);

        let width = 256;
        let height = 256;
        let cell_count = 1000;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        for template in &templates {
            println!("Testing template execution (Classic): {}", template.name);

            let executor = TemplateExecutor::with_mode(width, height, 42, GenerationMode::Classic);
            let heights = executor.execute(template, &cells, &neighbors);

            // 验证输出
            assert_eq!(
                heights.len(),
                cell_count,
                "Template '{}' should produce {} heights",
                template.name,
                cell_count
            );

            // 验证高度值合理
            let min_height = heights.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_height = heights.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

            println!("  Heights range: {:.2} to {:.2}", min_height, max_height);

            assert!(
                !heights.iter().any(|h| h.is_nan()),
                "Template '{}' should not produce NaN heights",
                template.name
            );
        }
    }

    #[test]
    fn test_execute_all_templates_bfs_mode() {
        let template_dir = Path::new("templates");
        let templates = load_templates_from_dir(template_dir);

        let width = 256;
        let height = 256;
        let cell_count = 1000;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        for template in &templates {
            println!("Testing template execution (BFS): {}", template.name);

            let executor = TemplateExecutor::with_mode(width, height, 42, GenerationMode::BfsBlob);
            let heights = executor.execute(template, &cells, &neighbors);

            assert_eq!(
                heights.len(),
                cell_count,
                "Template '{}' should produce {} heights",
                template.name,
                cell_count
            );

            assert!(
                !heights.iter().any(|h| h.is_nan()),
                "Template '{}' should not produce NaN heights",
                template.name
            );
        }
    }

    #[test]
    fn test_template_determinism() {
        let template_dir = Path::new("templates");
        let templates = load_templates_from_dir(template_dir);

        if templates.is_empty() {
            panic!("No templates found");
        }

        let template = &templates[0];
        let width = 128;
        let height = 128;
        let cell_count = 500;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        // 使用相同种子应该产生相同结果
        let executor1 = TemplateExecutor::new(width, height, 12345);
        let heights1 = executor1.execute(template, &cells, &neighbors);

        let executor2 = TemplateExecutor::new(width, height, 12345);
        let heights2 = executor2.execute(template, &cells, &neighbors);

        assert_eq!(
            heights1, heights2,
            "Same seed should produce identical results"
        );

        // 不同种子应该产生不同结果
        let executor3 = TemplateExecutor::new(width, height, 54321);
        let heights3 = executor3.execute(template, &cells, &neighbors);

        assert_ne!(
            heights1, heights3,
            "Different seeds should produce different results"
        );
    }

    // ============================================================================
    // 内置模板测试
    // ============================================================================

    #[test]
    fn test_builtin_templates_exist() {
        // 测试 template.rs 中定义的内置模板
        let earth_like = TerrainTemplate::earth_like();
        let archipelago = TerrainTemplate::archipelago();
        let continental = TerrainTemplate::continental();
        let volcano = TerrainTemplate::volcanic_island();

        assert!(!earth_like.commands.is_empty());
        assert!(!archipelago.commands.is_empty());
        assert!(!continental.commands.is_empty());
        assert!(!volcano.commands.is_empty());

        println!("Built-in templates:");
        println!("  - Earth-like: {} commands", earth_like.commands.len());
        println!("  - Archipelago: {} commands", archipelago.commands.len());
        println!("  - Continental: {} commands", continental.commands.len());
        println!("  - Volcano: {} commands", volcano.commands.len());
    }

    #[test]
    fn test_builtin_templates_execute() {
        let templates = vec![
            TerrainTemplate::earth_like(),
            TerrainTemplate::archipelago(),
            TerrainTemplate::continental(),
            TerrainTemplate::volcanic_island(),
            TerrainTemplate::volcano(),
            TerrainTemplate::high_island(),
            TerrainTemplate::continents(),
        ];

        let width = 128;
        let height = 128;
        let cell_count = 500;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        for template in &templates {
            let executor = TemplateExecutor::new(width, height, 42);
            let heights = executor.execute(template, &cells, &neighbors);

            assert_eq!(heights.len(), cell_count);
            assert!(!heights.iter().any(|h| h.is_nan()));

            println!(
                "Built-in '{}': heights {:.1} to {:.1}",
                template.name,
                heights.iter().cloned().fold(f32::INFINITY, f32::min),
                heights.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            );
        }
    }

    // ============================================================================
    // DSL 预设测试
    // ============================================================================

    #[test]
    fn test_dsl_erode_command_parse() {
        let text = "Hill 2 80-100 30-70 30-70
Erode 5 0.3 0.8 0.4
Normalize
SeaRatio 0.7";
        let template = parse_template("Erosion Test", "DSL erode parse", text)
            .expect("Should parse erode command");

        assert!(template
            .commands
            .iter()
            .any(|c| matches!(c, TerrainCommand::Erode { .. })));
    }

    #[test]
    fn test_dsl_presets_parse() {
        use crate::terrain::dsl::presets;

        let presets = vec![
            ("Volcano", presets::VOLCANO),
            ("High Island", presets::HIGH_ISLAND),
            ("Continents", presets::CONTINENTS),
            ("Archipelago", presets::ARCHIPELAGO),
            ("Pangea", presets::PANGEA),
            ("Mediterranean", presets::MEDITERRANEAN),
            ("Fractured", presets::FRACTURED),
            ("Rift Valley", presets::RIFT_VALLEY),
        ];

        for (name, dsl) in &presets {
            let template = parse_template(name, "Test preset", dsl)
                .unwrap_or_else(|_| panic!("Should parse preset: {}", name));

            assert!(
                !template.commands.is_empty(),
                "Preset '{}' should have commands",
                name
            );

            println!("Preset '{}': {} commands", name, template.commands.len());
        }
    }

    #[test]
    fn test_dsl_presets_execute() {
        use crate::terrain::dsl::presets;

        let presets = vec![
            ("Volcano", presets::VOLCANO),
            ("Continents", presets::CONTINENTS),
            ("Archipelago", presets::ARCHIPELAGO),
        ];

        let width = 128;
        let height = 128;
        let cell_count = 500;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        for (name, dsl) in &presets {
            let template = parse_template(name, "Test", dsl).unwrap();
            let executor = TemplateExecutor::new(width, height, 42);
            let heights = executor.execute(&template, &cells, &neighbors);

            assert_eq!(heights.len(), cell_count);
            assert!(!heights.iter().any(|h| h.is_nan()));
        }
    }

    // ============================================================================
    // 边界条件测试
    // ============================================================================

    #[test]
    fn test_empty_template() {
        let template = TerrainTemplate::new("Empty", "Empty template for testing");

        let width = 64;
        let height = 64;
        let cell_count = 100;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        let executor = TemplateExecutor::new(width, height, 42);
        let heights = executor.execute(&template, &cells, &neighbors);

        assert_eq!(heights.len(), cell_count);
        // 空模板应该产生全零高度
        assert!(heights.iter().all(|h| *h == 0.0));
    }

    #[test]
    fn test_single_cell_grid() {
        let template = TerrainTemplate::earth_like();

        let cells = vec![Pos2::new(32.0, 32.0)];
        let neighbors = vec![vec![]]; // 单个单元格没有邻居

        let executor = TemplateExecutor::new(64, 64, 42);
        let heights = executor.execute(&template, &cells, &neighbors);

        assert_eq!(heights.len(), 1);
        assert!(!heights[0].is_nan());
    }

    #[test]
    fn test_tectonic_simulation_realistic_distribution() {
        let width = 256;
        let height = 256;
        let cell_count = 2500;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        let mut tectonic = TectonicConfig::earth_like();
        tectonic.seed = 42;

        let (ocean, relief) = assert_tectonic_quality(tectonic, &cells, &neighbors);

        assert!(
            (0.50..=0.85).contains(&ocean),
            "Ocean ratio should be in a realistic range, got {:.3}",
            ocean
        );
        assert!(
            relief >= 80,
            "Relief should be significant for tectonic maps, got {}",
            relief
        );
    }

    #[test]
    fn test_tectonic_realism_stable_across_seeds() {
        let width = 256;
        let height = 256;
        let cell_count = 2500;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        for seed in [7_u64, 42, 123, 2024, 4096] {
            let mut tectonic = TectonicConfig::earth_like();
            tectonic.seed = seed;

            let (ocean, relief) = assert_tectonic_quality(tectonic, &cells, &neighbors);
            assert!(
                (0.48..=0.86).contains(&ocean),
                "Seed {} ocean ratio out of range: {:.3}",
                seed,
                ocean
            );
            assert!(relief >= 70, "Seed {} relief too low: {}", seed, relief);
        }
    }

    #[test]
    fn test_tectonic_ocean_ratio_responds_to_continental_ratio() {
        let width = 256;
        let height = 256;
        let cell_count = 2500;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        let mut oceanic_world = TectonicConfig::earth_like();
        oceanic_world.seed = 2024;
        oceanic_world.continental_ratio = 0.20;

        let mut continental_world = TectonicConfig::earth_like();
        continental_world.seed = 2024;
        continental_world.continental_ratio = 0.50;

        let (ocean_low_cont, _) = assert_tectonic_quality(oceanic_world, &cells, &neighbors);
        let (ocean_high_cont, _) = assert_tectonic_quality(continental_world, &cells, &neighbors);

        assert!(
            ocean_low_cont > ocean_high_cont,
            "Higher continental ratio should reduce ocean ratio, got low_cont={:.3}, high_cont={:.3}",
            ocean_low_cont,
            ocean_high_cont
        );
    }

    #[test]
    fn test_large_grid() {
        let template = TerrainTemplate::archipelago();

        let width = 512;
        let height = 512;
        let cell_count = 5000;
        let (cells, neighbors) = create_test_grid(width, height, cell_count);

        let executor = TemplateExecutor::new(width, height, 42);
        let heights = executor.execute(&template, &cells, &neighbors);

        assert_eq!(heights.len(), cell_count);
        assert!(!heights.iter().any(|h| h.is_nan()));
    }
}
