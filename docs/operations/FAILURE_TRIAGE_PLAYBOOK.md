# Failure Triage Playbook (Observability)

This playbook standardizes how we classify failures, collect evidence, and drive deterministic fixes for local + CI runs.

## Scope
- CI workflow failures (`.github/workflows/ci.yml`)
- Local verification failures
- M1 smoke path failures (`./scripts/smoke-m1.sh`)

## 1) Structured Failure Categories

Use one primary category per incident. Add secondary tags only when needed.

| Category | Signal / Symptom | Typical Root Cause | First Action |
| --- | --- | --- | --- |
| `ENV-TOOLCHAIN` | command not found, wrong version, setup mismatch | missing Node/pnpm/Rust/Go, incompatible runtime | run preflight and verify versions/paths |
| `JS-QUALITY` | `pnpm run lint` / `pnpm run format:check` fails | code style, formatting drift | run reported command locally and apply fixes |
| `TS-BUILD` | `pnpm run build` fails | TS compile/type/config regression | capture first compiler error + owning package |
| `JS-TEST` | `pnpm run test` fails | unit/integration behavior regression | rerun failing test with focused command |
| `RUST-FMT` | `cargo fmt --all --check` fails | rustfmt drift | run `cargo fmt --all` and re-check |
| `RUST-CLIPPY` | `cargo clippy ... -D warnings` fails | lint warning promoted to error | fix warnings, avoid allow-by-default bypass |
| `RUST-TEST` | `cargo test --workspace --all-targets` fails | behavior/panic/contract mismatch | rerun failing crate/test with full output |
| `SMOKE-BUILD` | smoke script fails before process starts | launcher/env/build artifact issue | inspect script diagnostics + build output |
| `SMOKE-RUNTIME` | binary exit/status/output differs from fail-closed contract | runtime contract mismatch or startup crash | inspect captured stdout/stderr + `dist-go/main.go` snippet |
| `CI-INFRA` | flaky network/cache/action runner issue | transient GitHub Actions problem | retry once, then isolate from product regressions |

## 2) Triage Flow (Required)

1. **Classify** into one primary category from the table.
2. **Capture evidence** before changing code:
   - failing command
   - exact error excerpt (first meaningful stack/error block)
   - environment snapshot (`node -v`, `pnpm -v`, `cargo --version`, `go version` when relevant)
3. **Reproduce locally** with the same command used by CI.
4. **Minimize scope** to a single package/crate/test when possible.
5. **Fix at source** (no fallback policy bypass, no silent ignores).
6. **Re-run full gate** (all required commands) before push.
7. **Document** in PR description using the incident template below.

### Incident Template (PR comment/body)

```md
- Category: <one of the structured categories>
- Symptom: <what failed>
- Reproduction: <exact command>
- Root cause: <short technical cause>
- Fix: <what changed>
- Verification: <commands re-run + results>
```

## 3) Actionable Command Matrix (CI ↔ Local)

Run from repo root unless specified.

```bash
pnpm install --frozen-lockfile
pnpm run lint
pnpm run format:check
pnpm run build
pnpm run test
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
./scripts/smoke-m1.sh
```

## 4) CI Failure Debugging Guide

### A. `test` job (quality/build/test/rust)
- Identify the **first** failing step in Actions logs.
- Reproduce only that command locally.
- If failure is version-sensitive, compare local versions with CI (`Node 22`, stable Rust, Go 1.22.x in smoke job).
- For Rust lint/test noise, prefer crate-level repro after first failure is found:
  - `cargo clippy -p <crate> --all-targets -- -D warnings`
  - `cargo test -p <crate> -- --nocapture`

### B. `smoke-executable` job
- Re-run `./scripts/smoke-m1.sh` locally.
- Use built-in diagnostics:
  - `.tmp-smoke-m1-run.out` and `.tmp-smoke-m1-run.err` when present
  - `examples/generic-simple-cli/dist-go/main.go` head (emission sanity)
  - printed env summary (`TSGODOWN_RUST_ENGINE_BIN`, paths)
- Confirm launcher executable:
  - `test -x ./scripts/rust-engine-launcher.sh`
- Smoke no longer opens an HTTP port while executable JS lowering is fail-closed.

## 5) Smoke-Specific Failure Recipes

### `SMOKE-BUILD`
Symptoms: no `dist-go/main.go`, launcher not executable, CLI build step exits non-zero.

Checklist:
1. `pnpm run build`
2. `cargo build -p engine-core`
3. `echo "$TSGODOWN_RUST_ENGINE_BIN"` and verify executable if explicitly set.
4. Re-run smoke script and read first failing block.

### `SMOKE-RUNTIME`
Symptoms: generated binary does not exit `1`, omits `"unsupported":true`, or omits `EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED`.

Checklist:
1. `cat .tmp-smoke-m1-run.out .tmp-smoke-m1-run.err`
2. Inspect generated Go: `sed -n '1,160p' examples/generic-simple-cli/dist-go/main.go`
3. Build/run manually:
   - `cd examples/generic-simple-cli/dist-go`
   - `go build -o tsgodown-local .`
   - `./tsgodown-local`
4. Validate stdout contains deterministic fail-closed JSON.

## 6) Escalation Rules
- If classified as `CI-INFRA` and reproducibility is absent locally, retry once.
- If still failing, note as infra-suspect in PR and attach logs.
- Do not merge with unresolved `SMOKE-*` or `RUST-*` failures.

## 7) Definition of Done for Failure Closure (compiler-mode)
- Primary category assigned
- Root cause identified
- Fix merged with no compiler-mode contract regression (including no TS-analyzer fallback path)
- Full command matrix passes locally
- CI green on updated branch
- Evidence comment/PR note includes milestone stage in locked sequence (`M5->M1->M2->M3->M4`) and verification commands
