# Node.js LTS Coverage Ledger

Baseline: Node.js `24.15.0` LTS, official API index:
<https://nodejs.org/docs/latest-v24.x/api/>

This ledger tracks public Node.js LTS surface for `tsgodown`. A row is required
for every documented API area. Completion requires each stable row to be `DONE`
for the Go backend or intentionally `FAIL_CLOSED`/`BLOCKED` with deterministic
diagnostics. `TODO` and `WIP` rows are allowed during development only.

| Key | Area | Stability | Contract Status | Go Status | Diagnostic | Evidence | Notes |
|---|---|---|---|---|---|---|---|
| node.assert | Assertion testing | stable | WIP | WIP | NODE_ASSERT_UNSUPPORTED | focused AOT subset | `assert.equal`, `assert.strictEqual`, and `assert.deepStrictEqual` lower through generic assertion helpers for primitive values and JSON-like arrays/objects. Full AssertionError object shape, messages, custom operators, partial/deep match variants, rejects/throws async behavior, and strict module aliases beyond the focused subset remain pending. |
| node.async_context | Asynchronous context tracking | stable | TODO | TODO | NODE_ASYNC_CONTEXT_UNSUPPORTED | planned | AsyncLocalStorage and async resource propagation. |
| node.async_hooks | Async hooks | stable | TODO | TODO | NODE_ASYNC_HOOKS_UNSUPPORTED | planned | Hook lifecycle and async resource IDs. |
| node.buffer | Buffer | stable | WIP | WIP | NODE_BUFFER_UNSUPPORTED | focused AOT subset | `Buffer.alloc` lowers fixed-size byte-slice allocation with numeric fill, `Buffer.from` lowers string inputs with `utf8`, `hex`, and `base64` encodings plus numeric arrays, and `Buffer.isBuffer` lowers byte-slice predicates; Blob, full encodings, mutation, typed-array interop, and byte-level parity pending. |
| node.addons_cpp | C++ addons | native | BLOCKED | BLOCKED | NODE_CPP_ADDON_BLOCKED | fail-closed planned | Native addon loading conflicts with no native fallback. |
| node.addons_node_api | C/C++ addons with Node-API | native | BLOCKED | BLOCKED | NODE_API_ADDON_BLOCKED | fail-closed planned | Node-API/N-API fallback forbidden. |
| node.embedder_api | C++ embedder API | embedder | BLOCKED | BLOCKED | NODE_EMBEDDER_API_BLOCKED | fail-closed planned | Embedding Node/V8 is forbidden. |
| node.child_process | Child processes | stable | WIP | WIP | NODE_CHILD_PROCESS_UNSUPPORTED | execa corpus subset | spawnSync subset exists; async lifecycle/stdio/IPC pending. |
| node.cluster | Cluster | stable | TODO | TODO | NODE_CLUSTER_UNSUPPORTED | planned | Multi-process coordination semantics. |
| node.cli_options | Command-line options | stable | TODO | TODO | NODE_CLI_OPTIONS_UNSUPPORTED | planned | CLI flags and runtime behavior. |
| node.console | Console | stable | WIP | WIP | NODE_CONSOLE_UNSUPPORTED | corpus stdout subset | Formatting, timing, inspection pending. |
| node.crypto | Crypto | stable | WIP | WIP | NODE_CRYPTO_UNSUPPORTED | uuid corpus subset | Hash/HMAC/random/WebCrypto/key APIs pending. |
| node.debugger | Debugger | tool | FAIL_CLOSED | FAIL_CLOSED | NODE_DEBUGGER_UNSUPPORTED | fail-closed planned | Debug transport not part of generated Go runtime. |
| node.deprecated | Deprecated APIs | legacy | TODO | TODO | NODE_DEPRECATED_API_UNSUPPORTED | planned | Must choose support or fail-closed per API. |
| node.diagnostics_channel | Diagnostics Channel | stable | TODO | TODO | NODE_DIAGNOSTICS_CHANNEL_UNSUPPORTED | planned | Pub/sub diagnostics API. |
| node.dns | DNS | stable | TODO | TODO | NODE_DNS_UNSUPPORTED | planned | Lookup, resolver, promises API. |
| node.domain | Domain | legacy | TODO | TODO | NODE_DOMAIN_UNSUPPORTED | planned | Legacy async error handling. |
| node.env_vars | Environment Variables | stable | WIP | WIP | NODE_ENV_UNSUPPORTED | corpus subset | process.env and CLI env behavior. |
| node.errors | Errors | stable | WIP | WIP | NODE_ERRORS_UNSUPPORTED | corpus error shape subset | Error classes/codes/stack semantics pending. |
| node.events | Events | stable | TODO | TODO | NODE_EVENTS_UNSUPPORTED | planned | EventEmitter ordering/errors/listeners. |
| node.fs | File system | stable | WIP | WIP | NODE_FS_UNSUPPORTED | fs-extra corpus subset | `fs.existsSync` lowers for string-path subsets; `fs.statSync` lowers a focused Stats subset with `mode`, `isFile()`, `isDirectory()`, and `isSymbolicLink()`; full fs/fs.promises/watch/stat/platform semantics pending. |
| node.globals | Globals | stable | WIP | WIP | NODE_GLOBALS_UNSUPPORTED | corpus subset | globalThis, timers, URL, fetch-related globals pending. |
| node.http | HTTP | stable | TODO | TODO | NODE_HTTP_UNSUPPORTED | route-era fixtures only | Full client/server streaming behavior pending. |
| node.http2 | HTTP/2 | stable | TODO | TODO | NODE_HTTP2_UNSUPPORTED | planned | HTTP/2 sessions/streams. |
| node.https | HTTPS | stable | TODO | TODO | NODE_HTTPS_UNSUPPORTED | planned | TLS-backed HTTP client/server. |
| node.inspector | Inspector | tool | FAIL_CLOSED | FAIL_CLOSED | NODE_INSPECTOR_UNSUPPORTED | fail-closed planned | Inspector protocol not compiled into Go. |
| node.intl | Internationalization | stable | TODO | TODO | NODE_INTL_UNSUPPORTED | planned | ICU/Intl behavior. |
| node.module_cjs | Modules: CommonJS modules | stable | WIP | WIP | NODE_CJS_UNSUPPORTED | corpus module graph subset | Require/cache/circular interop pending. |
| node.module_esm | Modules: ECMAScript modules | stable | WIP | WIP | NODE_ESM_UNSUPPORTED | corpus module graph subset | ESM resolution/TLA/import attributes pending. |
| node.module_api | Modules: node:module API | stable | TODO | TODO | NODE_MODULE_API_UNSUPPORTED | planned | createRequire, register hooks, builtin APIs. |
| node.packages | Modules: Packages | stable | WIP | WIP | NODE_PACKAGE_RESOLUTION_UNSUPPORTED | corpus package graph subset | exports/imports/type/main/module exact parity pending. |
| node.typescript | Modules: TypeScript | stable | TODO | TODO | NODE_TYPESCRIPT_MODULE_UNSUPPORTED | planned | Node LTS TypeScript module handling and tsdown artifact mapping. |
| node.net | Net | stable | TODO | TODO | NODE_NET_UNSUPPORTED | planned | TCP/IPC sockets. |
| node.os | OS | stable | WIP | WIP | NODE_OS_UNSUPPORTED | focused AOT subset | `os.homedir` lowers to a Go helper; broader OS info and platform differences pending. |
| node.path | Path | stable | WIP | WIP | NODE_PATH_UNSUPPORTED | corpus subset | `path.basename`, `path.delimiter`, `path.dirname`, `path.isAbsolute`, `path.join`, `path.normalize`, `path.parse`, `path.relative`, `path.resolve`, `path.sep`, `path.posix.sep`, and `path.win32.sep` lower for string-path subsets; full POSIX/win32 edge cases pending. |
| node.perf_hooks | Performance hooks | stable | TODO | TODO | NODE_PERF_HOOKS_UNSUPPORTED | planned | Performance timeline/observer. |
| node.permissions | Permissions | experimental | TODO | TODO | NODE_PERMISSIONS_UNSUPPORTED | planned | Permission model decisions required. |
| node.process | Process | stable | WIP | WIP | NODE_PROCESS_UNSUPPORTED | corpus argv/env/cwd subset | `process.arch`, `process.chdir`, `process.env`, `process.cwd`, `process.cwd()`, `process.execPath`, `process.getgid()`, `process.getuid()`, `process.version`, `process.versions`, `process.versions.node`, `process.platform`, `process.stdin`, `process.stdout`, `process.stderr`, no-IPC `process.channel`, and process function ref truthiness/`typeof` have focused AOT coverage for the Node.js 24.15.0 LTS target; signals, lifecycle, full stdio stream behavior, and broader process object behavior pending. |
| node.punycode | Punycode | deprecated | TODO | TODO | NODE_PUNYCODE_UNSUPPORTED | planned | Deprecated module support decision pending. |
| node.querystring | Query strings | stable | TODO | TODO | NODE_QUERYSTRING_UNSUPPORTED | planned | Legacy querystring parser/stringifier. |
| node.readline | Readline | stable | TODO | TODO | NODE_READLINE_UNSUPPORTED | planned | TTY/input interfaces. |
| node.repl | REPL | tool | FAIL_CLOSED | FAIL_CLOSED | NODE_REPL_UNSUPPORTED | fail-closed planned | Interactive REPL not generated runtime. |
| node.report | Report | stable | TODO | TODO | NODE_REPORT_UNSUPPORTED | planned | Diagnostic reports. |
| node.sea | Single executable applications | stable | FAIL_CLOSED | FAIL_CLOSED | NODE_SEA_UNSUPPORTED | fail-closed planned | Node SEA artifact is not Go output contract. |
| node.sqlite | SQLite | stable | TODO | TODO | NODE_SQLITE_UNSUPPORTED | planned | Built-in SQLite API. |
| node.stream | Stream | stable | TODO | TODO | NODE_STREAM_UNSUPPORTED | planned | Node streams, pipeline, backpressure. |
| node.string_decoder | String decoder | stable | TODO | TODO | NODE_STRING_DECODER_UNSUPPORTED | planned | Incremental string decoding. |
| node.test_runner | Test runner | stable | TODO | TODO | NODE_TEST_RUNNER_UNSUPPORTED | planned | Needed when compiling tests/tools. |
| node.timers | Timers | stable | WIP | WIP | NODE_TIMERS_UNSUPPORTED | async subset | setTimeout/setInterval/immediate ordering pending. |
| node.tls | TLS/SSL | stable | TODO | TODO | NODE_TLS_UNSUPPORTED | planned | TLS sockets/certs. |
| node.trace_events | Trace events | stable | TODO | TODO | NODE_TRACE_EVENTS_UNSUPPORTED | planned | Trace event API. |
| node.tty | TTY | stable | TODO | TODO | NODE_TTY_UNSUPPORTED | planned | TTY streams/window sizing. |
| node.dgram | UDP/datagram | stable | TODO | TODO | NODE_DGRAM_UNSUPPORTED | planned | UDP sockets. |
| node.url | URL | stable | WIP | WIP | NODE_URL_UNSUPPORTED | corpus subset | URL/URLSearchParams/file URL parity pending. |
| node.util | Utilities | stable | TODO | TODO | NODE_UTIL_UNSUPPORTED | planned | inspect, promisify, callbackify, types. |
| node.v8 | V8 | engine | BLOCKED | BLOCKED | NODE_V8_BLOCKED | fail-closed planned | V8 API conflicts with no V8 fallback. |
| node.vm | VM | engine | FAIL_CLOSED | FAIL_CLOSED | NODE_VM_UNSUPPORTED | fail-closed planned | Dynamic evaluation sandbox needs separate source-level strategy. |
| node.wasi | WASI | stable | TODO | TODO | NODE_WASI_UNSUPPORTED | planned | WASI integration decision pending. |
| node.webcrypto | Web Crypto API | stable | TODO | TODO | NODE_WEBCRYPTO_UNSUPPORTED | planned | SubtleCrypto and key APIs. |
| node.webstreams | Web Streams API | stable | TODO | TODO | NODE_WEBSTREAMS_UNSUPPORTED | planned | WHATWG streams. |
| node.worker_threads | Worker threads | stable | TODO | TODO | NODE_WORKER_THREADS_UNSUPPORTED | planned | Workers, shared memory, message channels. |
| node.zlib | Zlib | stable | TODO | TODO | NODE_ZLIB_UNSUPPORTED | planned | Compression streams and sync APIs. |

## Gate Rules

- Required rows are enforced by `pnpm run gate:node-lts-coverage-ledger`.
- `TODO` and `WIP` are allowed while developing.
- Final mode must reject `TODO` and `WIP`:

```bash
node scripts/check-ledger.mjs node-lts --final
```
