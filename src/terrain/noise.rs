// 噪声生成系统

use noise::{NoiseFn, Perlin};

/// 平滑阶梯函数（比 smoothstep 更平滑）
pub fn smootherstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// 约束噪声函数 - 过滤弱信号，防止散点
pub fn constrained_noise(noise_value: f64, threshold: f64) -> f64 {
    if noise_value.abs() < threshold {
        0.0
    } else {
        noise_value
    }
}

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
    config: NoiseConfig,
    amplitude: f64,
}

impl NoiseGenerator {
    pub fn new(seed_or_frequency: u32) -> Self {
        // 支持旧接口（seed）和新接口（frequency as integer）
        Self {
            perlin: Perlin::new(seed_or_frequency),
            config: NoiseConfig::default(),
            amplitude: 1.0,
        }
    }
    
    /// 从频率创建（新接口，用于分层系统）
    pub fn from_frequency(frequency: f64) -> Self {
        Self {
            perlin: Perlin::new(0),
            config: NoiseConfig {
                base_frequency: frequency,
                ..Default::default()
            },
            amplitude: 1.0,
        }
    }
    
    /// 链式设置振幅
    pub fn with_amplitude(mut self, amplitude: f64) -> Self {
        self.amplitude = amplitude;
        self
    }
    
    /// 链式设置八度数
    pub fn with_octaves(mut self, octaves: u32) -> Self {
        self.config.octaves = octaves;
        self
    }
    
    /// 链式设置种子
    pub fn with_seed(mut self, seed: u32) -> Self {
        self.perlin = Perlin::new(seed);
        self.config.seed = seed;
        self
    }
    
    /// 采样 fBm 噪声（使用内置配置）
    pub fn sample_fbm(&self, x: f64, y: f64) -> f64 {
        self.fbm(x, y, &self.config) * self.amplitude
    }
    
    /// 采样脊状噪声
    pub fn sample_ridged(&self, x: f64, y: f64) -> f64 {
        let freq = self.config.base_frequency;
        let value = self.perlin.get([x * freq, y * freq]);
        // 脊状变换: 1 - |noise|
        (1.0 - value.abs()) * self.amplitude
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
