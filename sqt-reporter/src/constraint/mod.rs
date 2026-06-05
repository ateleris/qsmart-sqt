use std::fmt;

pub use evaluator::ConstraintOutcome;
pub(crate) mod evaluator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Lte,
    Gte,
    Lt,
    Gt,
    Eq,
    Ne,
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Lte => write!(f, "<="),
            Op::Gte => write!(f, ">="),
            Op::Lt => write!(f, "<"),
            Op::Gt => write!(f, ">"),
            Op::Eq => write!(f, "=="),
            Op::Ne => write!(f, "!="),
        }
    }
}

/// A type-safe constraint on a specific metric.
///
/// Each variant encodes the metric identity together with the comparison
/// operator and threshold. Wire names match `JobMetrics` field keys in the
/// `OpenAPI` spec exactly — the orchestrator uses `metric_name()` to look up
/// the value in `JobMetrics::get()`.
#[derive(Debug, Clone)]
pub enum Constraint {
    // -- Execution ------------------------------------------------------------
    ReturnCode { op: Op, value: i64 },
    /// Wall-clock execution time (wire name: `time.avg_ms`).
    ExecutionTime { op: Op, ms: f64 },
    TimeLatencyP50Ms { op: Op, ms: f64 },
    TimeLatencyP95Ms { op: Op, ms: f64 },
    TimeLatencyP99Ms { op: Op, ms: f64 },
    TimeThroughputFps { op: Op, fps: f64 },

    // -- Memory ---------------------------------------------------------------
    MemoryPeakRss { op: Op, kb: f64 },
    MemoryAvgRss { op: Op, kb: f64 },
    /// Valgrind / `ASan` leak bytes (wire name: `memory.leaks_bytes`).
    MemoryLeaks { op: Op, bytes: f64 },
    MemoryStackKb { op: Op, kb: f64 },
    MemoryHeapAllocations { op: Op, count: i64 },

    // -- CPU ------------------------------------------------------------------
    /// Normalised CPU utilisation 0–100 % (wire name: `cpu.avg_normalized_pct`).
    CpuAvgNormalizedPct { op: Op, pct: f64 },

    // -- Coverage -------------------------------------------------------------
    CoverageLinePct { op: Op, pct: f64 },
    CoverageBranchPct { op: Op, pct: f64 },
    CoverageFunctionPct { op: Op, pct: f64 },

    // -- Static analysis / binary ---------------------------------------------
    StaticWarnings { op: Op, value: i64 },
    StaticCriticalIssues { op: Op, value: i64 },
    StaticMajorIssues { op: Op, value: i64 },
    BinarySizeKb { op: Op, kb: f64 },

    // -- ML accuracy (computed client-side, T-054) ----------------------------
    MlAccuracy { op: Op, value: f64 },
    MlF1Score { op: Op, value: f64 },
    MlRecall { op: Op, value: f64 },
    MlPrecision { op: Op, value: f64 },
}

// -- Named constructors -------------------------------------------------------

