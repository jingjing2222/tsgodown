# Testing Strategy (TDD First)

## Non-negotiable rule
All feature implementation must follow the **Test First** principle.

## Architecture guardrails (M4)
- Rust core is the **only** runtime analysis/build engine.
- TypeScript runtime code is orchestration/UI only.
- **Fail-closed policy:** runtime path must not fall back to any TypeScript analyzer path on Rust failures.

## Workflow
1. Write a failing test
2. Make the minimal implementation to pass the test
3. Refactor
4. Add regression tests

## Test layers
- Unit: pure logic at package scope (`packages/*/test`)
- Integration: rust adapter contract + pipeline orchestration
- E2E: convert real example projects and verify the CLI/build contract

## M1 release gate (Fastify -> Go compile success path)
M1 release gate is fixed to the **single canonical path** below.

- Canonical reference: [`M1_RELEASE_GATE.md`](M1_RELEASE_GATE.md)
- Script entrypoint: [`scripts/m1-release-gate.sh`](../../scripts/m1-release-gate.sh)
- Test location: `packages/cli/test/commands.e2e.test.ts`
- Test name: `M1 release gate: CLI build fastify-min fixture -> dist-go/main.go -> go build (if available)`
- Command: `pnpm run gate:m1`

Verification items:
1. Input: Fastify-min fixture TypeScript entry (`src/index.ts`)
2. Execution: run CLI `build` through the Rust adapter path
3. Output: confirm `dist-go/main.go` generation
4. Assertions:
   - Go scaffold shape (`package main`, `func main()`, `GET /health` route binding)
   - if the Go toolchain exists, `go build ./...` succeeds

Note: the gate is limited to execution-path verification and does not cover internal implementation details of `analyzer-rust` / `emitter-go`.

## M3 runtime correctness/stability extension
- Runtime executable-fixture coverage is extended beyond the M1/M2 happy-path checks.
- Primary test location: `packages/cli/test/commands.e2e.test.ts`
- Key acceptance tests:
  - `M2 acceptance: TS fixture routes are reachable in generated Go runtime`
  - `M3 acceptance: runtime method/path matrix fixture remains deterministic`
- Deterministic assertions must include:
  - method-aware route checks (`GET`, `POST`, `PUT`)
  - scaffold TODO body checks for named handlers
  - negative path behavior (`405 Method Not Allowed`, `404 page not found`)
- `scripts/smoke-m1.sh` must validate deterministic route behavior (`/health`, `/users`, `/missing`) instead of single-endpoint liveness only.

## analyzer-rust boundary contract (M1)
- Keep `packages/analyzer-rust/tests/contract_parity_regression.rs` as the fixed SSoT contract test for analyzer-rust.
- Fix supported and unsupported boundaries using fixture-based tests.
- Unsupported boundaries must include **DiagnosticIR.code mapping** to prevent regressions.
  - e.g. `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`, `ANALYZER_UNSUPPORTED_INLINE_HANDLER`, `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`
- analyzer-rust does not emit capability policy diagnostics (`CAPABILITY_*`).

## Required checks per PR/turn
- `pnpm install --frozen-lockfile`
- `pnpm run lint`
- `pnpm run format:check`
- `pnpm run build`
- `pnpm run test`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `./scripts/smoke-m1.sh`

## Failure handling
- Do not report features as complete when tests fail.
- Report the 3-item set: failure cause / reproduction command / mitigation plan.
- Classify and triage using [`docs/operations/FAILURE_TRIAGE_PLAYBOOK.md`](../operations/FAILURE_TRIAGE_PLAYBOOK.md).
