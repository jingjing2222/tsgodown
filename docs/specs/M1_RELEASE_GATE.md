# M1 Release Gate (Canonical Path)

Milestone 1 is release-gated by a **single canonical verification path**:

- Test name: `M1 release gate: CLI build fastify-min fixture -> dist-go/main.go -> go build (if available)`
- Location: `packages/cli/test/commands.e2e.test.ts`
- Runner: `pnpm run gate:m1`

## What this gate verifies

1. Fastify-min fixture can be built through the CLI + Rust adapter path.
2. `dist-go/main.go` is emitted.
3. Emitted Go scaffold shape is valid (`package main`, `func main`, health route scaffold).
4. If the Go toolchain exists on the machine, `go build ./...` succeeds.

## Why a single path

- Prevents divergent M1 acceptance definitions across tests.
- Makes release readiness checks fast and deterministic.
- Keeps M1 sign-off criteria explicit for final push/PR verification.
