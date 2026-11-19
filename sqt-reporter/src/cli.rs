// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The operator interface of the `sqt` command (SRS-HF-001, SRS-OPS-002).
//!
//! `sqt --help` and `sqt <command> --help` describe every command, option and
//! exit code, so the interface needs no external documentation to be used.

use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

const RUN_AFTER_HELP: &str = "\
Exit codes:
  0  every specification passed
  1  a specification failed or hit an infrastructure error
  2  the run was interrupted (Ctrl-C or SIGTERM)

Each run writes its report, PDF, traceability matrix, QDP archive and evidence
into <report.output_dir>/runs/<run_id>/ and prints that location at the end.";

/// Command line of the `sqt` binary.
#[derive(Debug, Parser)]
#[command(
    name = "sqt",
    version,
    about = "Software Qualification Toolkit: qualifies an ML library against its requirements \
             through a Runner"
)]
pub struct Cli {
    /// Log more: -v adds debug messages, -vv also every request and response
    /// payload exchanged with the Runner. Without -v or -q, `RUST_LOG` applies.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Log only warnings and errors.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// The commands of `sqt`.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run a full qualification session against the SQT Runner.
    #[command(after_help = RUN_AFTER_HELP)]
    Run {
        /// Path to the configuration file (sqt-reporter.yaml).
        #[arg(short, long, env = "SQT_CONFIG")]
        config: PathBuf,
        /// Also copy the JSON report to this path (it is always written into the run directory).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Validate config and Runner connectivity; exit without submitting jobs.
        #[arg(long)]
        preflight: bool,
    },
    // The pre-flight mode of SRS-OPS-007.
    /// Validate config, check Runner connectivity and auth, load spec files —
    /// without submitting any jobs.
    ///
    /// Equivalent to `sqt run --preflight` but available as a first-class
    /// subcommand for CI pipelines that want a dedicated pre-flight step.
    /// Exits 0 when every check passes, 1 otherwise.
    Preflight {
        /// Path to the configuration file (sqt-reporter.yaml).
        #[arg(short, long, env = "SQT_CONFIG")]
        config: PathBuf,
    },
}

impl Cli {
    /// The log filter for this command line (SRS-OPS-002).
    ///
    /// `-q` logs warnings and errors only, `-v` adds the SQT's debug messages
    /// and `-vv` its trace messages, which carry every request and response
    /// payload of the Runner contract. Without either flag, the filter in
    /// `RUST_LOG` applies (`rust_log`), and `info` when that is unset.
    pub fn log_filter(&self, rust_log: Option<&str>) -> String {
        match (self.quiet, self.verbose) {
            (true, _) => "warn".to_owned(),
            (false, 0) => rust_log.unwrap_or("info").to_owned(),
            (false, 1) => "info,sqt_reporter=debug".to_owned(),
            (false, _) => "info,sqt_reporter=trace".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("command line parses")
    }

    /// Verifies: SRS-HF-001
    #[test]
    fn command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    /// Verifies: SRS-HF-001
    #[test]
    fn help_lists_the_commands_and_options() {
        let help = Cli::command().render_long_help().to_string();
        for needle in ["run", "preflight", "--verbose", "--quiet", "--help", "--version"] {
            assert!(help.contains(needle), "`sqt --help` does not mention {needle}:\n{help}");
        }
    }

    /// Verifies: SRS-HF-001
    #[test]
    fn run_help_describes_options_exit_codes_and_outputs() {
        let mut cmd = Cli::command();
        let help = cmd.find_subcommand_mut("run").unwrap().render_long_help().to_string();
        for needle in ["--config", "SQT_CONFIG", "--output", "--preflight", "Exit codes", "runs/"] {
            assert!(help.contains(needle), "`sqt run --help` does not mention {needle}:\n{help}");
        }
    }

    /// Verifies: SRS-HF-001
    #[test]
    fn run_has_no_positional_input() {
        let cmd = Cli::command();
        for sub in ["run", "preflight"] {
            let sub = cmd.find_subcommand(sub).unwrap();
            assert_eq!(sub.get_positionals().count(), 0, "`{}` takes a positional", sub.get_name());
            let config = sub.get_arguments().find(|a| a.get_id() == "config").unwrap();
            assert_eq!(config.get_env().and_then(|e| e.to_str()), Some("SQT_CONFIG"));
        }
    }

    /// Verifies: SRS-OPS-002
    #[test]
    fn verbosity_flags_select_the_log_filter() {
        let cases: [(&[&str], &str); 5] = [
            (&["sqt", "run", "-c", "x.yaml"], "info"),
            (&["sqt", "-q", "run", "-c", "x.yaml"], "warn"),
            (&["sqt", "run", "-c", "x.yaml", "-v"], "info,sqt_reporter=debug"),
            (&["sqt", "-vv", "run", "-c", "x.yaml"], "info,sqt_reporter=trace"),
            (&["sqt", "preflight", "-c", "x.yaml", "-vvv"], "info,sqt_reporter=trace"),
        ];
        for (args, filter) in cases {
            assert_eq!(parse(args).log_filter(None), filter, "{args:?}");
        }
    }

    /// Verifies: SRS-OPS-002
    #[test]
    fn rust_log_applies_only_without_flags() {
        assert_eq!(parse(&["sqt", "run", "-c", "x"]).log_filter(Some("debug")), "debug");
        assert_eq!(parse(&["sqt", "-q", "run", "-c", "x"]).log_filter(Some("debug")), "warn");
    }

    /// Verifies: SRS-OPS-002
    #[test]
    fn verbose_and_quiet_conflict() {
        assert!(Cli::try_parse_from(["sqt", "-q", "-v", "run", "-c", "x"]).is_err());
    }
}
