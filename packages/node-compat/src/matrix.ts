import {
  CAPABILITY_BACKENDS,
  CAPABILITY_KEYS,
  type CapabilityBackend,
  type CapabilityBackendRule,
  type CapabilityKey,
  type CapabilityRule,
  CapabilityStatus,
} from "./types.js";

function backendRules(
  go: CapabilityBackendRule,
  overrides: Partial<Record<CapabilityBackend, CapabilityBackendRule>> = {},
): Record<CapabilityBackend, CapabilityBackendRule> {
  const defaults = Object.fromEntries(
    CAPABILITY_BACKENDS.map((backend) => [
      backend,
      { status: CapabilityStatus.TODO, strategy: "backend not implemented" },
    ]),
  ) as Record<CapabilityBackend, CapabilityBackendRule>;
  return {
    ...defaults,
    go,
    ...overrides,
  };
}

/**
 * Generated from docs/specs/* ledger files by scripts/sync-capability-matrix.mjs.
 * Do not edit this table manually.
 */
export const CAPABILITY_MATRIX: Record<CapabilityKey, CapabilityRule> = {
  "route.basic": {
    key: "route.basic",
    scope: "HTTP route",
    status: CapabilityStatus.WIP,
    strategy: "legacy route compatibility alias",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "legacy route compatibility alias",
    }),
  },
  "handler.async": {
    key: "handler.async",
    scope: "control-flow",
    status: CapabilityStatus.TODO,
    strategy: "legacy async handler alias; superseded by es.async.* ledgers",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "legacy async handler alias; superseded by es.async.* ledgers",
    }),
  },
  "module.esm": {
    key: "module.esm",
    scope: "module",
    status: CapabilityStatus.WIP,
    strategy: "legacy ESM alias; superseded by node.module_esm and es.modules",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        "legacy ESM alias; superseded by node.module_esm and es.modules",
    }),
  },
  "module.cjs": {
    key: "module.cjs",
    scope: "module",
    status: CapabilityStatus.TODO,
    strategy: "legacy CJS alias; superseded by node.module_cjs",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "legacy CJS alias; superseded by node.module_cjs",
    }),
  },
  "runtime.event_loop": {
    key: "runtime.event_loop",
    scope: "runtime",
    status: CapabilityStatus.TODO,
    strategy:
      "legacy event-loop alias; superseded by es.async.* and node.timers",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy:
        "legacy event-loop alias; superseded by es.async.* and node.timers",
    }),
  },
  "node.fs.basic": {
    key: "node.fs.basic",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "legacy fs alias; superseded by node.fs",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "legacy fs alias; superseded by node.fs",
    }),
  },
  "node.path.basic": {
    key: "node.path.basic",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "legacy path alias; superseded by node.path",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "legacy path alias; superseded by node.path",
    }),
  },
  "node.url.basic": {
    key: "node.url.basic",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "legacy url alias; superseded by node.url",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "legacy url alias; superseded by node.url",
    }),
  },
  "node.process.env": {
    key: "node.process.env",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy:
      "legacy process.env alias; superseded by node.process and node.env_vars",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy:
        "legacy process.env alias; superseded by node.process and node.env_vars",
    }),
  },
  "node.buffer.basic": {
    key: "node.buffer.basic",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "legacy buffer alias; superseded by node.buffer",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "legacy buffer alias; superseded by node.buffer",
    }),
  },
  "node.assert": {
    key: "node.assert",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Assertion testing",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        "`assert.equal`, `assert.strictEqual`, and `assert.deepStrictEqual` lower through generic assertion helpers for primitive values and JSON-like arrays/objects. Full AssertionError object shape, messages, custom operators, partial/deep match variants, rejects/throws async behavior, and strict module aliases beyond the focused subset remain pending.",
    }),
  },
  "node.async_context": {
    key: "node.async_context",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Asynchronous context tracking",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "AsyncLocalStorage and async resource propagation.",
    }),
  },
  "node.async_hooks": {
    key: "node.async_hooks",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Async hooks",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Hook lifecycle and async resource IDs.",
    }),
  },
  "node.buffer": {
    key: "node.buffer",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Buffer",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        "`Buffer.alloc` lowers fixed-size byte-slice allocation with numeric fill, `Buffer.from` lowers string inputs with `utf8`, `hex`, and `base64` encodings plus numeric arrays and index reads, and `Buffer.isBuffer` lowers byte-slice predicates; Blob, full encodings, mutation, typed-array interop, and byte-level parity pending.",
    }),
  },
  "node.addons_cpp": {
    key: "node.addons_cpp",
    scope: "node api",
    status: CapabilityStatus.BLOCKED,
    strategy: "node-lts: C++ addons",
    backends: backendRules({
      status: CapabilityStatus.BLOCKED,
      strategy: "Native addon loading conflicts with no native fallback.",
    }),
  },
  "node.addons_node_api": {
    key: "node.addons_node_api",
    scope: "node api",
    status: CapabilityStatus.BLOCKED,
    strategy: "node-lts: C/C++ addons with Node-API",
    backends: backendRules({
      status: CapabilityStatus.BLOCKED,
      strategy: "Node-API/N-API fallback forbidden.",
    }),
  },
  "node.embedder_api": {
    key: "node.embedder_api",
    scope: "node api",
    status: CapabilityStatus.BLOCKED,
    strategy: "node-lts: C++ embedder API",
    backends: backendRules({
      status: CapabilityStatus.BLOCKED,
      strategy: "Embedding Node/V8 is forbidden.",
    }),
  },
  "node.child_process": {
    key: "node.child_process",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Child processes",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "spawnSync subset exists; async lifecycle/stdio/IPC pending.",
    }),
  },
  "node.cluster": {
    key: "node.cluster",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Cluster",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Multi-process coordination semantics.",
    }),
  },
  "node.cli_options": {
    key: "node.cli_options",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Command-line options",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "CLI flags and runtime behavior.",
    }),
  },
  "node.console": {
    key: "node.console",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Console",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Formatting, timing, inspection pending.",
    }),
  },
  "node.crypto": {
    key: "node.crypto",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Crypto",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        '`crypto.createHash(...).update(...).digest("hex")` lowers md5/sha1/sha256 string digests, `crypto.randomFillSync` lowers byte fill, and `crypto.randomUUID` lowers UUID v4 generation for focused AOT subsets. HMAC, broader random APIs, WebCrypto, keys, streaming hash objects, and error shapes pending.',
    }),
  },
  "node.debugger": {
    key: "node.debugger",
    scope: "node api",
    status: CapabilityStatus.FAIL_CLOSED,
    strategy: "node-lts: Debugger",
    backends: backendRules({
      status: CapabilityStatus.FAIL_CLOSED,
      strategy: "Debug transport not part of generated Go runtime.",
    }),
  },
  "node.deprecated": {
    key: "node.deprecated",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Deprecated APIs",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Must choose support or fail-closed per API.",
    }),
  },
  "node.diagnostics_channel": {
    key: "node.diagnostics_channel",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Diagnostics Channel",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Pub/sub diagnostics API.",
    }),
  },
  "node.dns": {
    key: "node.dns",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: DNS",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Lookup, resolver, promises API.",
    }),
  },
  "node.domain": {
    key: "node.domain",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Domain",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Legacy async error handling.",
    }),
  },
  "node.env_vars": {
    key: "node.env_vars",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Environment Variables",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "process.env and CLI env behavior.",
    }),
  },
  "node.errors": {
    key: "node.errors",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Errors",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Error classes/codes/stack semantics pending.",
    }),
  },
  "node.events": {
    key: "node.events",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Events",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "EventEmitter ordering/errors/listeners.",
    }),
  },
  "node.fs": {
    key: "node.fs",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: File system",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        "`fs.existsSync` lowers for string-path subsets; `fs.statSync` lowers a focused Stats subset with `mode`, `isFile()`, `isDirectory()`, and `isSymbolicLink()`; `fs.readFileSync` lowers string reads for member and named-import subsets; awaited `fs.promises.readFile`, `fs.promises.writeFile`, and `fs.promises.readdir` lower for sequential string-path/string-data/string-name-array subsets. Full fs/fs.promises/watch/stat/platform semantics pending.",
    }),
  },
  "node.globals": {
    key: "node.globals",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Globals",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "globalThis, timers, URL, fetch-related globals pending.",
    }),
  },
  "node.http": {
    key: "node.http",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: HTTP",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Full client/server streaming behavior pending.",
    }),
  },
  "node.http2": {
    key: "node.http2",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: HTTP/2",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "HTTP/2 sessions/streams.",
    }),
  },
  "node.https": {
    key: "node.https",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: HTTPS",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "TLS-backed HTTP client/server.",
    }),
  },
  "node.inspector": {
    key: "node.inspector",
    scope: "node api",
    status: CapabilityStatus.FAIL_CLOSED,
    strategy: "node-lts: Inspector",
    backends: backendRules({
      status: CapabilityStatus.FAIL_CLOSED,
      strategy: "Inspector protocol not compiled into Go.",
    }),
  },
  "node.intl": {
    key: "node.intl",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Internationalization",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "ICU/Intl behavior.",
    }),
  },
  "node.module_cjs": {
    key: "node.module_cjs",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Modules: CommonJS modules",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Require/cache/circular interop pending.",
    }),
  },
  "node.module_esm": {
    key: "node.module_esm",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Modules: ECMAScript modules",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "ESM resolution/TLA/import attributes pending.",
    }),
  },
  "node.module_api": {
    key: "node.module_api",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Modules: node:module API",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "createRequire, register hooks, builtin APIs.",
    }),
  },
  "node.packages": {
    key: "node.packages",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Modules: Packages",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "exports/imports/type/main/module exact parity pending.",
    }),
  },
  "node.typescript": {
    key: "node.typescript",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Modules: TypeScript",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy:
        "Node LTS TypeScript module handling and tsdown artifact mapping.",
    }),
  },
  "node.net": {
    key: "node.net",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Net",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "TCP/IPC sockets.",
    }),
  },
  "node.os": {
    key: "node.os",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: OS",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        "`os.homedir` lowers to a Go helper and `typeof os.homedir()` lowers as a string result; broader OS info and platform differences pending.",
    }),
  },
  "node.path": {
    key: "node.path",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Path",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        "`path.basename`, `path.delimiter`, `path.dirname`, `path.isAbsolute`, `path.join`, `path.normalize`, `path.parse`, `path.relative`, `path.resolve`, `path.sep`, `path.posix.sep`, and `path.win32.sep` lower for string-path subsets, including direct named ESM imports for supported string-returning helpers; full POSIX/win32 edge cases pending.",
    }),
  },
  "node.perf_hooks": {
    key: "node.perf_hooks",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Performance hooks",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Performance timeline/observer.",
    }),
  },
  "node.permissions": {
    key: "node.permissions",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Permissions",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Permission model decisions required.",
    }),
  },
  "node.process": {
    key: "node.process",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Process",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy:
        "`process.arch`, `process.chdir`, `process.env`, `process.cwd`, `process.cwd()`, `process.execPath`, `process.getgid()`, `process.getuid()`, `process.version`, `process.versions`, `process.versions.node`, `process.platform`, `process.stdin`, `process.stdout`, `process.stderr`, no-IPC `process.channel`, and process function ref truthiness/`typeof` have focused AOT coverage for the Node.js 24.15.0 LTS target; signals, lifecycle, full stdio stream behavior, and broader process object behavior pending.",
    }),
  },
  "node.punycode": {
    key: "node.punycode",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Punycode",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Deprecated module support decision pending.",
    }),
  },
  "node.querystring": {
    key: "node.querystring",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Query strings",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Legacy querystring parser/stringifier.",
    }),
  },
  "node.readline": {
    key: "node.readline",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Readline",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "TTY/input interfaces.",
    }),
  },
  "node.repl": {
    key: "node.repl",
    scope: "node api",
    status: CapabilityStatus.FAIL_CLOSED,
    strategy: "node-lts: REPL",
    backends: backendRules({
      status: CapabilityStatus.FAIL_CLOSED,
      strategy: "Interactive REPL not generated runtime.",
    }),
  },
  "node.report": {
    key: "node.report",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Report",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Diagnostic reports.",
    }),
  },
  "node.sea": {
    key: "node.sea",
    scope: "node api",
    status: CapabilityStatus.FAIL_CLOSED,
    strategy: "node-lts: Single executable applications",
    backends: backendRules({
      status: CapabilityStatus.FAIL_CLOSED,
      strategy: "Node SEA artifact is not Go output contract.",
    }),
  },
  "node.sqlite": {
    key: "node.sqlite",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: SQLite",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Built-in SQLite API.",
    }),
  },
  "node.stream": {
    key: "node.stream",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Stream",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Node streams, pipeline, backpressure.",
    }),
  },
  "node.string_decoder": {
    key: "node.string_decoder",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: String decoder",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Incremental string decoding.",
    }),
  },
  "node.test_runner": {
    key: "node.test_runner",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Test runner",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Needed when compiling tests/tools.",
    }),
  },
  "node.timers": {
    key: "node.timers",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: Timers",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "setTimeout/setInterval/immediate ordering pending.",
    }),
  },
  "node.tls": {
    key: "node.tls",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: TLS/SSL",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "TLS sockets/certs.",
    }),
  },
  "node.trace_events": {
    key: "node.trace_events",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Trace events",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Trace event API.",
    }),
  },
  "node.tty": {
    key: "node.tty",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: TTY",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "TTY streams/window sizing.",
    }),
  },
  "node.dgram": {
    key: "node.dgram",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: UDP/datagram",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "UDP sockets.",
    }),
  },
  "node.url": {
    key: "node.url",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "node-lts: URL",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "URL/URLSearchParams/file URL parity pending.",
    }),
  },
  "node.util": {
    key: "node.util",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Utilities",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "inspect, promisify, callbackify, types.",
    }),
  },
  "node.v8": {
    key: "node.v8",
    scope: "node api",
    status: CapabilityStatus.BLOCKED,
    strategy: "node-lts: V8",
    backends: backendRules({
      status: CapabilityStatus.BLOCKED,
      strategy: "V8 API conflicts with no V8 fallback.",
    }),
  },
  "node.vm": {
    key: "node.vm",
    scope: "node api",
    status: CapabilityStatus.FAIL_CLOSED,
    strategy: "node-lts: VM",
    backends: backendRules({
      status: CapabilityStatus.FAIL_CLOSED,
      strategy:
        "Dynamic evaluation sandbox needs separate source-level strategy.",
    }),
  },
  "node.wasi": {
    key: "node.wasi",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: WASI",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "WASI integration decision pending.",
    }),
  },
  "node.webcrypto": {
    key: "node.webcrypto",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Web Crypto API",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "SubtleCrypto and key APIs.",
    }),
  },
  "node.webstreams": {
    key: "node.webstreams",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Web Streams API",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "WHATWG streams.",
    }),
  },
  "node.worker_threads": {
    key: "node.worker_threads",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Worker threads",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Workers, shared memory, message channels.",
    }),
  },
  "node.zlib": {
    key: "node.zlib",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "node-lts: Zlib",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Compression streams and sync APIs.",
    }),
  },
  "tsdown.esm_bundle": {
    key: "tsdown.esm_bundle",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: ESM bundle",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Static ESM imports/exports and execution order.",
    }),
  },
  "tsdown.cjs_bundle": {
    key: "tsdown.cjs_bundle",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: CJS bundle",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "CommonJS wrapper, require, exports/module.exports.",
    }),
  },
  "tsdown.dual_package": {
    key: "tsdown.dual_package",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Dual package output",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "ESM/CJS entry selection and parity.",
    }),
  },
  "tsdown.dts": {
    key: "tsdown.dts",
    scope: "tsdown artifact",
    status: CapabilityStatus.WIP,
    strategy: "tsdown-artifact: `.d.ts` input",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Symbol/type surface consumed for diagnostics/lowering.",
    }),
  },
  "tsdown.declaration_map": {
    key: "tsdown.declaration_map",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Declaration map input",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Original type location mapping.",
    }),
  },
  "tsdown.sourcemap": {
    key: "tsdown.sourcemap",
    scope: "tsdown artifact",
    status: CapabilityStatus.WIP,
    strategy: "tsdown-artifact: Source map input",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Original JS/TS diagnostic locations.",
    }),
  },
  "tsdown.package_exports": {
    key: "tsdown.package_exports",
    scope: "tsdown artifact",
    status: CapabilityStatus.WIP,
    strategy: "tsdown-artifact: package `exports` metadata",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Conditional exports and subpath exports.",
    }),
  },
  "tsdown.package_imports": {
    key: "tsdown.package_imports",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: package `imports` metadata",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Internal import maps.",
    }),
  },
  "tsdown.package_main_module_type": {
    key: "tsdown.package_main_module_type",
    scope: "tsdown artifact",
    status: CapabilityStatus.WIP,
    strategy: "tsdown-artifact: package `main`/`module`/`type` metadata",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Entry format selection.",
    }),
  },
  "tsdown.node_builtins": {
    key: "tsdown.node_builtins",
    scope: "tsdown artifact",
    status: CapabilityStatus.WIP,
    strategy: "tsdown-artifact: `node:` builtin imports",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Must map to Node LTS ledger capabilities.",
    }),
  },
  "tsdown.json_modules": {
    key: "tsdown.json_modules",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: JSON modules",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Import attributes and JSON cache semantics.",
    }),
  },
  "tsdown.import_attributes": {
    key: "tsdown.import_attributes",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Import attributes",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Attribute validation and resolution.",
    }),
  },
  "tsdown.dynamic_import": {
    key: "tsdown.dynamic_import",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Dynamic import",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Async module loading and errors.",
    }),
  },
  "tsdown.top_level_await": {
    key: "tsdown.top_level_await",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Top-level await",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Module async evaluation order.",
    }),
  },
  "tsdown.code_splitting": {
    key: "tsdown.code_splitting",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Code splitting/chunks",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Multiple chunks and runtime loading graph.",
    }),
  },
  "tsdown.externals": {
    key: "tsdown.externals",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: External dependencies",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "External package boundary and fail-closed policy.",
    }),
  },
  "tsdown.assets": {
    key: "tsdown.assets",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Asset/text imports",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Text/binary asset embedding or diagnostics.",
    }),
  },
  "tsdown.cli_shebang": {
    key: "tsdown.cli_shebang",
    scope: "tsdown artifact",
    status: CapabilityStatus.WIP,
    strategy: "tsdown-artifact: Shebang/CLI entrypoints",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "argv/execPath and executable entry behavior.",
    }),
  },
  "tsdown.platform_target": {
    key: "tsdown.platform_target",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Platform target metadata",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Node/platform target constraints.",
    }),
  },
  "tsdown.package_manager": {
    key: "tsdown.package_manager",
    scope: "tsdown artifact",
    status: CapabilityStatus.TODO,
    strategy: "tsdown-artifact: Package manager metadata",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Lockfile/package manager provenance.",
    }),
  },
  "tsdown.diagnostics_mapping": {
    key: "tsdown.diagnostics_mapping",
    scope: "tsdown artifact",
    status: CapabilityStatus.WIP,
    strategy: "tsdown-artifact: Sourcemap-mapped fail-closed diagnostics",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Every unsupported row must produce deterministic location.",
    }),
  },
  "es.values.primitives": {
    key: "es.values.primitives",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Primitive values",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "undefined/null/boolean/number/string subset exists.",
    }),
  },
  "es.values.bigint": {
    key: "es.values.bigint",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: BigInt",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Arithmetic, comparison, JSON errors.",
    }),
  },
  "es.values.symbol": {
    key: "es.values.symbol",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Symbol",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Registry, property keys, descriptions pending.",
    }),
  },
  "es.values.object_identity": {
    key: "es.values.object_identity",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Object identity",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Reference equality and mutation semantics.",
    }),
  },
  "es.coercion": {
    key: "es.coercion",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: Coercion and equality",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "ToPrimitive, ==, ===, relational comparisons.",
    }),
  },
  "es.scope.lexical": {
    key: "es.scope.lexical",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Lexical scope",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "let/const/function scope and closures.",
    }),
  },
  "es.scope.hoist_tdz": {
    key: "es.scope.hoist_tdz",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: Hoisting and TDZ",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "var/function/class hoist and TDZ errors.",
    }),
  },
  "es.functions.calls": {
    key: "es.functions.calls",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Function calls",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "args, returns, closures.",
    }),
  },
  "es.functions.this_bind": {
    key: "es.functions.this_bind",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: `this`, call/apply/bind",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Strict/non-strict this behavior.",
    }),
  },
  "es.functions.construct": {
    key: "es.functions.construct",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Constructors/new",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "new, prototype, return override pending.",
    }),
  },
  "es.classes": {
    key: "es.classes",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Classes",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Private members subset; super/static blocks pending.",
    }),
  },
  "es.objects.properties": {
    key: "es.objects.properties",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Object properties",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Descriptors/getters/setters pending.",
    }),
  },
  "es.objects.prototype": {
    key: "es.objects.prototype",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: Prototype chain",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Lookup, mutation, instanceof.",
    }),
  },
  "es.objects.destructuring": {
    key: "es.objects.destructuring",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Destructuring",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Defaults/rest/nested patterns pending.",
    }),
  },
  "es.objects.spread_rest": {
    key: "es.objects.spread_rest",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Spread/rest",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Array/object/call spread edge cases pending.",
    }),
  },
  "es.arrays": {
    key: "es.arrays",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Array semantics",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Holes, length, iteration, methods pending.",
    }),
  },
  "es.typed_arrays": {
    key: "es.typed_arrays",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: ArrayBuffer/DataView/TypedArray",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Binary view semantics.",
    }),
  },
  "es.control.block_if_switch": {
    key: "es.control.block_if_switch",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Blocks/if/switch",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Switch completion edge cases pending.",
    }),
  },
  "es.control.loops_labels": {
    key: "es.control.loops_labels",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Loops and labels",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Labeled break/continue subset exists.",
    }),
  },
  "es.control.try_finally": {
    key: "es.control.try_finally",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: try/catch/finally",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "finally completion override pending.",
    }),
  },
  "es.iteration": {
    key: "es.iteration",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Iterators/for-of",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Iterator closing/errors pending.",
    }),
  },
  "es.generators": {
    key: "es.generators",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: Generators",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "yield/return/throw.",
    }),
  },
  "es.async.promises": {
    key: "es.async.promises",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Promise semantics",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Resolution/rejection/microtasks pending.",
    }),
  },
  "es.async.async_await": {
    key: "es.async.async_await",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: async/await",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Ordering/error propagation pending.",
    }),
  },
  "es.async.async_iteration": {
    key: "es.async.async_iteration",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: Async iteration",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "for-await and async iterators.",
    }),
  },
  "es.modules": {
    key: "es.modules",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: ECMAScript modules",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Live bindings/TLA/cycles pending.",
    }),
  },
  "es.regexp": {
    key: "es.regexp",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: RegExp",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Flags, sticky/unicode/groups pending.",
    }),
  },
  "es.date": {
    key: "es.date",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Date",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Parsing/time zones/full method matrix pending.",
    }),
  },
  "es.json": {
    key: "es.json",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: JSON",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Reviver/replacer/toJSON edge cases pending.",
    }),
  },
  "es.error": {
    key: "es.error",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Error objects",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Stack/cause/codes/errors pending.",
    }),
  },
  "es.map_set": {
    key: "es.map_set",
    scope: "language",
    status: CapabilityStatus.WIP,
    strategy: "ecmascript: Map/Set/WeakMap/WeakSet",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "Weak collections and iteration edge cases pending.",
    }),
  },
  "es.intl": {
    key: "es.intl",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: Intl",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "ICU-backed behavior.",
    }),
  },
  "es.proxy_reflect": {
    key: "es.proxy_reflect",
    scope: "language",
    status: CapabilityStatus.TODO,
    strategy: "ecmascript: Proxy/Reflect",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "Trap invariants and reflection.",
    }),
  },
  "es.eval_dynamic": {
    key: "es.eval_dynamic",
    scope: "language",
    status: CapabilityStatus.FAIL_CLOSED,
    strategy: "ecmascript: eval/Function dynamic code",
    backends: backendRules({
      status: CapabilityStatus.FAIL_CLOSED,
      strategy: "No JS engine fallback; source-level strategy required.",
    }),
  },
};

if (Object.keys(CAPABILITY_MATRIX).join("\n") !== CAPABILITY_KEYS.join("\n")) {
  throw new Error(
    "CAPABILITY_MATRIX keys are out of sync with CAPABILITY_KEYS",
  );
}

export { CAPABILITY_BACKENDS, CAPABILITY_KEYS, CapabilityStatus };
