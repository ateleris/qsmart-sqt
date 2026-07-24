# Project instructions

## Formatting — always run rustfmt before committing

Before committing any Rust change, run rustfmt and make sure the tree is clean.
This repo's `rustfmt.toml` enables **nightly-only** options
(`imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`), so use
the nightly toolchain to honour them fully:

```
cargo +nightly fmt --all
```

Stable `cargo fmt` silently ignores those two options (it prints a warning and
leaves imports as-is), so code formatted only with stable will not group imports
the way `rustfmt.toml` intends. The CI `rustfmt` job runs stable and is advisory
(non-blocking), but commits should still match the intended nightly style.

Verify before committing:

```
cargo +nightly fmt --all -- --check
```
