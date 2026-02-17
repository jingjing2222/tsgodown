# M1 Release Gate (Canonical Path)

Milestone 1 is release-gated by a **single canonical verification path**:

- Test name: `M1 release gate: CLI build fastify-min fixture -> dist-go/main.go -> go build (if available)` (current reference fixture name)
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

1. A tracked reference fixture can be built through the CLI + Rust adapter path (currently `examples/fastify-min`).
2. `dist-go/main.go` is emitted.
3. Emitted Go scaffold shape is valid (`package main`, `func main`, health route scaffold).
4. If the Go toolchain exists on the machine, `go build ./...` succeeds.

## Why a single path

- Prevents divergent M1 acceptance definitions across tests.
- Makes release readiness checks fast and deterministic.
- Keeps M1 sign-off criteria explicit for final push/PR verification.
