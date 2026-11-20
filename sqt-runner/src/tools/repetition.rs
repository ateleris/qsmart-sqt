// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Combining the measured runs of a job into its metrics (`repetitions`,
//! `mode` and `result_consistent` of the Job API).
//!
//! The Runner runs the binary `warmup_repetitions` times without measuring,
//! then `repetitions` times measured. This module turns the measured runs into
//! the job's figures as `job-api.openapi.yaml` defines them. It is pure, so it
//! is tested without Docker.

use sqt_types::MeasurementMode;

use super::run_binary::RunBinaryOutput;

/// Upper bound on `repetitions` and on `warmup_repetitions` of one job. Each
/// run is a container start, so even this bound means hours of work; a larger
/// request is refused at submission.
pub const MAX_REPETITIONS: u32 = 10_000;

/// The metrics of a job, combined over its measured runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Repeated {
    /// Exit code of the last measured run, as the contract defines
    /// `return_code`; `None` if it reported none.
    pub return_code: Option<i64>,
    /// Mean execution time of the runs, in milliseconds.
    pub time_avg_ms: Option<f64>,
    /// Median execution time (nearest rank), in milliseconds. Latency mode
    /// with at least 2 runs only.
    pub latency_p50_ms: Option<f64>,
    /// 95th percentile of the execution time (nearest rank), in milliseconds.
    /// Latency mode with at least 20 runs only.
    pub latency_p95_ms: Option<f64>,
    /// 99th percentile of the execution time (nearest rank), in milliseconds.
    /// Latency mode with at least 100 runs only.
    pub latency_p99_ms: Option<f64>,
    /// Items per second over all runs; throughput mode only.
    pub throughput_fps: Option<f64>,
    /// Highest peak resident set size of any run, in kilobytes.
    pub memory_peak_rss_kb: Option<f64>,
    /// Mean of the runs' average resident set sizes, in kilobytes.
    pub memory_avg_rss_kb: Option<f64>,
    /// Mean of GNU time's "Percent of CPU" over the runs (not yet normalised).
    pub cpu_pct: Option<f64>,
    /// Mean Docker overhead per run, in milliseconds.
    pub docker_overhead_ms: Option<f64>,
    /// Standard output of the first measured run: the job's inference output.
    pub stdout: Vec<u8>,
    /// `true` when every measured run wrote exactly the same standard output.
    pub result_consistent: bool,
}

/// Combine the measured runs of a job.
///
/// The output of the job is the first run's standard output; the caller says
/// whether every run wrote the same (`outputs_identical`), so it need not keep
/// every run's output. A figure is reported only when every run measured it;
/// one run without it makes the figure `None`, so a constraint on it fails
/// instead of resting on part of the runs (SRS-REL-002). As the contract
/// defines them, the latency percentiles need at least 2, 20 and 100 runs and
/// are `None` in throughput mode, and the throughput is `None` in latency mode.
/// `items_per_run` is the number of inference results one run produces.
pub fn combine(
    runs: &[RunBinaryOutput],
    mode: &MeasurementMode,
    items_per_run: usize,
    outputs_identical: bool,
) -> Repeated {
    let times = all(runs, |r| r.time_execution_ms);
    let mut sorted = times.clone().unwrap_or_default();
    sorted.sort_by(f64::total_cmp);
    let latency = *mode == MeasurementMode::Latency;
    let pct = |p: f64, min_runs: usize| {
        (latency && sorted.len() >= min_runs).then(|| percentile(&sorted, p)).flatten()
    };

    let throughput_fps = match (mode, &times) {
        (MeasurementMode::Throughput, Some(t)) => {
            let total_secs = t.iter().sum::<f64>() / 1000.0;
            #[expect(
                clippy::cast_precision_loss,
                reason = "run and item counts are far below 2^52"
            )]
            let items = (runs.len() * items_per_run.max(1)) as f64;
            (total_secs > 0.0).then(|| items / total_secs)
        }
        _ => None,
    };

    Repeated {
        return_code: runs.last().and_then(|r| r.return_code),
        time_avg_ms: times.as_deref().and_then(mean),
        latency_p50_ms: pct(50.0, 2),
        latency_p95_ms: pct(95.0, 20),
        latency_p99_ms: pct(99.0, 100),
        throughput_fps,
        memory_peak_rss_kb: all(runs, |r| r.memory_peak_rss_kb)
            .and_then(|v| v.into_iter().reduce(f64::max)),
        memory_avg_rss_kb: all(runs, |r| r.memory_avg_rss_kb).as_deref().and_then(mean),
        cpu_pct: all(runs, |r| r.cpu_pct).as_deref().and_then(mean),
        docker_overhead_ms: all(runs, |r| r.docker_overhead_ms).as_deref().and_then(mean),
        stdout: runs.first().map(|r| r.stdout.clone()).unwrap_or_default(),
        result_consistent: outputs_identical,
    }
}

/// Every run's value of a figure, or `None` if any run lacks it.
fn all(runs: &[RunBinaryOutput], f: impl Fn(&RunBinaryOutput) -> Option<f64>) -> Option<Vec<f64>> {
    if runs.is_empty() {
        return None;
    }
    runs.iter().map(f).collect()
}

