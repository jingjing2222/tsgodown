# Fastify Complex Operator Runbook

## Goal
Run a single command that validates the end-to-end developer experience for TypeScript users who do not need Go knowledge.

## Command
From repo root:

```bash
pnpm run devx:fastify-complex
```

## What "success" looks like
- Terminal ends with: `[fastify-complex] PASS`
- Generated file exists: `examples/fastify-complex/dist-go/main.go`
- Route checks pass:
  - `GET /health -> 501`
  - `POST /users -> 501`
  - `PUT /users/:id -> 501`
  - `DELETE /users/:id -> 501`
  - `GET /users -> 405`
  - `GET /missing -> 404`

## Failure triage (cause → fix)
- `missing required command '<tool>'`
  - Install the missing tool (`node`, `pnpm`, `cargo`, `go`, `curl`) and rerun.
- `TSGODOWN_RUST_ENGINE_BIN is not executable`
  - Set `TSGODOWN_RUST_ENGINE_BIN` to `scripts/rust-engine-launcher.sh` or `chmod +x` your custom launcher.
- `TSGODOWN_ENGINE_CORE_BIN is not executable`
  - Run `cargo build -p engine-core` and rerun.
- `dist-go/main.go was not generated`
  - Inspect CLI stderr for Rust adapter contract errors and verify launcher/env configuration.
- Route status/body mismatch
  - Check `.tmp-fastify-complex-server.log` and generated `dist-go/main.go` to confirm emitted route signatures.

## Optional overrides
- `FASTIFY_COMPLEX_PORT` (default `18081`)
- `TSGODOWN_RUST_ENGINE_BIN` (default `scripts/rust-engine-launcher.sh`)
- `TSGODOWN_ENGINE_CORE_BIN` (default `target/debug/engine-core`)
