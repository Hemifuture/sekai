//! 生成所有模板的地形截图 - 使用项目自带的网格生成

use sekai::terrain::{TerrainConfig, TerrainGenerator};
use sekai::delaunay::triangulate;
use sekai::models::map::grid::Grid;
use std::fs;
use std::collections::HashSet;
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
    // Heights are u8 (0-255) where:
    // 0-20 = ocean (sea level is 20)
    // 20-255 = land
    
    let sea_level = 20.0;
    
    if h <= sea_level {
        // 海洋 - 深蓝到浅蓝 (0 = 深海, 20 = 海岸)
        let t = h / sea_level;
        let r = (10.0 + 40.0 * t) as u8;
        let g = (30.0 + 70.0 * t) as u8;
        let b = (100.0 + 100.0 * t) as u8;
        (r, g, b)
    } else if h < 60.0 {
        // 海岸/平原 - 浅绿
        let t = (h - sea_level) / 40.0;
        let r = (80.0 + 40.0 * t) as u8;
        let g = (180.0 - 20.0 * t) as u8;
        let b = (80.0 - 30.0 * t) as u8;
        (r, g, b)
    } else if h < 120.0 {
        // 丘陵 - 深绿到黄绿
        let t = (h - 60.0) / 60.0;
        let r = (120.0 + 60.0 * t) as u8;
        let g = (160.0 - 40.0 * t) as u8;
        let b = (50.0 - 20.0 * t) as u8;
        (r, g, b)
    } else if h < 180.0 {
        // 山地 - 棕色
        let t = (h - 120.0) / 60.0;
        let r = (180.0 - 40.0 * t) as u8;
        let g = (120.0 - 20.0 * t) as u8;
        let b = (30.0 + 70.0 * t) as u8;
        (r, g, b)
    } else {
        // 高山 - 灰白/雪
        let t = ((h - 180.0) / 75.0).min(1.0);
        let v = (140.0 + 115.0 * t) as u8;
        (v, v, v)
    }
}
