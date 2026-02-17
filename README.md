# tsgodown

A long-term TypeScript/JavaScript → Go compiler project built around tsdown artifacts (bundle + sourcemap + d.ts), with a Fastify-first profile and an SSoT-driven architecture.

## Project Goals
- Keep a tsdown-like DX: `defineConfig` + `tsgodown.config.ts`
- Build a semantic compiler pipeline: artifacts → IR → capability gate → Go emitter
- Keep profile adapters thin (framework parsing only; no policy/rule ownership)
- Enforce TDD and CI as merge gates

## Current Status (early skeleton)
- Config loading/normalization
- Basic Fastify route detection
- Go `main.go` scaffold emission
- Initial SSoT docs:
  - [`docs/specs/IR_SPEC.md`](docs/specs/IR_SPEC.md)
  - [`docs/specs/CAPABILITY_MATRIX.md`](docs/specs/CAPABILITY_MATRIX.md)
  - [`docs/specs/ARTIFACT_SCHEMA.md`](docs/specs/ARTIFACT_SCHEMA.md)

## Documentation Map
- Architecture overview: [`docs/architecture/OVERVIEW.md`](docs/architecture/OVERVIEW.md)
- Testing strategy: [`docs/specs/TESTING_STRATEGY.md`](docs/specs/TESTING_STRATEGY.md)
- M1 release gate (canonical): [`docs/specs/M1_RELEASE_GATE.md`](docs/specs/M1_RELEASE_GATE.md)
- Release workflow/versioning policy: [`docs/specs/RELEASE_WORKFLOW.md`](docs/specs/RELEASE_WORKFLOW.md)
- Observability / failure triage playbook: [`docs/operations/FAILURE_TRIAGE_PLAYBOOK.md`](docs/operations/FAILURE_TRIAGE_PLAYBOOK.md)
- Performance baseline scaffold: [`docs/specs/PERFORMANCE_BASELINE.md`](docs/specs/PERFORMANCE_BASELINE.md)
- Fastify complex operator runbook: [`docs/FASTIFY_COMPLEX_RUNBOOK.md`](docs/FASTIFY_COMPLEX_RUNBOOK.md)

## Quick Start
```bash
pnpm install
pnpm run build
cd examples/fastify-min
node --import tsx ../../packages/cli/src/index.ts build
```

Generated output:
- `examples/fastify-min/dist-go/main.go`

## Local Rust Engine Launcher (no temp scripts)
Use the reusable launcher when you want local CLI runs to go through `engine-core analyze` and still emit `dist-go/main.go`.

```bash
cargo build -p engine-core
export TSGODOWN_RUST_ENGINE_BIN="$(pwd)/scripts/rust-engine-launcher.sh"
# Optional override (default is ./target/debug/engine-core)
export TSGODOWN_ENGINE_CORE_BIN="$(pwd)/target/debug/engine-core"

cd examples/fastify-min
node --import tsx ../../packages/cli/src/index.ts build
```

If setup is wrong, the launcher fails fast with actionable errors (missing executable, bad JSON request, or `engine-core analyze` failure).

## Development Commands
- `pnpm run lint`
- `pnpm run format:check`
- `pnpm run test:tdd`
- `pnpm run perf:baseline`
- `pnpm run devx:fastify-complex` (one-command build + run + verify for `examples/fastify-complex`)

## Fastify Complex DevX Quickstart (one command)
From repo root:

```bash
pnpm run devx:fastify-complex
```

This command:
- builds TS workspace + Rust `engine-core`
- builds `examples/fastify-complex` to `dist-go/main.go`
- compiles + runs the Go binary
- verifies deterministic routes (`/health`, `/users`, `/users/:id`, method-mismatch 405, missing-route 404)
- prints actionable errors in `cause + fix hint` format

## M1 Local Smoke Verification (Apple Silicon / M1 path)
Run the one-command local smoke script from repo root:

```bash
./scripts/smoke-m1.sh
```

What it does:
- preflight checks (`node`, `pnpm`, `cargo`, `go`, `curl`) and Rust launcher env (`TSGODOWN_RUST_ENGINE_BIN`, auto-generated when unset)
- builds TS packages + Rust `engine-core`
- builds `examples/fastify-min` into `dist-go/main.go`
- runs `go build` in `examples/fastify-min/dist-go`
- starts the binary on configurable port (`SMOKE_PORT`, default `18080`)
- calls `/health` and verifies `200` + body `ok`
- performs graceful teardown and prints diagnostics on failure

Optional env overrides:
- `SMOKE_PORT` (default: `18080`)
- `SMOKE_EXPECTED_BODY` (default: `ok`)

## How to verify M1 locally
Use the canonical gate command from repo root:

```bash
pnpm run gate:m1
```

This executes [`scripts/m1-release-gate.sh`](scripts/m1-release-gate.sh), which runs the fixed M1 acceptance test in `packages/cli/test/commands.e2e.test.ts` (name prefix: `M1 release gate:`).

If you need the exact direct test invocation used by the script:

```bash
cd packages/cli
node --import tsx --test-name-pattern "^M1 release gate:" --test test/commands.e2e.test.ts
```

## Migration Note
- Legacy package `@tsgodown/ir` is deprecated and intentionally inactive.
- Legacy TypeScript core analyze/capability/emit paths are deprecated and disabled in `@tsgodown/core`/`@tsgodown/pipeline` (orchestration/UI only).
- Active IR model/package is `@tsgodown/ir-core` and policy SSoT remains [`IR_SPEC.md`](docs/specs/IR_SPEC.md) + [`CAPABILITY_MATRIX.md`](docs/specs/CAPABILITY_MATRIX.md).

## Workspace Package Policy
- Placeholder packages are not kept as empty directories.
- If a package is not implemented yet, track it in docs/backlog only (do not create `packages/<name>` until there is executable scaffold/code).
- `packages/artifact-indexer`, `packages/go-emitter`, `packages/runtime-go`, and `packages/test-harness` are intentionally absent under this policy.
- Current Go emitter package path is `packages/emitter-go`.

## License
MIT
