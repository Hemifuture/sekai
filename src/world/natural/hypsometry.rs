//! Area-weighted hypsometric statistics shared by the T0 calibration gate and
//! its diagnostic probes (spec `2026-08-21-t0-hypsometric-calibration-design`
//! §3.2). Every published hypsometric number is the empirical CDF of `(value,
//! area)` samples; nothing here depends on cell order.

/// Sorts the samples by value so the quantile and share helpers can scan them.
pub fn sort_hypsometric_samples(samples: &mut [(f32, f64)]) {
    samples.sort_by(|first, second| first.0.total_cmp(&second.0));
}

/// Total weight of the samples.
pub fn hypsometric_total_area(samples: &[(f32, f64)]) -> f64 {
    samples.iter().map(|sample| sample.1).sum()
}

/// Area-weighted mean of the sample values, `NaN` without samples.
pub fn hypsometric_mean(samples: &[(f32, f64)]) -> f64 {
    let total = hypsometric_total_area(samples);
    if total <= 0.0 {
        return f64::NAN;
    }
    samples
        .iter()
        .map(|&(value, area)| f64::from(value) * area)
        .sum::<f64>()
        / total
}

/// Area-weighted empirical quantile of samples already sorted by
/// [`sort_hypsometric_samples`]: the first value whose cumulative area reaches
/// `quantile` of the total, `NaN` without samples.
pub fn hypsometric_quantile(sorted: &[(f32, f64)], quantile: f64) -> f32 {
    let target = quantile.clamp(0.0, 1.0) * hypsometric_total_area(sorted);
    let mut cumulative = 0.0;
    sorted
        .iter()
        .find(|&&(_, area)| {
            cumulative += area;
            cumulative >= target
        })
        .or(sorted.last())
        .map_or(f32::NAN, |sample| sample.0)
}

/// Area share of samples strictly below `ceiling`, `NaN` without samples.
pub fn hypsometric_share_below(samples: &[(f32, f64)], ceiling: f32) -> f64 {
    let total = hypsometric_total_area(samples);
    if total <= 0.0 {
        return f64::NAN;
    }
    samples
        .iter()
        .filter(|sample| sample.0 < ceiling)
        .map(|sample| sample.1)
        .sum::<f64>()
        / total
}

#[cfg(test)]
mod tests {
    use super::{
        hypsometric_mean, hypsometric_quantile, hypsometric_share_below, sort_hypsometric_samples,
    };

    #[test]
    fn quantiles_and_shares_follow_the_area_weighted_cdf() {
        let mut samples = vec![(30.0_f32, 1.0_f64), (10.0, 3.0), (20.0, 2.0), (40.0, 4.0)];
        sort_hypsometric_samples(&mut samples);
        assert_eq!(hypsometric_quantile(&samples, 0.0), 10.0);
        assert_eq!(hypsometric_quantile(&samples, 0.3), 10.0);
        assert_eq!(hypsometric_quantile(&samples, 0.5), 20.0);
        assert_eq!(hypsometric_quantile(&samples, 0.6), 30.0);
        assert_eq!(hypsometric_quantile(&samples, 1.0), 40.0);
        assert_eq!(hypsometric_share_below(&samples, 20.0), 0.3);
        assert_eq!(hypsometric_share_below(&samples, 100.0), 1.0);
        assert!((hypsometric_mean(&samples) - 26.0).abs() <= 1.0e-12);
        assert!(hypsometric_quantile(&[], 0.5).is_nan());
        assert!(hypsometric_mean(&[]).is_nan());
    }
}
