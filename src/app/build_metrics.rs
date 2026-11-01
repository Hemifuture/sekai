//! Session-only application timing; never part of a recipe or project wire.

use std::time::Duration;

use super::authoring::BuildToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildOutcome {
    Published,
    Cancelled,
    Failed,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildPhase {
    OpenOrRequest,
    WaitWorker,
    InputAndSurface,
    Graph,
    Presentation,
    GpuInstall,
    FirstSubmittedFrame,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TimingSpan {
    pub phase: BuildPhase,
    pub start: Duration,
    pub duration: Duration,
    pub parent: Option<usize>,
}

#[derive(Debug)]
pub(super) struct BuildMetrics {
    pub token: BuildToken,
    pub outcome: BuildOutcome,
    pub spans: Vec<TimingSpan>,
}

impl BuildMetrics {
    pub fn new(token: BuildToken) -> Self {
        Self {
            token,
            outcome: BuildOutcome::Stale,
            spans: Vec::new(),
        }
    }

    pub fn record(&mut self, span: TimingSpan) {
        self.spans.push(span);
    }
    pub fn finish(&mut self, outcome: BuildOutcome) {
        self.outcome = outcome;
    }
    pub fn root_duration(&self) -> Duration {
        self.spans
            .iter()
            .filter(|span| span.parent.is_none())
            .map(|span| span.duration)
            .sum()
    }
}

/// One active attempt shared only with its worker, with no retained world history.
#[derive(Clone, Debug)]
pub(super) struct BuildObservation {
    pub started_at: std::time::Instant,
    pub metrics: std::sync::Arc<std::sync::Mutex<BuildMetrics>>,
    pub termination: Option<BuildOutcome>,
    pub cancel_requested_at: Option<Duration>,
    pub cancel_feedback_at: Option<Duration>,
}

impl BuildObservation {
    pub fn new(token: BuildToken, started_at: std::time::Instant) -> Self {
        let mut metrics = BuildMetrics::new(token);
        metrics.record(TimingSpan {
            phase: BuildPhase::OpenOrRequest,
            start: Duration::ZERO,
            duration: started_at.elapsed(),
            parent: None,
        });
        metrics.record(TimingSpan {
            phase: BuildPhase::WaitWorker,
            start: started_at.elapsed(),
            duration: Duration::ZERO,
            parent: None,
        });
        Self {
            started_at,
            metrics: std::sync::Arc::new(std::sync::Mutex::new(metrics)),
            termination: None,
            cancel_requested_at: None,
            cancel_feedback_at: None,
        }
    }

    pub fn span(&self, phase: BuildPhase, parent: Option<usize>) -> TimingGuard {
        TimingGuard {
            observation: self.clone(),
            phase,
            parent,
            start: self.started_at.elapsed(),
        }
    }

    pub fn worker_exited(&self) {
        let mut metrics = self
            .metrics
            .lock()
            .expect("the timing lock is not held during operations");
        let wait = &mut metrics.spans[1];
        if wait.duration.is_zero() {
            wait.duration = self.started_at.elapsed().saturating_sub(wait.start);
        }
    }

    pub fn emit(&self, outcome: BuildOutcome) {
        let mut metrics = self
            .metrics
            .lock()
            .expect("the timing lock is not held during operations");
        metrics.finish(outcome);
        if let Some(requested) = self.cancel_requested_at {
            let worker = &metrics.spans[1];
            super::frame_stats::emit_runtime_line(&format!("SEKAI_BUILD_CANCEL outcome={outcome:?} feedback_update_s={:?} observed_worker_exit_s={:.6} slot_release_s={:.6} source=application_update_proxy", self.cancel_feedback_at.map(|feedback| feedback.saturating_sub(requested).as_secs_f64()), (worker.start + worker.duration).saturating_sub(requested).as_secs_f64(), self.started_at.elapsed().saturating_sub(requested).as_secs_f64()));
        }
        super::frame_stats::emit_runtime_line(&format!("SEKAI_BUILD_METRICS token={:?} outcome={:?} request_elapsed_s={:.6} root_spans_s={:.6} source=application_submission_proxy validation=included_in_graph_and_presentation", metrics.token, metrics.outcome, self.started_at.elapsed().as_secs_f64(), metrics.root_duration().as_secs_f64()));
        for (index, span) in metrics.spans.iter().enumerate() {
            super::frame_stats::emit_runtime_line(&format!("SEKAI_BUILD_SPAN index={index} phase={:?} start_s={:.6} duration_s={:.6} parent={:?} inclusive=true", span.phase, span.start.as_secs_f64(), span.duration.as_secs_f64(), span.parent));
        }
    }
}

/// Records the actual operation even when it exits through an error.
pub(super) struct TimingGuard {
    observation: BuildObservation,
    phase: BuildPhase,
    parent: Option<usize>,
    start: Duration,
}

impl Drop for TimingGuard {
    fn drop(&mut self) {
        let span = TimingSpan {
            phase: self.phase,
            start: self.start,
            duration: self
                .observation
                .started_at
                .elapsed()
                .saturating_sub(self.start),
            parent: self.parent,
        };
        self.observation
            .metrics
            .lock()
            .expect("the timing lock is not held during operations")
            .record(span);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::authoring::{AuthorInputs, AuthoringSession};

    fn metrics() -> BuildMetrics {
        BuildMetrics::new(
            AuthoringSession::new(AuthorInputs::default())
                .unwrap()
                .begin_build(),
        )
    }

    #[test]
    fn nested_spans_do_not_double_count_elapsed_time() {
        let mut metrics = metrics();
        metrics.record(TimingSpan {
            phase: BuildPhase::WaitWorker,
            start: Duration::ZERO,
            duration: Duration::from_secs(10),
            parent: None,
        });
        metrics.record(TimingSpan {
            phase: BuildPhase::Graph,
            start: Duration::from_secs(1),
            duration: Duration::from_secs(8),
            parent: Some(0),
        });
        assert_eq!(metrics.root_duration(), Duration::from_secs(10));
        assert_eq!(metrics.spans.len(), 2);
    }

    #[test]
    fn terminal_outcomes_keep_cancel_failure_and_stale_out_of_success() {
        for outcome in [
            BuildOutcome::Published,
            BuildOutcome::Cancelled,
            BuildOutcome::Failed,
            BuildOutcome::Stale,
        ] {
            let mut metrics = metrics();
            metrics.finish(outcome);
            assert_eq!(metrics.outcome, outcome);
        }
    }

    #[test]
    fn observations_cannot_change_recipe_identity_or_recipe_wire() {
        let inputs = AuthorInputs::default();
        let id = inputs.recipe_id().unwrap();
        let wire = serde_json::to_string(&inputs).unwrap();
        let mut metrics = metrics();
        metrics.record(TimingSpan {
            phase: BuildPhase::OpenOrRequest,
            start: Duration::ZERO,
            duration: Duration::from_secs(2),
            parent: None,
        });
        metrics.finish(BuildOutcome::Failed);
        assert_eq!(inputs.recipe_id().unwrap(), id);
        assert_eq!(serde_json::to_string(&inputs).unwrap(), wire);
        assert_eq!(metrics.spans.len(), 1);
    }
}

/// Native open operation carried into its one-shot world request.
#[derive(Clone, Copy, Debug)]
pub(super) struct OpenTiming {
    pub started_at: std::time::Instant,
    pub read_and_decode: Duration,
}
