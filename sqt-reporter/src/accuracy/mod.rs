// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Client-side ML metrics: accuracy, F1, precision and recall (SRS-FUNC-017),
//! and the output-precision comparison against reference values
//! (SRS-FUNC-016), computed from the outputs a Runner returns.

pub use compute::AccuracyMetrics;
/// The metric computations.
pub mod compute;
