# M1 Release Gate (Canonical Path)

Milestone 1 is release-gated by a **single canonical verification path**:

- Canonical gate intent: `CLI build reference fixture -> dist-go/main.go + tsgodownrt -> go build (if available)`
- Current test id: `M1 release gate: CLI build fastify-scaffold-real fixture -> dist-go/main.go -> go build (if available)`
- Location: `packages/cli/test/commands.e2e.test.ts`
- Runner: `pnpm run gate:m1`
- Script: `scripts/m1-release-gate.sh`

## How to verify M1 locally
From repo root:

```bash
pnpm install
pnpm run gate:m1
```

The canonical command above executes `scripts/m1-release-gate.sh`, which runs the fixed E2E test filtered by `^M1 release gate:`.

Direct invocation (same test path as script):

```bash
cd packages/cli
node --import tsx --test-name-pattern "^M1 release gate:" --test test/commands.e2e.test.ts
```

## What this gate verifies

1. A tracked reference fixture can be built through the CLI + Rust adapter path (current fixture: `examples/fastify-scaffold-real`).
2. `dist-go/main.go` is emitted.
3. Emitted Go target shape is valid (`package main`, `func main`, `go.mod`, `tsgodownrt/runtime.go`).
4. If the Go toolchain exists on the machine, `go build ./...` succeeds.
5. Until executable JS lowering is implemented, the generated binary fails closed with deterministic unsupported-codegen diagnostics.

## Why a single path

- Prevents divergent M1 acceptance definitions across tests.
- Makes release readiness checks fast and deterministic.
- Keeps M1 sign-off criteria explicit for final push/PR verification.
