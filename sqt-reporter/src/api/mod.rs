// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The Orchestrator's side of the Runner API contract (`job-api.openapi.yaml`).

/// HTTP client for the contract operations.
pub mod client;

pub use client::ApiClient;
