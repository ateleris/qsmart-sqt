#![allow(dead_code)]

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
/// Each variant encodes the metric identity (name, unit, whether it belongs
/// to a run or analysis job) together with the comparison operator and the
/// correctly-typed threshold value.
///
/// # Examples
///
/// ```ignore
/// Constraint::return_code(Op::Eq, 0)
/// Constraint::execution_time(Op::Lte, 500.0)   // ms
/// Constraint::memory_peak_rss(Op::Lte, 4096.0) // KB
/// Constraint::cpu_max_pct(Op::Lte, 80.0)       // %
/// ```
#[derive(Debug, Clone)]
pub enum Constraint {
    // -- Run metrics ----------------------------------------------------------
    ReturnCode { op: Op, value: i64 },
    ExecutionTime { op: Op, ms: f64 },
    MemoryPeakRss { op: Op, kb: f64 },
    MemoryAvgRss { op: Op, kb: f64 },
    CpuMaxPct { op: Op, pct: f64 },
    CpuAvgPct { op: Op, pct: f64 },
    MemoryLeaks { op: Op, bytes: f64 },
    CoverageLinePct { op: Op, pct: f64 },
    MlAccuracy { op: Op, value: f64 },
    MlF1Score { op: Op, value: f64 },
    MlRecall { op: Op, value: f64 },
    MlPrecision { op: Op, value: f64 },

    // -- Analysis metrics -----------------------------------------------------
    SonarCriticalIssues { op: Op, value: i64 },
    SonarMajorIssues { op: Op, value: i64 },
    StaticWarnings { op: Op, value: i64 },
    BinarySizeKb { op: Op, kb: f64 },
}

// -- Named constructors -------------------------------------------------------

impl Constraint {
    pub fn return_code(op: Op, value: i64) -> Self {
        Self::ReturnCode { op, value }
    }
    pub fn execution_time(op: Op, ms: f64) -> Self {
        Self::ExecutionTime { op, ms }
    }
    pub fn memory_peak_rss(op: Op, kb: f64) -> Self {
        Self::MemoryPeakRss { op, kb }
    }
    pub fn memory_avg_rss(op: Op, kb: f64) -> Self {
        Self::MemoryAvgRss { op, kb }
    }
    pub fn cpu_max_pct(op: Op, pct: f64) -> Self {
        Self::CpuMaxPct { op, pct }
    }
    pub fn cpu_avg_pct(op: Op, pct: f64) -> Self {
        Self::CpuAvgPct { op, pct }
    }
    pub fn memory_leaks(op: Op, bytes: f64) -> Self {
        Self::MemoryLeaks { op, bytes }
    }
    pub fn coverage_line_pct(op: Op, pct: f64) -> Self {
        Self::CoverageLinePct { op, pct }
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
    pub fn sonar_critical_issues(op: Op, value: i64) -> Self {
        Self::SonarCriticalIssues { op, value }
    }
    pub fn sonar_major_issues(op: Op, value: i64) -> Self {
        Self::SonarMajorIssues { op, value }
    }
    pub fn static_warnings(op: Op, value: i64) -> Self {
        Self::StaticWarnings { op, value }
    }
    pub fn binary_size_kb(op: Op, kb: f64) -> Self {
        Self::BinarySizeKb { op, kb }
    }
}

// -- String-based factory -----------------------------------------------------

impl Constraint {
    /// Create a constraint from its wire name, operator, and a numeric value.
    ///
    /// The value is interpreted according to the metric's native type: integer
    /// metrics (`return_code`, `sonar.*`, `static.warnings`) truncate to `i64`,
    /// all others use `f64` directly.
    ///
    /// Returns `None` if `name` is not a known metric.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// Constraint::from_name("return_code", Op::Eq, 0.0);
    /// Constraint::from_name("time.execution", Op::Lte, 500.0);
    /// ```
    #[allow(clippy::cast_possible_truncation)]
    pub fn from_name(name: &str, op: Op, value: f64) -> Option<Self> {
        Some(match name {
            "return_code" => Self::ReturnCode { op, value: value as i64 },
            "time.execution" => Self::ExecutionTime { op, ms: value },
            "memory.peak_rss" => Self::MemoryPeakRss { op, kb: value },
            "memory.avg_rss" => Self::MemoryAvgRss { op, kb: value },
            "cpu.max_pct" => Self::CpuMaxPct { op, pct: value },
            "cpu.avg_pct" => Self::CpuAvgPct { op, pct: value },
            "memory.leaks" => Self::MemoryLeaks { op, bytes: value },
            "coverage.line_pct" => Self::CoverageLinePct { op, pct: value },
            "ml.accuracy" => Self::MlAccuracy { op, value },
            "ml.f1_score" => Self::MlF1Score { op, value },
            "ml.recall" => Self::MlRecall { op, value },
            "ml.precision" => Self::MlPrecision { op, value },
            "sonar.critical_issues" => Self::SonarCriticalIssues { op, value: value as i64 },
            "sonar.major_issues" => Self::SonarMajorIssues { op, value: value as i64 },
            "static.warnings" => Self::StaticWarnings { op, value: value as i64 },
            "binary.size_kb" => Self::BinarySizeKb { op, kb: value },
            _ => return None,
        })
    }
}

// -- Derived properties -------------------------------------------------------

impl Constraint {
    /// The API wire name used to look up the metric in response payloads.
    pub fn metric_name(&self) -> &'static str {
        match self {
            Self::ReturnCode { .. } => "return_code",
            Self::ExecutionTime { .. } => "time.execution",
            Self::MemoryPeakRss { .. } => "memory.peak_rss",
            Self::MemoryAvgRss { .. } => "memory.avg_rss",
            Self::CpuMaxPct { .. } => "cpu.max_pct",
            Self::CpuAvgPct { .. } => "cpu.avg_pct",
            Self::MemoryLeaks { .. } => "memory.leaks",
            Self::CoverageLinePct { .. } => "coverage.line_pct",
            Self::MlAccuracy { .. } => "ml.accuracy",
            Self::MlF1Score { .. } => "ml.f1_score",
            Self::MlRecall { .. } => "ml.recall",
            Self::MlPrecision { .. } => "ml.precision",
            Self::SonarCriticalIssues { .. } => "sonar.critical_issues",
            Self::SonarMajorIssues { .. } => "sonar.major_issues",
            Self::StaticWarnings { .. } => "static.warnings",
            Self::BinarySizeKb { .. } => "binary.size_kb",
        }
    }

