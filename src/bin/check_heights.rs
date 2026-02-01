use sekai::terrain::{TerrainConfig, TerrainGenerator};
use eframe::egui::Pos2;

fn main() {
    let size = 200;
    let cells: Vec<Pos2> = (0..size*size)
        .map(|i| {
            let x = (i % size) as f32 / size as f32;
            let y = (i / size) as f32 / size as f32;
            Pos2::new(x, y)
        })
        .collect();
    
    let neighbors: Vec<Vec<u32>> = (0..size*size)
        .map(|i| {
            let mut n = Vec::new();
            let x = i % size;
            let y = i / size;
            if x > 0 { n.push((i - 1) as u32); }
            if x < size - 1 { n.push((i + 1) as u32); }
            if y > 0 { n.push((i - size) as u32); }
            if y < size - 1 { n.push((i + size) as u32); }
            n
        })
        .collect();
    
    let config = TerrainConfig::with_template("earth-like");
    let generator = TerrainGenerator::new(config);
    let (heights, _, _) = generator.generate(&cells, &neighbors);
    
    let min = heights.iter().min().unwrap();
    let max = heights.iter().max().unwrap();
    let sum: u32 = heights.iter().map(|&h| h as u32).sum();
    let avg = sum as f32 / heights.len() as f32;
    
    let land = heights.iter().filter(|&&h| h > 20).count();
    let sea = heights.iter().filter(|&&h| h <= 20).count();
    
    println!("Height stats:");
    println!("  Min: {}", min);
    println!("  Max: {}", max);
    println!("  Avg: {:.1}", avg);
    println!("  Land cells (h>20): {} ({:.1}%)", land, land as f32 * 100.0 / heights.len() as f32);
    println!("  Sea cells (h<=20): {} ({:.1}%)", sea, sea as f32 * 100.0 / heights.len() as f32);
}