impl Constraint {
    pub fn return_code(op: Op, value: i64) -> Self {
        Self::ReturnCode { op, value }
    }
    pub fn execution_time(op: Op, ms: f64) -> Self {
        Self::ExecutionTime { op, ms }
    }
    pub fn time_latency_p50_ms(op: Op, ms: f64) -> Self {
        Self::TimeLatencyP50Ms { op, ms }
    }
    pub fn time_latency_p95_ms(op: Op, ms: f64) -> Self {
        Self::TimeLatencyP95Ms { op, ms }
    }
    pub fn time_latency_p99_ms(op: Op, ms: f64) -> Self {
        Self::TimeLatencyP99Ms { op, ms }
    }
    pub fn time_throughput_fps(op: Op, fps: f64) -> Self {
        Self::TimeThroughputFps { op, fps }
    }
    pub fn memory_peak_rss(op: Op, kb: f64) -> Self {
        Self::MemoryPeakRss { op, kb }
    }
    pub fn memory_avg_rss(op: Op, kb: f64) -> Self {
        Self::MemoryAvgRss { op, kb }
    }
    pub fn memory_leaks(op: Op, bytes: f64) -> Self {
        Self::MemoryLeaks { op, bytes }
    }
    pub fn memory_stack_kb(op: Op, kb: f64) -> Self {
        Self::MemoryStackKb { op, kb }
    }
    pub fn memory_heap_allocations(op: Op, count: i64) -> Self {
        Self::MemoryHeapAllocations { op, count }
    }
    pub fn cpu_avg_normalized_pct(op: Op, pct: f64) -> Self {
        Self::CpuAvgNormalizedPct { op, pct }
    }
    pub fn coverage_line_pct(op: Op, pct: f64) -> Self {
        Self::CoverageLinePct { op, pct }
    }
    pub fn coverage_branch_pct(op: Op, pct: f64) -> Self {
        Self::CoverageBranchPct { op, pct }
    }
    pub fn coverage_function_pct(op: Op, pct: f64) -> Self {
        Self::CoverageFunctionPct { op, pct }
    }
    pub fn static_warnings(op: Op, value: i64) -> Self {
        Self::StaticWarnings { op, value }
    }
    pub fn static_critical_issues(op: Op, value: i64) -> Self {
        Self::StaticCriticalIssues { op, value }
    }
    pub fn static_major_issues(op: Op, value: i64) -> Self {
        Self::StaticMajorIssues { op, value }
    }
    pub fn binary_size_kb(op: Op, kb: f64) -> Self {
        Self::BinarySizeKb { op, kb }
    }
    pub fn ml_accuracy(op: Op, value: f64) -> Self {
        Self::MlAccuracy { op, value }
    }
    pub fn ml_f1_score(op: Op, value: f64) -> Self {
        Self::MlF1Score { op, value }
    }
    pub fn ml_recall(op: Op, value: f64) -> Self {
        Self::MlRecall { op, value }
    }
    pub fn ml_precision(op: Op, value: f64) -> Self {
        Self::MlPrecision { op, value }
    }
}

// -- String-based factory -----------------------------------------------------

impl Constraint {
    /// Create a constraint from its wire name, operator, and threshold.
    ///
    /// Returns `None` if `name` is not a known metric.
    #[allow(clippy::cast_possible_truncation)]
    pub fn from_name(name: &str, op: Op, value: f64) -> Option<Self> {
        Some(match name {
            "return_code" => Self::ReturnCode { op, value: value as i64 },
            "time.avg_ms" => Self::ExecutionTime { op, ms: value },
            "time.latency_p50_ms" => Self::TimeLatencyP50Ms { op, ms: value },
            "time.latency_p95_ms" => Self::TimeLatencyP95Ms { op, ms: value },
            "time.latency_p99_ms" => Self::TimeLatencyP99Ms { op, ms: value },
            "time.throughput_fps" => Self::TimeThroughputFps { op, fps: value },
            "memory.peak_rss_kb" => Self::MemoryPeakRss { op, kb: value },
            "memory.avg_rss_kb" => Self::MemoryAvgRss { op, kb: value },
            "memory.leaks_bytes" => Self::MemoryLeaks { op, bytes: value },
            "memory.stack_kb" => Self::MemoryStackKb { op, kb: value },
            "memory.heap_allocations" => {
                Self::MemoryHeapAllocations { op, count: value as i64 }
            }
            "cpu.avg_normalized_pct" => Self::CpuAvgNormalizedPct { op, pct: value },
            "coverage.line_pct" => Self::CoverageLinePct { op, pct: value },
            "coverage.branch_pct" => Self::CoverageBranchPct { op, pct: value },
            "coverage.function_pct" => Self::CoverageFunctionPct { op, pct: value },
            "static.warnings" => Self::StaticWarnings { op, value: value as i64 },
            "static.critical_issues" => Self::StaticCriticalIssues { op, value: value as i64 },
            "static.major_issues" => Self::StaticMajorIssues { op, value: value as i64 },
            "binary.size_kb" => Self::BinarySizeKb { op, kb: value },
            "ml.accuracy" => Self::MlAccuracy { op, value },
            "ml.f1_score" => Self::MlF1Score { op, value },
            "ml.recall" => Self::MlRecall { op, value },
            "ml.precision" => Self::MlPrecision { op, value },
            _ => return None,
        })
    }
}

