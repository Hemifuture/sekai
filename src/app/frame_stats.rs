const SAMPLE_WINDOW_SECONDS: f64 = 10.0;
#[cfg(not(target_arch = "wasm32"))]
const FRAME_STATS_ENV: &str = "SEKAI_FRAME_STATS";
#[cfg(not(target_arch = "wasm32"))]
const FRAME_STATS_SCENARIO_ENV: &str = "SEKAI_FRAME_STATS_SCENARIO";
#[cfg(any(target_arch = "wasm32", test))]
const FRAME_STATS_QUERY_PAIR: &str = "sekai_frame_stats=1";
#[cfg(any(target_arch = "wasm32", test))]
const FRAME_STATS_SCENARIO_QUERY_PAIR: &str = "sekai_frame_scenario=medium_wind";

pub(super) fn runtime_frame_sampler() -> FrameSampler {
    FrameSampler::with_start_requested(runtime_opt_in_requested())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn runtime_medium_wind_scenario_requested() -> bool {
    std::env::var(FRAME_STATS_SCENARIO_ENV).is_ok_and(|value| value == "medium_wind")
}

#[cfg(target_arch = "wasm32")]
pub(super) fn runtime_medium_wind_scenario_requested() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|query| query_requests_medium_wind(&query))
}

#[cfg(not(target_arch = "wasm32"))]
fn runtime_opt_in_requested() -> bool {
    std::env::var(FRAME_STATS_ENV).is_ok_and(|value| value == "1")
}

#[cfg(target_arch = "wasm32")]
fn runtime_opt_in_requested() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|query| query_requests_sampling(&query))
}

#[cfg(any(target_arch = "wasm32", test))]
fn query_requests_sampling(query: &str) -> bool {
    query
        .strip_prefix('?')
        .unwrap_or(query)
        .split('&')
        .any(|pair| pair == FRAME_STATS_QUERY_PAIR)
}

#[cfg(any(target_arch = "wasm32", test))]
fn query_requests_medium_wind(query: &str) -> bool {
    query
        .strip_prefix('?')
        .unwrap_or(query)
        .split('&')
        .any(|pair| pair == FRAME_STATS_SCENARIO_QUERY_PAIR)
}

pub(super) fn emit_runtime_line(line: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
    #[cfg(target_arch = "wasm32")]
    log::info!("{line}");
}

/// Frame-rate statistics derived from consecutive application update timestamps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FrameStats {
    sample_count: usize,
    average_fps: f64,
    one_percent_low_fps: f64,
}

impl FrameStats {
    fn from_intervals(intervals: &[f64]) -> Option<Self> {
        if intervals.is_empty()
            || intervals
                .iter()
                .any(|interval| !interval.is_finite() || *interval <= 0.0)
        {
            return None;
        }
        let total_seconds = intervals.iter().sum::<f64>();
        let mut slowest = intervals.to_vec();
        slowest.sort_by(|left, right| {
            right
                .partial_cmp(left)
                .expect("finite frame intervals have a total order")
        });
        let slow_count = intervals.len().div_ceil(100);
        let slow_seconds = slowest[..slow_count].iter().sum::<f64>();
        Some(Self {
            sample_count: intervals.len(),
            average_fps: intervals.len() as f64 / total_seconds,
            one_percent_low_fps: slow_count as f64 / slow_seconds,
        })
    }

    pub(super) const fn sample_count(self) -> usize {
        self.sample_count
    }

    pub(super) const fn average_fps(self) -> f64 {
        self.average_fps
    }

    pub(super) const fn one_percent_low_fps(self) -> f64 {
        self.one_percent_low_fps
    }
}

/// Explicitly enabled, non-persisted sampler for one ten-second update window.
#[derive(Debug, Default)]
pub(super) struct FrameSampler {
    start_requested: bool,
    started_at_seconds: Option<f64>,
    previous_update_seconds: Option<f64>,
    intervals_seconds: Vec<f64>,
    completed: Option<FrameStats>,
    completed_reported: bool,
    viewport_reported: bool,
}

impl FrameSampler {
    fn with_start_requested(start_requested: bool) -> Self {
        Self {
            start_requested,
            ..Self::default()
        }
    }

    pub(super) fn start(&mut self, now_seconds: f64) {
        if !now_seconds.is_finite() {
            return;
        }
        self.start_requested = false;
        self.started_at_seconds = Some(now_seconds);
        self.previous_update_seconds = Some(now_seconds);
        self.intervals_seconds = Vec::new();
        self.completed = None;
        self.completed_reported = false;
        self.viewport_reported = false;
    }

    pub(super) fn disable(&mut self) {
        *self = Self::default();
    }

    pub(super) fn observe_update(&mut self, now_seconds: f64) {
        if self.start_requested {
            self.start(now_seconds);
        }
        let (Some(started), Some(previous)) =
            (self.started_at_seconds, self.previous_update_seconds)
        else {
            return;
        };
        if self.completed.is_some() || !now_seconds.is_finite() || now_seconds <= previous {
            return;
        }
        self.intervals_seconds.push(now_seconds - previous);
        self.previous_update_seconds = Some(now_seconds);
        if now_seconds - started >= SAMPLE_WINDOW_SECONDS {
            self.completed = FrameStats::from_intervals(&self.intervals_seconds);
        }
    }