fn mean(values: &[f64]) -> Option<f64> {
    #[expect(clippy::cast_precision_loss, reason = "run counts are far below 2^52")]
    let n = values.len() as f64;
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / n)
}

/// Nearest-rank percentile of ascending `sorted` values.
fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a rank between 1 and the number of runs"
    )]
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted.get(rank.min(sorted.len()) - 1).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ms: f64, code: i64) -> RunBinaryOutput {
        RunBinaryOutput {
            return_code: Some(code),
            time_execution_ms: Some(ms),
            memory_peak_rss_kb: Some(ms * 10.0),
            memory_avg_rss_kb: Some(ms),
            cpu_pct: Some(100.0),
            docker_overhead_ms: Some(5.0),
            stdout: b"[1]".to_vec(),
        }
    }

    fn runs(n: u32) -> Vec<RunBinaryOutput> {
        (1..=n).map(|i| run(f64::from(i), 0)).collect()
    }

    /// Verifies: SRS-FUNC-005, SRS-INST-001
    #[test]
    fn latency_figures_cover_every_measured_run() {
        let runs: Vec<_> = (1..=10).map(|i| run(f64::from(i) * 10.0, 0)).collect();
        let r = combine(&runs, &MeasurementMode::Latency, 1, true);
        assert_eq!(r.time_avg_ms, Some(55.0));
        assert_eq!(r.latency_p50_ms, Some(50.0));
        assert_eq!(r.memory_peak_rss_kb, Some(1000.0), "peak is the highest of any run");
        assert_eq!(r.memory_avg_rss_kb, Some(55.0));
        assert_eq!(r.throughput_fps, None, "latency mode leaves throughput null");
    }

    /// Verifies: SRS-FUNC-005, SRS-REL-002
    #[test]
    fn percentiles_need_the_contract_minimum_of_runs() {
        let one = combine(&runs(1), &MeasurementMode::Latency, 1, true);
        assert_eq!(one.latency_p50_ms, None, "p50 needs at least 2 runs");
        let two = combine(&runs(2), &MeasurementMode::Latency, 1, true);
        assert_eq!(two.latency_p50_ms, Some(1.0));
        assert_eq!(two.latency_p95_ms, None, "p95 needs at least 20 runs");
        let twenty = combine(&runs(20), &MeasurementMode::Latency, 1, true);
        assert_eq!(twenty.latency_p95_ms, Some(19.0));
        assert_eq!(twenty.latency_p99_ms, None, "p99 needs at least 100 runs");
        let hundred = combine(&runs(100), &MeasurementMode::Latency, 1, true);
        assert_eq!(hundred.latency_p50_ms, Some(50.0));
        assert_eq!(hundred.latency_p95_ms, Some(95.0));
        assert_eq!(hundred.latency_p99_ms, Some(99.0));
    }

    /// Verifies: SRS-FUNC-005, SRS-INST-001
    #[test]
    fn throughput_counts_items_per_second_over_all_runs() {
        // 4 runs of 250 ms, 5 items each: 20 items in 1 s.
        let runs: Vec<_> = (0..4).map(|_| run(250.0, 0)).collect();
        let r = combine(&runs, &MeasurementMode::Throughput, 5, true);
        assert_eq!(r.throughput_fps, Some(20.0));
        assert_eq!(r.time_avg_ms, Some(250.0));
        assert_eq!(r.latency_p50_ms, None, "throughput mode leaves the percentiles null");
    }

    /// Verifies: SRS-FUNC-005, SRS-INST-001
    #[test]
    fn return_code_is_the_last_runs() {
        let r = combine(&[run(1.0, 3), run(1.0, 0)], &MeasurementMode::Latency, 1, true);
        assert_eq!(r.return_code, Some(0), "the contract reports the last repetition's code");
        let r = combine(&[run(1.0, 0), run(1.0, 2)], &MeasurementMode::Latency, 1, true);
        assert_eq!(r.return_code, Some(2));
    }

    /// Verifies: SRS-REL-004
    #[test]
    fn result_consistent_is_the_callers_comparison() {
        assert!(combine(&runs(3), &MeasurementMode::Latency, 1, true).result_consistent);
        assert!(!combine(&runs(3), &MeasurementMode::Latency, 1, false).result_consistent);
    }

    /// Verifies: SRS-REL-002
    #[test]
    fn a_figure_missing_in_one_run_is_reported_as_missing() {
        let mut runs = vec![run(10.0, 0), run(20.0, 0)];
        runs[1].cpu_pct = None;
        let r = combine(&runs, &MeasurementMode::Latency, 1, true);
        assert_eq!(r.cpu_pct, None);
        assert_eq!(r.time_avg_ms, Some(15.0), "figures every run measured are still reported");
    }

    /// Verifies: SRS-REL-002
    #[test]
    fn no_runs_report_nothing() {
        let r = combine(&[], &MeasurementMode::Throughput, 1, true);
        assert_eq!(r.time_avg_ms, None);
        assert_eq!(r.return_code, None);
        assert_eq!(r.throughput_fps, None);
    }
}