    /// Display unit for the metric value.
    pub fn unit(&self) -> &'static str {
        match self {
            Self::ExecutionTime { .. } => "ms",
            Self::MemoryPeakRss { .. } | Self::MemoryAvgRss { .. } | Self::BinarySizeKb { .. } => {
                "KB"
            }
            Self::CpuMaxPct { .. } | Self::CpuAvgPct { .. } | Self::CoverageLinePct { .. } => "%",
            Self::MemoryLeaks { .. } => "bytes",
            _ => "",
        }
    }

    /// Whether this metric comes from an analysis job (as opposed to a run job).
    pub fn is_analysis(&self) -> bool {
        matches!(
            self,
            Self::SonarCriticalIssues { .. }
                | Self::SonarMajorIssues { .. }
                | Self::StaticWarnings { .. }
                | Self::BinarySizeKb { .. }
        )
    }

    /// The comparison operator.
    pub fn op(&self) -> Op {
        match self {
            Self::ReturnCode { op, .. }
            | Self::ExecutionTime { op, .. }
            | Self::MemoryPeakRss { op, .. }
            | Self::MemoryAvgRss { op, .. }
            | Self::CpuMaxPct { op, .. }
            | Self::CpuAvgPct { op, .. }
            | Self::MemoryLeaks { op, .. }
            | Self::CoverageLinePct { op, .. }
            | Self::MlAccuracy { op, .. }
            | Self::MlF1Score { op, .. }
            | Self::MlRecall { op, .. }
            | Self::MlPrecision { op, .. }
            | Self::SonarCriticalIssues { op, .. }
            | Self::SonarMajorIssues { op, .. }
            | Self::StaticWarnings { op, .. }
            | Self::BinarySizeKb { op, .. } => *op,
        }
    }

    /// The expected threshold as `f64` (for comparison with the actual metric).
    #[allow(clippy::cast_precision_loss)]
    pub fn expected(&self) -> f64 {
        match self {
            Self::ReturnCode { value, .. }
            | Self::SonarCriticalIssues { value, .. }
            | Self::SonarMajorIssues { value, .. }
            | Self::StaticWarnings { value, .. } => *value as f64,

            Self::ExecutionTime { ms, .. } => *ms,
            Self::MemoryPeakRss { kb, .. }
            | Self::MemoryAvgRss { kb, .. }
            | Self::BinarySizeKb { kb, .. } => *kb,
            Self::CpuMaxPct { pct, .. }
            | Self::CpuAvgPct { pct, .. }
            | Self::CoverageLinePct { pct, .. } => *pct,
            Self::MemoryLeaks { bytes, .. } => *bytes,
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
        assert!(!c.is_analysis());
    }

    #[test]
    fn execution_time_properties() {
        let c = Constraint::execution_time(Op::Lte, 500.0);
        assert_eq!(c.metric_name(), "time.execution");
        assert_eq!(c.unit(), "ms");
        assert_eq!(c.expected(), 500.0);
        assert!(!c.is_analysis());
    }

    #[test]
    fn memory_peak_rss_properties() {
        let c = Constraint::memory_peak_rss(Op::Lte, 4096.0);
        assert_eq!(c.metric_name(), "memory.peak_rss");
        assert_eq!(c.unit(), "KB");
        assert_eq!(c.expected(), 4096.0);
    }

    #[test]
    fn cpu_max_pct_properties() {
        let c = Constraint::cpu_max_pct(Op::Lte, 80.0);
        assert_eq!(c.metric_name(), "cpu.max_pct");
        assert_eq!(c.unit(), "%");
        assert_eq!(c.expected(), 80.0);
    }

    #[test]
    fn memory_leaks_properties() {
        let c = Constraint::memory_leaks(Op::Eq, 0.0);
        assert_eq!(c.metric_name(), "memory.leaks");
        assert_eq!(c.unit(), "bytes");
    }

    #[test]
    fn coverage_line_pct_properties() {
        let c = Constraint::coverage_line_pct(Op::Gte, 80.0);
        assert_eq!(c.metric_name(), "coverage.line_pct");
        assert_eq!(c.unit(), "%");
    }

    #[test]
    fn sonar_critical_is_analysis() {
        let c = Constraint::sonar_critical_issues(Op::Eq, 0);
        assert_eq!(c.metric_name(), "sonar.critical_issues");
        assert!(c.is_analysis());
    }

    #[test]
    fn binary_size_is_analysis() {
        let c = Constraint::binary_size_kb(Op::Lte, 1024.0);
        assert_eq!(c.metric_name(), "binary.size_kb");
        assert_eq!(c.unit(), "KB");
        assert!(c.is_analysis());
    }

    #[test]
    fn static_warnings_is_analysis() {
        let c = Constraint::static_warnings(Op::Lte, 5);
        assert!(c.is_analysis());
    }

    #[test]
    fn ml_metrics_are_unitless() {
        assert_eq!(Constraint::ml_accuracy(Op::Gte, 0.9).unit(), "");
        assert_eq!(Constraint::ml_f1_score(Op::Gte, 0.9).unit(), "");
        assert_eq!(Constraint::ml_recall(Op::Gte, 0.9).unit(), "");
        assert_eq!(Constraint::ml_precision(Op::Gte, 0.9).unit(), "");
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

    // --- from_name factory ---

    #[test]
    fn from_name_roundtrips_all_metrics() {
        let cases: &[(&str, f64)] = &[
            ("return_code", 0.0),
            ("time.execution", 500.0),
            ("memory.peak_rss", 4096.0),
            ("memory.avg_rss", 2048.0),
            ("cpu.max_pct", 80.0),
            ("cpu.avg_pct", 50.0),
            ("memory.leaks", 0.0),
            ("coverage.line_pct", 85.0),
            ("ml.accuracy", 0.95),
            ("ml.f1_score", 0.9),
            ("ml.recall", 0.88),
            ("ml.precision", 0.92),
            ("sonar.critical_issues", 3.0),
            ("sonar.major_issues", 5.0),
            ("static.warnings", 10.0),
            ("binary.size_kb", 1024.0),
        ];
        for &(name, value) in cases {
            let c = Constraint::from_name(name, Op::Lte, value)
                .unwrap_or_else(|| panic!("from_name failed for {name}"));
            assert_eq!(c.metric_name(), name, "metric_name mismatch for {name}");
            assert_eq!(c.op(), Op::Lte);
        }
    }

    #[test]
    fn from_name_integer_truncation() {
        let c = Constraint::from_name("return_code", Op::Eq, 1.9).unwrap();
        assert_eq!(c.expected(), 1.0); // truncated to i64 = 1
    }

    #[test]
    fn from_name_unknown_returns_none() {
        assert!(Constraint::from_name("no.such.metric", Op::Eq, 0.0).is_none());
    }

    #[test]
    fn from_name_matches_typed_constructor() {
        let typed = Constraint::execution_time(Op::Lte, 500.0);
        let dynamic = Constraint::from_name("time.execution", Op::Lte, 500.0).unwrap();
        assert_eq!(typed.metric_name(), dynamic.metric_name());
        assert_eq!(typed.unit(), dynamic.unit());
        assert_eq!(typed.op(), dynamic.op());
        assert_eq!(typed.expected(), dynamic.expected());
        assert_eq!(typed.is_analysis(), dynamic.is_analysis());
    }
}
