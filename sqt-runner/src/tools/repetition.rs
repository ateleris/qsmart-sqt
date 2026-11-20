// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Combining the measured runs of a job into its metrics (`repetitions`,
//! `mode` and `result_consistent` of the Job API).
//!
//! The Runner runs the binary `warmup_repetitions` times without measuring,
//! then `repetitions` times measured. This module turns the measured runs into
//! the job's figures. It is pure, so it is tested without Docker.

use sqt_types::MeasurementMode;

use super::run_binary::RunBinaryOutput;

/// The metrics of a job, combined over its measured runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Repeated {
    /// The first non-zero exit code of any run; 0 if every run exited with 0;
    /// `None` if a run reported no exit code.
    pub return_code: Option<i64>,
    /// Mean execution time of the runs, in milliseconds.
    pub time_avg_ms: Option<f64>,
    /// Median execution time (nearest rank), in milliseconds.
    pub latency_p50_ms: Option<f64>,
    /// 95th percentile of the execution time (nearest rank), in milliseconds.
    pub latency_p95_ms: Option<f64>,
    /// 99th percentile of the execution time (nearest rank), in milliseconds.
    pub latency_p99_ms: Option<f64>,
    /// Items per second over all runs; only in throughput mode.
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
/// A figure is reported only when every run measured it; one run without it
/// makes the figure `None`, so a constraint on it fails instead of resting on
/// part of the runs (SRS-REL-002). In latency mode the throughput stays `None`,
/// as the contract defines. `items_per_run` is the number of inference results
/// one run produces, used for the throughput.
///
/// # Panics
///
/// Never: an empty `runs` gives a result with every figure `None`.
pub fn combine(runs: &[RunBinaryOutput], mode: &MeasurementMode, items_per_run: usize) -> Repeated {
    let times = all(runs, |r| r.time_execution_ms);
    let mut sorted = times.clone().unwrap_or_default();
    sorted.sort_by(f64::total_cmp);

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

    let first_stdout = runs.first().map(|r| r.stdout.clone()).unwrap_or_default();
    Repeated {
        return_code: return_code(runs),
        time_avg_ms: times.as_deref().and_then(mean),
        latency_p50_ms: percentile(&sorted, 50.0),
        latency_p95_ms: percentile(&sorted, 95.0),
        latency_p99_ms: percentile(&sorted, 99.0),
        throughput_fps,
        memory_peak_rss_kb: all(runs, |r| r.memory_peak_rss_kb)
            .and_then(|v| v.into_iter().reduce(f64::max)),
        memory_avg_rss_kb: all(runs, |r| r.memory_avg_rss_kb).as_deref().and_then(mean),
        cpu_pct: all(runs, |r| r.cpu_pct).as_deref().and_then(mean),
        docker_overhead_ms: all(runs, |r| r.docker_overhead_ms).as_deref().and_then(mean),
        result_consistent: runs.iter().all(|r| r.stdout == first_stdout),
        stdout: first_stdout,
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

fn return_code(runs: &[RunBinaryOutput]) -> Option<i64> {
    let codes: Option<Vec<i64>> = runs.iter().map(|r| r.return_code).collect();
    let codes = codes.filter(|c| !c.is_empty())?;
    Some(codes.iter().copied().find(|&c| c != 0).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ms: f64, code: i64, stdout: &str) -> RunBinaryOutput {
        RunBinaryOutput {
            return_code: Some(code),
            time_execution_ms: Some(ms),
            memory_peak_rss_kb: Some(ms * 10.0),
            memory_avg_rss_kb: Some(ms),
            cpu_pct: Some(100.0),
            docker_overhead_ms: Some(5.0),
            stdout: stdout.as_bytes().to_vec(),
        }
    }

    /// Verifies: SRS-FUNC-005, SRS-INST-001
    #[test]
    fn latency_figures_cover_every_measured_run() {
        let runs: Vec<_> = (1..=10).map(|i| run(f64::from(i) * 10.0, 0, "[1]")).collect();
        let r = combine(&runs, &MeasurementMode::Latency, 1);
        assert_eq!(r.time_avg_ms, Some(55.0));
        assert_eq!(r.latency_p50_ms, Some(50.0));
        assert_eq!(r.latency_p95_ms, Some(100.0));
        assert_eq!(r.latency_p99_ms, Some(100.0));
        assert_eq!(r.memory_peak_rss_kb, Some(1000.0), "peak is the highest of any run");
        assert_eq!(r.memory_avg_rss_kb, Some(55.0));
        assert_eq!(r.throughput_fps, None, "latency mode leaves throughput null");
        assert_eq!(r.return_code, Some(0));
    }

    /// Verifies: SRS-FUNC-005, SRS-INST-001
    #[test]
    fn throughput_counts_items_per_second_over_all_runs() {
        // 4 runs of 250 ms, 5 items each: 20 items in 1 s.
        let runs: Vec<_> = (0..4).map(|_| run(250.0, 0, "[1,2,3,4,5]")).collect();
        let r = combine(&runs, &MeasurementMode::Throughput, 5);
        assert_eq!(r.throughput_fps, Some(20.0));
        assert_eq!(r.time_avg_ms, Some(250.0));
    }

    /// Verifies: SRS-REL-004, SRS-INST-001
    #[test]
    fn result_consistent_compares_every_run_output() {
        let same = vec![run(1.0, 0, "[0.5]"), run(1.0, 0, "[0.5]"), run(1.0, 0, "[0.5]")];
        assert!(combine(&same, &MeasurementMode::Latency, 1).result_consistent);
        let differ = vec![run(1.0, 0, "[0.5]"), run(1.0, 0, "[0.5]"), run(1.0, 0, "[0.6]")];
        let r = combine(&differ, &MeasurementMode::Latency, 1);
        assert!(!r.result_consistent);
        assert_eq!(r.stdout, b"[0.5]", "the output reported is the first run's");
    }

    /// Verifies: SRS-FUNC-005, SRS-INST-001
    #[test]
    fn one_failing_run_fails_the_job_return_code() {
        let runs = vec![run(1.0, 0, ""), run(1.0, 3, ""), run(1.0, 0, "")];
        assert_eq!(combine(&runs, &MeasurementMode::Latency, 1).return_code, Some(3));
    }

    /// Verifies: SRS-REL-002
    #[test]
    fn a_figure_missing_in_one_run_is_reported_as_missing() {
        let mut runs = vec![run(10.0, 0, ""), run(20.0, 0, "")];
        runs[1].cpu_pct = None;
        runs[1].return_code = None;
        let r = combine(&runs, &MeasurementMode::Latency, 1);
        assert_eq!(r.cpu_pct, None);
        assert_eq!(r.return_code, None);
        assert_eq!(r.time_avg_ms, Some(15.0), "figures every run measured are still reported");
    }

    /// Verifies: SRS-FUNC-005
    #[test]
    fn a_single_run_reports_its_own_figures() {
        let r = combine(&[run(42.0, 0, "x")], &MeasurementMode::Latency, 1);
        assert_eq!(r.time_avg_ms, Some(42.0));
        assert_eq!(r.latency_p50_ms, Some(42.0));
        assert_eq!(r.latency_p99_ms, Some(42.0));
        assert!(r.result_consistent);
    }

    /// Verifies: SRS-REL-002
    #[test]
    fn no_runs_report_nothing() {
        let r = combine(&[], &MeasurementMode::Throughput, 1);
        assert_eq!(r.time_avg_ms, None);
        assert_eq!(r.return_code, None);
        assert_eq!(r.throughput_fps, None);
    }
}