    pub(super) const fn is_enabled(&self) -> bool {
        self.started_at_seconds.is_some()
    }

    pub(super) const fn is_requested_or_enabled(&self) -> bool {
        self.start_requested || self.is_enabled()
    }

    pub(super) const fn is_sampling(&self) -> bool {
        self.is_enabled() && self.completed.is_none()
    }

    pub(super) const fn completed(&self) -> Option<FrameStats> {
        self.completed
    }

    pub(super) fn take_viewport_report_request(&mut self) -> bool {
        if !self.is_requested_or_enabled() || self.viewport_reported {
            return false;
        }
        self.viewport_reported = true;
        true
    }

    pub(super) fn take_unreported_completed(&mut self) -> Option<FrameStats> {
        if self.completed_reported {
            return None;
        }
        let completed = self.completed?;
        self.completed_reported = true;
        Some(completed)
    }

    #[cfg(test)]
    fn retained_interval_capacity(&self) -> usize {
        self.intervals_seconds.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::{query_requests_medium_wind, query_requests_sampling, FrameSampler, FrameStats};

    #[test]
    fn wasm_query_opt_in_requires_an_exact_enabled_pair() {
        assert!(query_requests_sampling("?sekai_frame_stats=1"));
        assert!(query_requests_sampling("?x=2&sekai_frame_stats=1&y=3"));
        assert!(!query_requests_sampling(""));
        assert!(!query_requests_sampling("?sekai_frame_stats=0"));
        assert!(!query_requests_sampling("?sekai_frame_stats=1x"));
        assert!(!query_requests_sampling("?not_sekai_frame_stats=1"));
    }

    #[test]
    fn wasm_measurement_scenario_requires_an_exact_medium_wind_pair() {
        assert!(query_requests_medium_wind(
            "?sekai_frame_stats=1&sekai_frame_scenario=medium_wind"
        ));
        assert!(!query_requests_medium_wind("?sekai_frame_scenario=medium"));
        assert!(!query_requests_medium_wind(
            "?not_sekai_frame_scenario=medium_wind"
        ));
    }

    #[test]
    fn runtime_opt_in_starts_on_the_first_update_and_reports_once() {
        let mut sampler = FrameSampler::with_start_requested(true);

        sampler.observe_update(20.0);
        assert!(sampler.is_sampling());
        assert!(sampler.take_viewport_report_request());
        assert!(!sampler.take_viewport_report_request());
        assert_eq!(sampler.retained_interval_capacity(), 0);
        for step in 1..=10 {
            sampler.observe_update(20.0 + f64::from(step));
        }

        let stats = sampler.take_unreported_completed().unwrap();
        assert_eq!(stats.sample_count(), 10);
        assert_eq!(sampler.take_unreported_completed(), None);
    }

    #[test]
    fn statistics_use_elapsed_update_intervals_and_average_the_slowest_one_percent() {
        let intervals = [0.010, 0.020, 0.040, 0.010, 0.020];

        let stats = FrameStats::from_intervals(&intervals).unwrap();

        assert_eq!(stats.sample_count(), 5);
        assert!((stats.average_fps() - 50.0).abs() < 1.0e-9);
        assert!((stats.one_percent_low_fps() - 25.0).abs() < 1.0e-9);
    }

    #[test]
    fn one_percent_low_uses_the_average_of_the_slowest_ceil_one_percent() {
        let mut intervals = vec![0.010; 198];
        intervals.extend([0.050, 0.100]);

        let stats = FrameStats::from_intervals(&intervals).unwrap();

        assert!((stats.one_percent_low_fps() - (2.0 / 0.150)).abs() < 1.0e-9);
    }

    #[test]
    fn disabled_sampler_observation_is_constant_and_does_not_allocate() {
        let mut sampler = FrameSampler::default();

        for time_seconds in [0.0, 0.1, 1.0, 100.0] {
            sampler.observe_update(time_seconds);
        }

        assert!(!sampler.is_enabled());
        assert!(!sampler.take_viewport_report_request());
        assert_eq!(sampler.retained_interval_capacity(), 0);
        assert_eq!(sampler.completed(), None);
    }

    #[test]
    fn disabling_an_active_sampler_releases_its_opt_in_storage() {
        let mut sampler = FrameSampler::default();
        sampler.start(0.0);
        sampler.observe_update(0.1);
        assert!(sampler.retained_interval_capacity() > 0);

        sampler.disable();

        assert!(!sampler.is_enabled());
        assert_eq!(sampler.retained_interval_capacity(), 0);
    }

    #[test]
    fn opt_in_sampler_collects_exactly_one_ten_second_window() {
        let mut sampler = FrameSampler::default();
        sampler.start(5.0);

        for step in 1..10 {
            sampler.observe_update(5.0 + f64::from(step));
        }
        assert_eq!(sampler.completed(), None);

        sampler.observe_update(15.0);
        let stats = sampler.completed().unwrap();
        assert_eq!(stats.sample_count(), 10);
        assert!((stats.average_fps() - 1.0).abs() < 1.0e-9);

        sampler.observe_update(16.0);
        assert_eq!(sampler.completed(), Some(stats));
    }
}
