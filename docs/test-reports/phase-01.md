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

- Command: `git push -u origin claude/inspiring-archimedes-jybz3q`
- Expected: push accepted; the `ci` workflow runs and is green
- Actual: push rejected with HTTP 403 ("Claude doesn't have GitHub access to
  Mudher84/wana-os"). The workflow file parses as valid YAML, but it has
  never run on GitHub.
- Result: **NOT RUN** (blocked on repository write access)

## Phase 1 status

**IN PROGRESS.** The local exit criteria pass (T1-T4). Phase 1 becomes PASS
once T5 has run green on GitHub Actions.
