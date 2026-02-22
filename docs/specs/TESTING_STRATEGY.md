# Testing Strategy (TDD First)

## Non-negotiable rule
All feature implementation must follow the **Test First** principle.

## Milestone lock (M5 -> M1 -> M2 -> M3 -> M4)
- This strategy follows the locked execution/reporting sequence: `M5 -> M1 -> M2 -> M3 -> M4`.
- M5 defines compiler-mode contracts and fail-closed policy.
- M1/M2/M3 define executable proof expansion.
- M4 enforces architecture guardrails + release/triage DoD discipline.

## Architecture guardrails (M4)
- Rust core is the **only** runtime analysis/build engine.
- TypeScript runtime code is orchestration/UI only.
- **Fail-closed policy:** runtime path must not use any TypeScript analyzer fallback path on Rust failures.
- **Framework-agnostic core path:** `packages/core/src`, `packages/pipeline/src`, and `packages/cli/src/commands` must not introduce framework-name branching/adapters; enforced by `scripts/guard-core-path-no-framework-branching.mjs`.

## Workflow
1. Write a failing test
2. Make the minimal implementation to pass the test
3. Refactor
4. Add regression tests

## Test layers
- Unit: pure logic at package scope (`packages/*/test`)
- Integration: rust adapter contract + pipeline orchestration
- E2E: convert real example projects and verify the CLI/build contract

## M1 release gate (TS service artifact fixture -> Go compile success path)
M1 release gate is fixed to the **single canonical path** below.

- Canonical reference: [`M1_RELEASE_GATE.md`](M1_RELEASE_GATE.md)
- Script entrypoint: [`scripts/m1-release-gate.sh`](../../scripts/m1-release-gate.sh)
- Test location: `packages/cli/test/commands.e2e.test.ts`
- Canonical gate intent: `CLI build reference fixture -> dist-go/main.go -> go build (if available)`
- Current test id: `M1 release gate: CLI build fastify-scaffold-real fixture -> dist-go/main.go -> go build (if available)`
- Command: `pnpm run gate:m1`

Verification items:
1. Input: reference fixture TypeScript entry (`src/index.ts`) from tracked examples (current fixture: `examples/fastify-scaffold-real`)
2. Execution: run CLI `build` through the Rust adapter path
3. Output: confirm `dist-go/main.go` generation
4. Assertions:
   - Go scaffold shape (`package main`, `func main()`, `GET /health` route binding)
   - if the Go toolchain exists, `go build ./...` succeeds

Note: the gate is limited to execution-path verification and does not cover internal implementation details of `analyzer-rust` / `emitter-go`.

## M3 runtime correctness/stability extension
- Runtime executable-fixture coverage is extended beyond the M1/M2 happy-path checks.
- M3+ parity suites act as a semantics-parity ratchet: regressions are blocked and previously proven behavior stays protected as a global safety net.
- Normative parity definition: [`SEMANTIC_PARITY_CONTRACT.md`](./SEMANTIC_PARITY_CONTRACT.md)

## M4 semantics parity harness skeleton (global safety net)
- Entrypoint: `scripts/differential-harness.mjs`
- Representative default scenario: `generic-simple-cli-get-health` (framework fixtures remain supplemental parity samples)
- Deterministic report contract:
  - `version: "m4-differential-harness.v1"`
  - stable `summary` (`total`, `matched`, `mismatched`, `pass`)
  - sorted `cases[]` with normalized headers and explicit `diffs[]`
- Fail conditions (fail-closed): missing TS/Go case, status mismatch, headers mismatch, body mismatch.
- Local run: `pnpm run harness:semantics-parity` (legacy alias: `pnpm run harness:differential`)
- Coverage ratchet gate: `pnpm run gate:coverage-ratchet`
  - baseline artifact: `profiles/differential-coverage-baseline.json`
  - fail-closed checks: minimum scenario count, minimum case count, required scenario ids
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
- `pnpm run gate:semantics-parity`
- `pnpm run gate:compliance`
- `./scripts/smoke-m1.sh`

## Failure handling
- Do not report features as complete when tests fail.
- Report the 3-item set: failure cause / reproduction command / mitigation plan.
- Classify and triage using [`docs/operations/FAILURE_TRIAGE_PLAYBOOK.md`](../operations/FAILURE_TRIAGE_PLAYBOOK.md).


## Install-first workspace coverage
- CI install-first checks must run against **tracked example workspaces** discovered from `examples/*/tsgodown.config.ts` (not a framework-specific allowlist).
- This guarantees newly added workspaces are included automatically once added to git.
- If a planned workspace is not yet present, keep TODO hooks in check scripts/docs instead of baking framework-specific assumptions into the gate language.
