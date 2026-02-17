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
- Runtime line includes selected port, for example: `[fastify-complex] run go runtime (port=18081)`
- Route checks pass:
  - `GET /health -> 501` with body containing `TODO implement handler health for GET /health`
  - `POST /users -> 501` with body containing `TODO implement handler createUser for POST /users`
  - `PATCH /users/123 -> 501` with body containing `TODO implement handler updateUser for PATCH /users/:id`
  - `DELETE /users/123 -> 501` with body containing `TODO implement handler removeUser for DELETE /users/:id`
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
- `configured port <port> is already in use`
  - You set `FASTIFY_COMPLEX_PORT` to a busy port. Pick a free one and rerun.
- `default port 18081 is in use; auto-selecting port <port>`
  - Informational warning only; script continues on the auto-selected free port.
- Route status/body mismatch
  - Check `.tmp-fastify-complex-server.log` and generated `dist-go/main.go` to confirm emitted route signatures and TODO handler text.

## Optional overrides
- `FASTIFY_COMPLEX_PORT` (default `18081`)
- `TSGODOWN_RUST_ENGINE_BIN` (default `scripts/rust-engine-launcher.sh`)
- `TSGODOWN_ENGINE_CORE_BIN` (default `target/debug/engine-core`)
