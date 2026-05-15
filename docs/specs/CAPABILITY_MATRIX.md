# Capability Matrix (SSoT-2)

Single table that decides whether backend-neutral IR semantics can be lowered to a target backend.

| Capability Key | Scope | Contract Status | Contract Strategy | Go Status | Go Strategy | Rust Status | Rust Strategy | C++ Status | C++ Strategy |
|---|---|---|---|---|---|---|---|---|---|
| route.basic | HTTP route | WIP | direct mapping | WIP | direct mapping | TODO | backend not implemented | TODO | backend not implemented |
| handler.async | control-flow | TODO | goroutine + await shim | TODO | goroutine + await shim | TODO | backend not implemented | TODO | backend not implemented |
| module.esm | module | WIP | static link graph | WIP | static link graph | TODO | backend not implemented | TODO | backend not implemented |
| module.cjs | module | TODO | cjs bridge | TODO | cjs bridge | TODO | backend not implemented | TODO | backend not implemented |
| runtime.event_loop | runtime | TODO | scheduler shim | TODO | scheduler shim | TODO | backend not implemented | TODO | backend not implemented |
| node.fs.basic | node api | TODO | os/io adapter | TODO | os/io adapter | TODO | backend not implemented | TODO | backend not implemented |
| node.path.basic | node api | WIP | filepath adapter (join/resolve/dirname/basename) | WIP | filepath adapter (join/resolve/dirname/basename) | TODO | backend not implemented | TODO | backend not implemented |
| node.url.basic | node api | WIP | net/url adapter (URL + URLSearchParams) | WIP | net/url adapter (URL + URLSearchParams) | TODO | backend not implemented | TODO | backend not implemented |
| node.process.env | node api | TODO | runtime env map | TODO | runtime env map | TODO | backend not implemented | TODO | backend not implemented |
| node.buffer.basic | node api | TODO | []byte wrapper | TODO | []byte wrapper | TODO | backend not implemented | TODO | backend not implemented |

## Decision rules
- If required capabilities for an IR node are not `DONE|WIP(allow)`, compilation fails.
- On failure, include source-map-based original location in diagnostics.
- Capability decision execution is based on the Rust core runtime path.
- Do not bypass Rust path failure with TS analyzer fallback (no fallback).
- `node-compat` capability checker defaults: `allowWip=true`, `failFast=true`, `targetBackend=go`.
- IR capability keys and runtime helper contracts stay backend-neutral; backend-specific lowering status lives in backend columns.
