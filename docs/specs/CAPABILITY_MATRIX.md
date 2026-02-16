# Capability Matrix (SSoT-2)

IR 의미를 Go로 내릴 수 있는지 판정하는 단일 테이블.

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

## 판정 규칙
- IR 노드가 요구하는 capability가 `DONE|WIP(allow)`가 아니면 컴파일 실패.
- 실패 시 source map 기반 원본 위치를 진단에 포함.
- capability 판정 실행 주체는 Rust core 런타임 경로를 기준으로 한다.
- Rust path 실패를 TS analyzer fallback으로 우회하지 않는다 (no fallback).
- `node-compat` capability checker 기본값: `allowWip=true`, `failFast=true`.
