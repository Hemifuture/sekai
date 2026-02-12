//! 生成所有模板的地形截图 - 使用项目自带的网格生成

use sekai::delaunay::triangulate;
use sekai::models::map::grid::Grid;
use sekai::terrain::{TerrainConfig, TerrainGenerator};
use std::collections::HashSet;
use std::fs;
fn main() {
    let templates = [
        "earth-like",
        "archipelago",
        "continental",
        "volcanic_island",
        "volcano",
        "high_island",
        "continents",
        "pangea",
        "mediterranean",
        "oceanic",
        "peninsula",
        "atoll",
        "low_island",
        "highland",
        "isthmus",
        "rift_valley",
        "fractured",
        "tectonic_collision",
        "fjord_coast",
    ];

    fs::create_dir_all("screenshots").unwrap();

    // 使用项目的 Grid 生成点
    let mut grid = Grid::from_cells_count(1000, 1000, 3000);
    grid.generate_points();
    let cells = grid.get_all_points();

    println!("Generated {} cells using Grid", cells.len());

    // 使用项目的 triangulate 函数
    let triangles = triangulate(&cells);
    println!("Triangulated: {} triangles", triangles.len() / 3);

    // 使用项目的邻居提取方法
    let neighbors = extract_neighbors(&triangles, cells.len());
    println!("Built neighbors, example: {:?}", &neighbors[0]);

    for template_name in &templates {
        println!("\nGenerating: {}", template_name);

        let config = TerrainConfig::with_template(*template_name);
        let generator = TerrainGenerator::new(config);

        let (heights, _, _) = generator.generate(&cells, &neighbors);

        // 统计 (海平面是 20)
        let sea = heights.iter().filter(|&&h| h <= 20).count();
        let sea_pct = sea as f32 * 100.0 / cells.len() as f32;
        println!("  Sea: {:.1}%, Land: {:.1}%", sea_pct, 100.0 - sea_pct);

        // 渲染到图像 (800x800)
        let img_size = 800;
        let mut img = vec![(50u8, 100u8, 200u8); img_size * img_size];

        // 对每个像素找到最近的单元格
        for py in 0..img_size {
            for px in 0..img_size {
                let x = px as f32 / img_size as f32 * 1000.0;
                let y = py as f32 / img_size as f32 * 1000.0;

                // 找最近的单元格
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

        // 写入 PPM
        let mut ppm = format!("P3\n{} {}\n255\n", img_size, img_size);
        for (r, g, b) in &img {
            ppm.push_str(&format!("{} {} {} ", r, g, b));
        }

        let filename = format!("screenshots/{}.ppm", template_name);
        fs::write(&filename, ppm).unwrap();
        println!("  -> {}", filename);
    }

    println!("\nDone! Generated {} screenshots.", templates.len());
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
    // Smooth continuous color gradient with many stops
    // Heights are u8 (0-255) where sea level = 20

    // Color stops: (height, r, g, b)
    let stops: &[(f32, f32, f32, f32)] = &[
        (0.0, 8.0, 20.0, 80.0),       // deep ocean - dark blue
        (5.0, 12.0, 30.0, 110.0),     // deep ocean
        (10.0, 20.0, 50.0, 140.0),    // mid ocean
        (14.0, 30.0, 70.0, 165.0),    // mid-shallow ocean
        (17.0, 45.0, 90.0, 185.0),    // shallow ocean
        (19.0, 60.0, 110.0, 195.0),   // continental shelf
        (20.0, 70.0, 120.0, 200.0),   // sea level / coast water
        (21.0, 190.0, 185.0, 140.0),  // beach / sand
        (24.0, 160.0, 195.0, 110.0),  // coastal lowland
        (30.0, 120.0, 190.0, 90.0),   // lowland green
        (45.0, 90.0, 175.0, 70.0),    // plains
        (65.0, 70.0, 160.0, 55.0),    // lush plains
        (90.0, 105.0, 155.0, 50.0),   // foothills - yellow-green
        (120.0, 140.0, 140.0, 45.0),  // low hills - olive
        (150.0, 165.0, 125.0, 50.0),  // hills - tan
        (175.0, 155.0, 105.0, 55.0),  // low mountains - brown
        (200.0, 140.0, 95.0, 65.0),   // mountains - dark brown
        (220.0, 155.0, 140.0, 120.0), // alpine - grey-brown
        (240.0, 200.0, 200.0, 200.0), // high alpine - light grey
        (255.0, 245.0, 245.0, 250.0), // snow cap - white
    ];

    // Clamp height
    let h = h.clamp(0.0, 255.0);

    // Find the two surrounding stops and interpolate
    if h <= stops[0].0 {
        return (stops[0].1 as u8, stops[0].2 as u8, stops[0].3 as u8);
    }

    for i in 1..stops.len() {
        if h <= stops[i].0 {
            let (h0, r0, g0, b0) = stops[i - 1];
            let (h1, r1, g1, b1) = stops[i];
            let t = (h - h0) / (h1 - h0);
            // Smooth hermite interpolation for extra smoothness
            let t = t * t * (3.0 - 2.0 * t);
            let r = (r0 + (r1 - r0) * t).clamp(0.0, 255.0) as u8;
            let g = (g0 + (g1 - g0) * t).clamp(0.0, 255.0) as u8;
            let b = (b0 + (b1 - b0) * t).clamp(0.0, 255.0) as u8;
            return (r, g, b);
        }
    }

    let last = stops[stops.len() - 1];
    (last.1 as u8, last.2 as u8, last.3 as u8)
}
