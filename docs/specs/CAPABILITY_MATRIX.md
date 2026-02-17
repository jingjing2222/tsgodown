# Capability Matrix (SSoT-2)

Single table that decides whether IR semantics can be lowered to Go.

| Capability Key | Scope | Status | Strategy |
|---|---|---|---|
| route.basic | HTTP route | WIP | direct mapping |
| handler.async | control-flow | TODO | goroutine + await shim |
| module.esm | module | WIP | static link graph |
| module.cjs | module | TODO | cjs bridge |
| runtime.event_loop | runtime | TODO | scheduler shim |
| node.fs.basic | node api | TODO | os/io adapter |
| node.path.basic | node api | WIP | filepath adapter (join/resolve/dirname/basename) |
| node.url.basic | node api | WIP | net/url adapter (URL + URLSearchParams) |
| node.process.env | node api | TODO | runtime env map |
| node.buffer.basic | node api | TODO | []byte wrapper |

## Decision rules
- If required capabilities for an IR node are not `DONE|WIP(allow)`, compilation fails.
- On failure, include source-map-based original location in diagnostics.
- Capability decision execution is based on the Rust core runtime path.
- Do not bypass Rust path failure with TS analyzer fallback (no fallback).
- `node-compat` capability checker defaults: `allowWip=true`, `failFast=true`.
