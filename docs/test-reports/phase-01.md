# Test report: Phase 1 (Project architecture)

- Commit under test: `d0e1659` (build: Rust workspace, wana-log crate, make check, CI stage 1-3)
- Date: 2026-09-25
- Host: Ubuntu 24.04 sandbox, Rust 1.94.1 (pinned), gcc 13.3

## T1: `make check` on a clean tree

- Command: `make check`
- Expected: exit 0; rustfmt clean; clippy with no warnings; all unit tests pass; `[CHECK] repository checks: PASS`
- Actual: exit 0. `wana-log` unit tests: 6 passed, 0 failed. Clippy (`-D warnings`): no diagnostics. Repo check printed `[CHECK] repository checks: PASS`.
- Result: **PASS**

## T2: Repository check rejects tracked build output

- Command: `echo x > fake.iso && git add -f fake.iso && tools/check-repo.sh`
- Expected: non-zero exit, `fake.iso` named
- Actual: `[CHECK] error: build artifacts are tracked: fake.iso`, `[CHECK] FAIL`, exit 1
- Result: **PASS**

## T3: Repository check rejects a missing required doc

- Command: move `docs/ROADMAP.md` away, run `tools/check-repo.sh`
- Expected: non-zero exit
- Actual: `[CHECK] error: required file missing: docs/ROADMAP.md`, exit 1
- Result: **PASS**

## T4: Format check rejects badly formatted code

- Command: append `pub fn  x( ){}` to `crates/wana-log/src/lib.rs`, run `make fmt-check`
- Expected: non-zero exit
- Actual: exit 2
- Result: **PASS**

## T5: CI workflow runs on GitHub

- First attempt: `git push` was rejected with HTTP 403 (no repository access). **NOT RUN.**
- Retry after the owner granted access: `81510e3` pushed to `claude/inspiring-archimedes-jybz3q` and `develop`.
- Expected: the `ci` workflow runs and is green.
- Actual: run [36104384533](https://github.com/Mudher84/wana-os/actions/runs/36104384533)
  (`claude/...`) and run [36104384840](https://github.com/Mudher84/wana-os/actions/runs/36104384840)
  (`develop`) both finished with conclusion `success`. The log shows the pinned toolchain
  `1.94.1` in use and `[CHECK] repository checks: PASS`.
- Result: **PASS**

## Phase 1 status

**PASS.** T1-T5 all pass.