// -- Derived properties -------------------------------------------------------

impl Constraint {
    /// Wire name matching `JobMetrics::get()`.
    pub fn metric_name(&self) -> &'static str {
        match self {
            Self::ReturnCode { .. } => "return_code",
            Self::ExecutionTime { .. } => "time.avg_ms",
            Self::TimeLatencyP50Ms { .. } => "time.latency_p50_ms",
            Self::TimeLatencyP95Ms { .. } => "time.latency_p95_ms",
            Self::TimeLatencyP99Ms { .. } => "time.latency_p99_ms",
            Self::TimeThroughputFps { .. } => "time.throughput_fps",
            Self::MemoryPeakRss { .. } => "memory.peak_rss_kb",
            Self::MemoryAvgRss { .. } => "memory.avg_rss_kb",
            Self::MemoryLeaks { .. } => "memory.leaks_bytes",
            Self::MemoryStackKb { .. } => "memory.stack_kb",
            Self::MemoryHeapAllocations { .. } => "memory.heap_allocations",
            Self::CpuAvgNormalizedPct { .. } => "cpu.avg_normalized_pct",
            Self::CoverageLinePct { .. } => "coverage.line_pct",
            Self::CoverageBranchPct { .. } => "coverage.branch_pct",
            Self::CoverageFunctionPct { .. } => "coverage.function_pct",
            Self::StaticWarnings { .. } => "static.warnings",
            Self::StaticCriticalIssues { .. } => "static.critical_issues",
            Self::StaticMajorIssues { .. } => "static.major_issues",
            Self::BinarySizeKb { .. } => "binary.size_kb",
            Self::MlAccuracy { .. } => "ml.accuracy",
            Self::MlF1Score { .. } => "ml.f1_score",
            Self::MlRecall { .. } => "ml.recall",
            Self::MlPrecision { .. } => "ml.precision",
        }
    }

    /// Returns `false` for metrics computed client-side (T-054).
    ///
    /// Client-side metrics (`ml.*`) are derived from `output.all_outputs` after the
    /// job completes and are not present in `JobMetrics`. Until T-054 is implemented
    /// these constraints always produce a failing outcome.
    pub fn is_server_side(&self) -> bool {
        !matches!(
            self,
            Self::MlAccuracy { .. }
                | Self::MlF1Score { .. }
                | Self::MlRecall { .. }
                | Self::MlPrecision { .. }
        )
    }

    /// Display unit for the metric value.
    pub fn unit(&self) -> &'static str {
        match self {
            Self::ExecutionTime { .. }
            | Self::TimeLatencyP50Ms { .. }
            | Self::TimeLatencyP95Ms { .. }
            | Self::TimeLatencyP99Ms { .. } => "ms",
            Self::MemoryPeakRss { .. }
            | Self::MemoryAvgRss { .. }
            | Self::MemoryStackKb { .. }
            | Self::BinarySizeKb { .. } => "KB",
            Self::MemoryLeaks { .. } => "bytes",
            Self::CpuAvgNormalizedPct { .. }
            | Self::CoverageLinePct { .. }
            | Self::CoverageBranchPct { .. }
            | Self::CoverageFunctionPct { .. } => "%",
            Self::TimeThroughputFps { .. } => "fps",
            _ => "",
        }
    }

    /// Comparison operator.
    pub fn op(&self) -> Op {
        match self {
            Self::ReturnCode { op, .. }
            | Self::ExecutionTime { op, .. }
            | Self::TimeLatencyP50Ms { op, .. }
            | Self::TimeLatencyP95Ms { op, .. }
            | Self::TimeLatencyP99Ms { op, .. }
            | Self::TimeThroughputFps { op, .. }
            | Self::MemoryPeakRss { op, .. }
            | Self::MemoryAvgRss { op, .. }
            | Self::MemoryLeaks { op, .. }
            | Self::MemoryStackKb { op, .. }
            | Self::MemoryHeapAllocations { op, .. }
            | Self::CpuAvgNormalizedPct { op, .. }
            | Self::CoverageLinePct { op, .. }
            | Self::CoverageBranchPct { op, .. }
            | Self::CoverageFunctionPct { op, .. }
            | Self::StaticWarnings { op, .. }
            | Self::StaticCriticalIssues { op, .. }
            | Self::StaticMajorIssues { op, .. }
            | Self::BinarySizeKb { op, .. }
            | Self::MlAccuracy { op, .. }
            | Self::MlF1Score { op, .. }
            | Self::MlRecall { op, .. }
            | Self::MlPrecision { op, .. } => *op,
        }
    }

    /// Expected threshold as `f64`.
    #[allow(clippy::cast_precision_loss)]
    pub fn expected(&self) -> f64 {
        match self {
            Self::ReturnCode { value, .. }
            | Self::StaticWarnings { value, .. }
            | Self::StaticCriticalIssues { value, .. }
            | Self::StaticMajorIssues { value, .. } => *value as f64,
            Self::MemoryHeapAllocations { count, .. } => *count as f64,
            Self::ExecutionTime { ms, .. }
            | Self::TimeLatencyP50Ms { ms, .. }
            | Self::TimeLatencyP95Ms { ms, .. }
            | Self::TimeLatencyP99Ms { ms, .. } => *ms,
            Self::TimeThroughputFps { fps, .. } => *fps,
            Self::MemoryPeakRss { kb, .. }
            | Self::MemoryAvgRss { kb, .. }
            | Self::MemoryStackKb { kb, .. }
            | Self::BinarySizeKb { kb, .. } => *kb,
            Self::MemoryLeaks { bytes, .. } => *bytes,
            Self::CpuAvgNormalizedPct { pct, .. }
            | Self::CoverageLinePct { pct, .. }
            | Self::CoverageBranchPct { pct, .. }
            | Self::CoverageFunctionPct { pct, .. } => *pct,
            Self::MlAccuracy { value, .. }
            | Self::MlF1Score { value, .. }
            | Self::MlRecall { value, .. }
            | Self::MlPrecision { value, .. } => *value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_code_properties() {
        let c = Constraint::return_code(Op::Eq, 0);
        assert_eq!(c.metric_name(), "return_code");
        assert_eq!(c.unit(), "");
        assert_eq!(c.op(), Op::Eq);
        assert_eq!(c.expected(), 0.0);
    }

    #[test]
    fn execution_time_wire_name() {
        let c = Constraint::execution_time(Op::Lte, 500.0);
        assert_eq!(c.metric_name(), "time.avg_ms");
        assert_eq!(c.unit(), "ms");
        assert_eq!(c.expected(), 500.0);
    }

    #[test]
    fn memory_peak_rss_wire_name() {
        let c = Constraint::memory_peak_rss(Op::Lte, 4096.0);
        assert_eq!(c.metric_name(), "memory.peak_rss_kb");
        assert_eq!(c.unit(), "KB");
        assert_eq!(c.expected(), 4096.0);
    }

    #[test]
    fn memory_leaks_wire_name() {
        let c = Constraint::memory_leaks(Op::Eq, 0.0);
        assert_eq!(c.metric_name(), "memory.leaks_bytes");
        assert_eq!(c.unit(), "bytes");
    }

    #[test]
    fn cpu_avg_normalized_pct_properties() {
        let c = Constraint::cpu_avg_normalized_pct(Op::Lte, 80.0);
        assert_eq!(c.metric_name(), "cpu.avg_normalized_pct");
        assert_eq!(c.unit(), "%");
        assert_eq!(c.expected(), 80.0);
    }

    #[test]
    fn coverage_line_pct_properties() {
        let c = Constraint::coverage_line_pct(Op::Gte, 80.0);
        assert_eq!(c.metric_name(), "coverage.line_pct");
        assert_eq!(c.unit(), "%");
    }

    #[test]
    fn static_critical_issues_wire_name() {
        let c = Constraint::static_critical_issues(Op::Eq, 0);
        assert_eq!(c.metric_name(), "static.critical_issues");
    }

    #[test]
    fn static_major_issues_wire_name() {
        let c = Constraint::static_major_issues(Op::Eq, 0);
        assert_eq!(c.metric_name(), "static.major_issues");
    }

    #[test]
    fn binary_size_properties() {
        let c = Constraint::binary_size_kb(Op::Lte, 1024.0);
        assert_eq!(c.metric_name(), "binary.size_kb");
        assert_eq!(c.unit(), "KB");
    }

    #[test]
    fn static_warnings_properties() {
        let c = Constraint::static_warnings(Op::Lte, 5);
        assert_eq!(c.metric_name(), "static.warnings");
    }

    #[test]
    fn ml_metrics_are_unitless() {
        assert_eq!(Constraint::ml_accuracy(Op::Gte, 0.9).unit(), "");
        assert_eq!(Constraint::ml_f1_score(Op::Gte, 0.9).unit(), "");
        assert_eq!(Constraint::ml_recall(Op::Gte, 0.9).unit(), "");
        assert_eq!(Constraint::ml_precision(Op::Gte, 0.9).unit(), "");
    }

    #[test]
    fn ml_constraints_are_not_server_side() {
        // T-054: ml.* metrics are computed client-side — the orchestrator must not
        // look them up in JobMetrics or it will always get "metric unavailable".
        assert!(!Constraint::ml_accuracy(Op::Gte, 0.9).is_server_side());
        assert!(!Constraint::ml_f1_score(Op::Gte, 0.9).is_server_side());
        assert!(!Constraint::ml_recall(Op::Gte, 0.9).is_server_side());
        assert!(!Constraint::ml_precision(Op::Gte, 0.9).is_server_side());
    }

    #[test]
    fn all_non_ml_constraints_are_server_side() {
        let server_side = vec![
            Constraint::return_code(Op::Eq, 0),
            Constraint::execution_time(Op::Lte, 100.0),
            Constraint::memory_peak_rss(Op::Lte, 4096.0),
            Constraint::memory_leaks(Op::Eq, 0.0),
            Constraint::cpu_avg_normalized_pct(Op::Lte, 80.0),
            Constraint::coverage_line_pct(Op::Gte, 80.0),
            Constraint::static_critical_issues(Op::Eq, 0),
            Constraint::binary_size_kb(Op::Lte, 1024.0),
        ];
        for c in &server_side {
            assert!(c.is_server_side(), "{} should be server-side", c.metric_name());
        }
    }

    #[test]
    fn op_display() {
        assert_eq!(Op::Lte.to_string(), "<=");
        assert_eq!(Op::Gte.to_string(), ">=");
        assert_eq!(Op::Lt.to_string(), "<");
        assert_eq!(Op::Gt.to_string(), ">");
        assert_eq!(Op::Eq.to_string(), "==");
        assert_eq!(Op::Ne.to_string(), "!=");
    }

    #[test]
    fn from_name_roundtrips_all_metrics() {
        let cases: &[(&str, f64)] = &[
            ("return_code", 0.0),
            ("time.avg_ms", 500.0),
            ("time.latency_p50_ms", 50.0),
            ("time.latency_p95_ms", 90.0),
            ("time.latency_p99_ms", 100.0),
            ("time.throughput_fps", 30.0),
            ("memory.peak_rss_kb", 4096.0),
            ("memory.avg_rss_kb", 2048.0),
            ("memory.leaks_bytes", 0.0),
            ("memory.stack_kb", 64.0),
            ("memory.heap_allocations", 0.0),
            ("cpu.avg_normalized_pct", 80.0),
            ("coverage.line_pct", 85.0),
            ("coverage.branch_pct", 70.0),
            ("coverage.function_pct", 90.0),
            ("static.warnings", 10.0),
            ("static.critical_issues", 0.0),
            ("static.major_issues", 5.0),
            ("binary.size_kb", 1024.0),
            ("ml.accuracy", 0.95),
            ("ml.f1_score", 0.9),
            ("ml.recall", 0.88),
            ("ml.precision", 0.92),
        ];
        for &(name, value) in cases {
            let c = Constraint::from_name(name, Op::Lte, value)
                .unwrap_or_else(|| panic!("from_name failed for {name}"));
            assert_eq!(c.metric_name(), name, "metric_name mismatch for {name}");
            assert_eq!(c.op(), Op::Lte);
        }
    }

    #[test]
    fn from_name_unknown_returns_none() {
        assert!(Constraint::from_name("no.such.metric", Op::Eq, 0.0).is_none());
    }

    // --- Cross-module contract (AD-013) --------------------------------------

    #[test]
    fn server_metric_names_recognized_by_job_metrics_get() {
        // Every metric name returned by metric_name() for a server-side constraint
        // must be a key recognized by JobMetrics::get(). If they diverge, the
        // orchestrator silently produces "metric unavailable" failing outcomes for
        // every run — the qualification tool appears to work but measures nothing.
        //
        // ml.* metrics are excluded: they are computed client-side from
        // output.all_outputs (T-054) and are intentionally absent from JobMetrics.
        use sqt_types::JobMetrics;

        let server_side = vec![
            Constraint::return_code(Op::Eq, 0),
            Constraint::execution_time(Op::Lte, 100.0),
            Constraint::time_latency_p50_ms(Op::Lte, 50.0),
            Constraint::time_latency_p95_ms(Op::Lte, 90.0),
            Constraint::time_latency_p99_ms(Op::Lte, 100.0),
            Constraint::time_throughput_fps(Op::Gte, 10.0),
            Constraint::memory_peak_rss(Op::Lte, 4096.0),
            Constraint::memory_avg_rss(Op::Lte, 2048.0),
            Constraint::memory_leaks(Op::Eq, 0.0),
            Constraint::memory_stack_kb(Op::Lte, 64.0),
            Constraint::memory_heap_allocations(Op::Lte, 1000),
            Constraint::cpu_avg_normalized_pct(Op::Lte, 80.0),
            Constraint::coverage_line_pct(Op::Gte, 80.0),
            Constraint::coverage_branch_pct(Op::Gte, 70.0),
            Constraint::coverage_function_pct(Op::Gte, 75.0),
            Constraint::static_warnings(Op::Lte, 10),
            Constraint::static_critical_issues(Op::Eq, 0),
            Constraint::static_major_issues(Op::Lte, 5),
            Constraint::binary_size_kb(Op::Lte, 1024.0),
        ];

        // Fully-populated metrics so get() returns Some for every valid name.
        let metrics = JobMetrics {
            return_code: Some(0),
            time_avg_ms: Some(87.3),
            time_latency_p50_ms: Some(50.0),
            time_latency_p95_ms: Some(90.0),
            time_latency_p99_ms: Some(100.0),
            time_throughput_fps: Some(30.0),
            memory_peak_rss_kb: Some(4096.0),
            memory_avg_rss_kb: Some(2048.0),
            memory_leaks_bytes: Some(0),
            memory_stack_kb: Some(48.0),
            memory_heap_allocations: Some(0),
            cpu_avg_normalized_pct: Some(24.3),
            coverage_line_pct: Some(87.2),
            coverage_branch_pct: Some(72.1),
            coverage_function_pct: Some(95.0),
            static_warnings: Some(1),
            static_critical_issues: Some(0),
            static_major_issues: Some(3),
            binary_size_kb: Some(412.0),
            ..Default::default()
        };

        for constraint in &server_side {
            let name = constraint.metric_name();
            assert!(
                metrics.get(name).is_some(),
                "Constraint::metric_name() returned '{name}' but JobMetrics::get('{name}') \
                 returned None — this constraint can never be evaluated, only silently failed"
            );
        }
    }
}
