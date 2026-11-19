// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The SQT Orchestrator: the client side of a qualification run.
//!
//! It loads the configuration and the test specifications, discovers the
//! requirements the Runner declares, submits one job per workload to the
//! Runner and polls it to completion, evaluates every constraint against the
//! measured metrics, and writes the report, the traceability matrix, the
//! evidence and the QDP archive into a directory of its own per run.
//!
//! The `sqt` binary (`main.rs`) wires these modules together; [`cli`] defines
//! its command line.

pub mod accuracy;
pub mod api;
pub mod baseline;
pub mod cli;
pub mod config;
pub mod constraint;
pub mod evidence;
pub mod metrics;
pub mod orchestrator;
pub mod outputs;
pub mod preflight;
pub mod report;
pub mod specs;
pub mod traceability;
