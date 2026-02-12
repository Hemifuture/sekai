//! 测试 continents 模板 - 多个种子

use sekai::delaunay::triangulate;
use sekai::models::map::grid::Grid;
use sekai::terrain::template::TerrainTemplate;
use sekai::terrain::{TerrainConfig, TerrainGenerationMode, TerrainGenerator};
use std::collections::HashSet;
use std::fs;

fn main() {
    let seeds = [42, 123, 456, 789, 1024, 2048];

    fs::create_dir_all("screenshots").unwrap();

    let mut grid = Grid::from_cells_count(1000, 1000, 5000); // 更多单元格
    grid.generate_points();
    let cells = grid.get_all_points();

    println!("Generated {} cells", cells.len());

    let triangles = triangulate(&cells);
    let neighbors = extract_neighbors(&triangles, cells.len());

    for seed in &seeds {
        println!("\nSeed: {}", seed);

        let template = TerrainTemplate::continents();
        let config = TerrainConfig {
            mode: TerrainGenerationMode::TemplateWithSeed(template, *seed),
            ..Default::default()
        };
        let generator = TerrainGenerator::new(config);

        let (heights, _, _) = generator.generate(&cells, &neighbors);

        let sea = heights.iter().filter(|&&h| h <= 20).count();
        let sea_pct = sea as f32 * 100.0 / cells.len() as f32;
        println!("  Sea: {:.1}%", sea_pct);

        // 渲染
        let img_size = 800;
        let mut img = vec![(50u8, 100u8, 200u8); img_size * img_size];

        for py in 0..img_size {
            for px in 0..img_size {
                let x = px as f32 / img_size as f32 * 1000.0;
                let y = py as f32 / img_size as f32 * 1000.0;

                let mut min_dist = f32::INFINITY;
                let mut nearest = 0;
                for (i, cell) in cells.iter().enumerate() {
                    let dx = cell.x - x;
                    let dy = cell.y - y;
                    let dist = dx * dx + dy * dy;
                    if dist < min_dist {
                        min_dist = dist;
                        nearest = i;
                    }
                }

                let h = heights[nearest] as f32;
                img[py * img_size + px] = height_to_color(h);
            }
        }

        let mut ppm = format!("P3\n{} {}\n255\n", img_size, img_size);
        for (r, g, b) in &img {
            ppm.push_str(&format!("{} {} {} ", r, g, b));
        }

        let filename = format!("screenshots/continents_seed_{}.ppm", seed);
        fs::write(&filename, ppm).unwrap();
        println!("  -> {}", filename);
    }
}

fn extract_neighbors(triangles: &[u32], num_points: usize) -> Vec<Vec<u32>> {
    let mut neighbors: Vec<HashSet<u32>> = vec![HashSet::new(); num_points];
    for chunk in triangles.chunks(3) {
        if chunk.len() == 3 {
            let (a, b, c) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
            if a < num_points && b < num_points && c < num_points {
                neighbors[a].insert(chunk[1]);
                neighbors[a].insert(chunk[2]);
                neighbors[b].insert(chunk[0]);
                neighbors[b].insert(chunk[2]);
                neighbors[c].insert(chunk[0]);
                neighbors[c].insert(chunk[1]);
            }
        }
    }
    neighbors
        .into_iter()
        .map(|set| set.into_iter().collect())
        .collect()
}

fn height_to_color(h: f32) -> (u8, u8, u8) {
    let sea_level = 20.0;
    if h <= sea_level {
        let t = h / sea_level;
        (
            (10.0 + 40.0 * t) as u8,
            (30.0 + 70.0 * t) as u8,
            (100.0 + 100.0 * t) as u8,
        )
    } else if h < 60.0 {
        let t = (h - sea_level) / 40.0;
        (
            (80.0 + 40.0 * t) as u8,
            (180.0 - 20.0 * t) as u8,
            (80.0 - 30.0 * t) as u8,
        )
    } else if h < 120.0 {
        let t = (h - 60.0) / 60.0;
        (
            (120.0 + 60.0 * t) as u8,
            (160.0 - 40.0 * t) as u8,
            (50.0 - 20.0 * t) as u8,
        )
    } else if h < 180.0 {
        let t = (h - 120.0) / 60.0;
        (
            (180.0 - 40.0 * t) as u8,
            (120.0 - 20.0 * t) as u8,
            (30.0 + 70.0 * t) as u8,
        )
    } else {
        let t = ((h - 180.0) / 75.0).min(1.0);
        let v = (140.0 + 115.0 * t) as u8;
        (v, v, v)
    }
}
