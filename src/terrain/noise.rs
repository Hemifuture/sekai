// 噪声生成系统

use noise::{NoiseFn, Perlin};

/// 噪声配置
#[derive(Debug, Clone)]
pub struct NoiseConfig {
    /// 叠加层数
    pub octaves: u32,
    /// 基础频率
    pub base_frequency: f64,
    /// 振幅衰减系数（每层振幅 *= persistence）
    pub persistence: f64,
    /// 频率倍增系数（每层频率 *= lacunarity）
    pub lacunarity: f64,
    /// 随机种子
    pub seed: u32,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            octaves: 4,
            base_frequency: 0.01,
            persistence: 0.5,
            lacunarity: 2.0,
            seed: 0,
        }
    }
}

impl NoiseConfig {
    /// 中尺度噪声配置（模拟区域构造）
    pub fn medium_scale() -> Self {
        Self {
            octaves: 3,
            base_frequency: 0.01,
            persistence: 0.5,
            lacunarity: 2.0,
            seed: 0,
        }
    }

    /// 小尺度噪声配置（模拟表面细节）
    pub fn detail_scale() -> Self {
        Self {
            octaves: 5,
            base_frequency: 0.05,
            persistence: 0.4,
            lacunarity: 2.2,
            seed: 0,
        }
    }
}

/// 噪声生成器
pub struct NoiseGenerator {
    perlin: Perlin,
}

impl NoiseGenerator {
    pub fn new(seed: u32) -> Self {
        Self {
            perlin: Perlin::new(seed),
        }
    }

    /// 生成分形布朗运动 (Fractal Brownian Motion) 噪声
    pub fn fbm(&self, x: f64, y: f64, config: &NoiseConfig) -> f64 {
        let mut value = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = config.base_frequency;
        let mut max_value = 0.0;

        for _ in 0..config.octaves {
            value += self.perlin.get([x * frequency, y * frequency]) * amplitude;
            max_value += amplitude;
            amplitude *= config.persistence;
            frequency *= config.lacunarity;
        }

        // 归一化到 [-1, 1]
        value / max_value
    }

    /// 为多个点生成噪声值
    pub fn generate_noise_map(
        &self,
        positions: &[eframe::egui::Pos2],
        config: &NoiseConfig,
    ) -> Vec<f32> {
        positions
            .iter()
            .map(|pos| {
                let noise = self.fbm(pos.x as f64, pos.y as f64, config);
                noise as f32
            })
            .collect()
    }

    /// 生成带约束的噪声（用于板块内部）
    /// strength: 噪声强度因子
    pub fn generate_constrained_noise(
        &self,
        positions: &[eframe::egui::Pos2],
        config: &NoiseConfig,
        strength_map: &[f32],
    ) -> Vec<f32> {
        positions
            .iter()
            .enumerate()
            .map(|(i, pos)| {
                let noise = self.fbm(pos.x as f64, pos.y as f64, config);
                let strength = strength_map[i];
                (noise as f32) * strength
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Pos2;

    #[test]
    fn test_fbm_noise() {
        let generator = NoiseGenerator::new(42);
        let config = NoiseConfig::default();

        let value = generator.fbm(100.0, 200.0, &config);

        // 噪声值应该在 [-1, 1] 范围内
        assert!(value >= -1.0 && value <= 1.0);
    }

    #[test]
    fn test_noise_map_generation() {
        let generator = NoiseGenerator::new(42);
        let config = NoiseConfig::default();

        let positions = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(20.0, 20.0),
        ];

        let noise_map = generator.generate_noise_map(&positions, &config);

        assert_eq!(noise_map.len(), 3);
        for &value in &noise_map {
            assert!(value >= -1.0 && value <= 1.0);
        }
    }

    #[test]
    fn test_constrained_noise() {
        let generator = NoiseGenerator::new(42);
        let config = NoiseConfig::default();

        let positions = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(20.0, 20.0),
        ];

        let strength_map = vec![0.5, 1.0, 0.2];

        let noise_map =
            generator.generate_constrained_noise(&positions, &config, &strength_map);

        assert_eq!(noise_map.len(), 3);
    }
}
