# TS -> Go Full Parity Plan

## Summary

The goal is not "11 commits to make a few test cases pass."

The goal is a compiler/runtime where TypeScript and Node.js code compiles to Go
without framework dependency, and generated Go behavior matches Node behavior.

The 10 real Node test cases are the scorecard and release gate. Implementation
work is grouped by TypeScript/Node semantics, not by individual test case.

Commits should not be split too finely. Commit by large functional axis.

## 100-point definition

There are two scoring layers now:

1. **Current Go parity phase**: the selected 10 real Node corpus entries pass.
2. **Project end state**: Node.js 26 public runtime/language/package semantics
   are either implemented with parity or explicitly fail closed with
   deterministic diagnostics. Stable Node.js 26 APIs are support targets, not
   optional backlog.

All 10 real Node app/library corpus entries must pass:

- Original Node execution is green.
- Generated Go project passes `go build`.
- Generated Go binary execution is green.
- Node vs Go parity diff is zero.

Parity comparison covers:

- Library result JSON.
- CLI/app exit code, stdout, stderr.
- Environment variables, argv, cwd.
- Filesystem side effects.
- Async completion order.
- Thrown error shape where the test case observes it.

Implementation claims required for 100 points:

- No framework-specific branching.
- No Node/V8/embed fallback.
- Unsupported input fails closed with deterministic diagnostics.
- Capabilities required by the corpus are `DONE`.
- Corpus gate runs with `allowWip=false`.

Additional requirements for the Node.js 26 target:

- Node.js 26.1.0 official API docs are the coverage baseline:
  <https://nodejs.org/api/documentation.html>
- Every documented area has a row in the capability ledger.
- Every stable Node.js 26 area must be `DONE` for Go backend or have a tracked
  blocking issue and deterministic fail-closed diagnostic.
- Experimental/deprecated/native/embedder areas still appear in the ledger; the
  project must choose `DONE`, `TODO`, `BLOCKED`, or `FAIL_CLOSED`, never omit
  them silently.
- Generated Go must not use Node, V8, Node-API, N-API, native addon loading, or
  embedder fallback.

## Existing documentation audit

Current docs are useful but not enough for the Node.js 26 end state.

| Document | Current content | Gap |
|---|---|---|
| `README.md` | States "100% behavioral coverage" only inside the declared semantic envelope; links capability matrix. | Does not claim or track Node.js 26 full API coverage. Still contains old service/route framing. |
| `docs/specs/CAPABILITY_MATRIX.md` | Backend-aware matrix for 10 coarse capabilities (`route.basic`, `module.esm`, `node.fs.basic`, etc.). | Too small for Node.js 26. Missing most core modules, JS language semantics, globals, process/CLI behavior, streams, network, crypto, workers, test runner, diagnostics, and native/embedder decisions. |
| `docs/backlog/NODE_COMPAT_MATRIX.md` | Initial Node matrix with 9 TODO rows. | Backlog only; not synchronized with code or official Node 26 docs. |
| `docs/specs/SEMANTIC_PARITY_CONTRACT.md` | HTTP route parity contract: status/body/headers/method behavior. | Narrow route-era parity. Does not define CLI/library/FS/env/argv/async/error/module side-effect parity for Node corpus. |
| `test-corpus/node-real/manifest.json` | 10 real corpus entries and gate metadata. | Good corpus manifest, but not a full Node 26 coverage ledger. |

Action: create a dedicated Node.js 26 coverage ledger and make
`docs/specs/CAPABILITY_MATRIX.md` a generated/backend summary of that ledger
instead of a small hand-maintained table.

## Node.js 26 coverage ledger baseline

Status legend:

- `DONE`: implemented and covered by Node/Go differential tests.
- `WIP`: partially implemented; accepted only in explicit WIP gates.
- `TODO`: required for Node.js 26 target, not yet implemented.
- `FAIL_CLOSED`: intentionally unsupported for now, with deterministic
  diagnostic and no silent codegen.
- `BLOCKED`: incompatible with no-Node/no-V8/no-native-fallback policy unless
  re-scoped as source-level rewrite or separate runtime feature.

Current score against full Node.js 26 coverage is approximately **70/100**:
the 10-corpus Go parity phase is green, but the full Node.js 26 API/semantics
ledger and backend plugin split are not complete.

| Area | Node.js 26 required surface | Current repo evidence | Current status | Gap to 100 |
|---|---|---|---|---|
| JS value model | `undefined`, `null`, booleans, numbers, strings, bigint, symbol, object identity, arrays, functions, classes | Runtime code in `crates/engine-core/src/emit_go.rs`; focused tests in `crates/engine-core/src/lib.rs` | WIP | Complete BigInt, Symbol registry, property descriptors, prototypes, getters/setters, typed arrays, equality/coercion matrix. |
| JS control flow | block, if, switch, loops, labels, break/continue, return, throw, try/catch/finally | Labeled control flow recently added in Rust IR/codegen tests | WIP | Exhaustive completion semantics, finally override rules, iterator closing, generator/async-generator semantics. |
| JS functions/binding | lexical scope, closures, `this`, call/apply/bind, constructors, classes, private fields, destructuring, rest/spread | Corpus vectors and engine tests cover subset | WIP | Full hoist/TDZ, `super`, static blocks, decorators if emitted by TS, advanced destructuring defaults. |
| JS standard built-ins | `Array`, `Object`, `Map`, `Set`, `Date`, `RegExp`, `JSON`, `Error`, `Promise`, `Intl`, `Temporal` | Corpus covers subset; Date/RegExp/JSON/Error have focused runtime work | WIP | Full ECMAScript built-in matrix; Node 26 has Temporal available, needs explicit target decision/tests. |
| Async/event loop | Promise jobs, `async`/`await`, microtasks, timers, `nextTick`, immediates | Corpus async vectors pass for current subset | WIP | Precise Node ordering: `process.nextTick`, microtask queue, timers, immediates, unhandled rejection, abort signals. |
| Module system | CJS, ESM, dual packages, `exports`, `imports`, `main`, `type`, `node:` specifiers, JSON modules, TypeScript module docs | Analyzer/module graph tests; corpus uses package graph | WIP | Full Node 26 package resolution, loader hooks policy, CJS/ESM interop edge cases, circular dependency parity. |
| TypeScript input | Node 26 TypeScript module handling plus tsdown bundle/source map/`.d.ts` compiler input | README and CLI path describe tsdown input | WIP | Exact TS syntax/lowering support matrix; source map diagnostics for every fail-closed path. |
| `process`/CLI/env | `argv`, `execPath`, `env`, `cwd`, `exit`, stdio, signals, warnings, versions, resource usage | Corpus and runtime tests cover argv/env/cwd/exit/stdout/stderr subset | WIP | Full `process` object, signals, warnings, exit lifecycle, stdin, permissions, report integration. |
| File system | `node:fs`, `fs/promises`, sync/callback/promise APIs, watchers, streams, stats, permissions | `fs-extra` corpus subset green | WIP | Full fs API, watchers, symlink/hardlink/stat modes, platform differences, abortable operations. |
| Path/URL/querystring | `node:path`, `node:url`, `node:querystring`, URL globals | `path`/`url` marked WIP in matrix; corpus subset green | WIP | POSIX/win32 path split, file URL conversion, URLSearchParams edge cases, legacy querystring. |
| Buffer/text/binary | `Buffer`, `Blob`, `TextEncoder`, `TextDecoder`, `StringDecoder`, base64/hex/utf8 | Matrix has `node.buffer.basic` TODO; uuid/crypto-ish corpus uses byte-like behavior | TODO | Full Buffer API and encoding compatibility. |
| Crypto/WebCrypto | `node:crypto`, `crypto.webcrypto`, hashes, HMAC, random, UUID, keys, subtle crypto | `uuid` corpus subset green | WIP | Full crypto API or fail-closed unsupported algorithms; deterministic errors; no OpenSSL/Node fallback. |
| Streams/Web Streams | `node:stream`, iterable streams, web streams, pipeline, backpressure | execa/fs-extra touch small process/io subset | TODO | Readable/Writable/Transform, backpressure, async iteration, web stream interop. |
| Child process | `spawn`, `spawnSync`, `exec`, `execFile`, IPC, stdio modes, signals | `execa` corpus subset; `spawnSync` utf8 fixed | WIP | Async subprocess lifecycle, stdio piping, IPC, shell behavior, signal/kill semantics. |
| Network | `net`, `http`, `https`, `http2`, `tls`, `dgram`, `dns` | Old route scaffold docs/tests; not Node runtime parity | TODO | Full client/server APIs, sockets, TLS, DNS, HTTP parser behavior, streaming bodies. |
| Events/diagnostics | `EventEmitter`, `AsyncLocalStorage`, `async_hooks`, diagnostics channel, trace events | Matrix has event loop TODO; route tests only | TODO | EventEmitter semantics, async context propagation, diagnostics subscriptions. |
| OS/perf/util | `os`, `perf_hooks`, `util`, `assert`, `console`, `test` runner | Some `console` output parity in gates | TODO | Full utility modules, inspection formatting, assertion errors, test runner if compiling tests. |
| Workers/concurrency | `worker_threads`, `cluster`, message channels, Atomics/shared memory | Not covered | TODO | Worker runtime model or deterministic fail-closed until implemented. |
| VM/V8/inspector/debugger | `vm`, `v8`, inspector, debugger, snapshots/code cache | Not covered | FAIL_CLOSED/BLOCKED | Cannot embed Node/V8. Need source-level interpretation/rewrite or deterministic diagnostic. |
| Native/addons/embedder | C++ addons, Node-API, C++ embedder API, FFI | Not covered | BLOCKED | Conflicts with no native/Node fallback. Must fail closed unless separately reimplemented at source/API boundary. |
| Deprecated/legacy | domain, punycode, deprecated APIs | Not covered | TODO/FAIL_CLOSED | Still ledger rows required; choose support or deterministic diagnostic. |
| Packaging/runtime artifacts | SEA, permissions, report, REPL, WASI, SQLite, zlib | Not covered | TODO/FAIL_CLOSED | Decide per API. Stable APIs need implementation target; experimental can fail closed with diagnostics. |

## 100-point implementation plan from current state

### Phase 1: Backend interface and registry

- Add a Rust `Backend` trait for backend-neutral emission.
- Move Go emission behind a `GoBackend` implementation.
- Add backend registry with `go` as the only enabled backend.
- Unsupported backend names produce deterministic diagnostics.
- Strengthen guards so backend-neutral IR/contract cannot contain Go terms.

Commit: `engine: introduce backend interface and go backend`

### Phase 2: Go emitter isolation

- Split `crates/engine-core/src/emit_go.rs` into backend-specific modules:
  - `backend/mod.rs`
  - `backend/go/mod.rs`
  - `backend/go/emitter.rs`
  - `backend/go/runtime_emit.rs`
- Keep Go emitter responsible for rendering Go only.
- Remove JS semantic policy decisions from Go emitter.

Commit: `engine: isolate go emitter from semantic policy`

### Phase 3: Runtime contract extraction

- Promote `runtime_contract.rs` into backend-neutral semantic contract SSoT.
- Define JS operations in contract form:
  - value operations
  - property access/mutation
  - call/construct/this binding
  - completion records
  - module cache/resolution hooks
  - async queue contract
  - Node runtime API contracts
- Make Go runtime generator consume the contract instead of owning policy.

Commit: `engine: extract backend neutral runtime contract`

### Phase 4: Node.js 26 coverage ledger

- Add `docs/specs/NODE26_COVERAGE_LEDGER.md`.
- Include every official Node.js 26 documentation area.
- Track per area:
  - contract status
  - Go backend status
  - test evidence
  - fail-closed diagnostic code
  - known semantic gaps
- Generate/sync `docs/specs/CAPABILITY_MATRIX.md` from this ledger or add a
  guard that proves both stay consistent.

Commit: `docs: add node 26 coverage ledger`

### Phase 5: Hardcode prevention and holdout tests

- Extend `pnpm run gate:node-corpus-general-compiler`.
- Scan compiler/codegen/runtime for corpus-name/package-specific branches.
- Add holdout semantics tests with same syntax patterns but different packages,
  names, data, and control-flow shapes.
- Fail if corpus vectors pass only because corpus-specific strings or package
  paths are recognized.

Commit: `test: enforce non corpus specific parity`

### Phase 6: General JS semantics expansion

- Add differential tests by semantic axis, not by corpus:
  - scope/closure/hoist/TDZ
  - object/prototype/descriptors
  - array/property ordering
  - equality/coercion
  - `this`/class/private/super
  - destructuring/spread/rest
  - try/finally completion
  - Promise/microtask/timer ordering
  - built-ins matrix
- Require Node and generated Go to run the same vectors.

Commit: `engine: expand general js semantic parity`

### Phase 7: Node.js 26 runtime API expansion

- Implement stable Node.js 26 API groups in functional batches:
  - process/CLI/env/stdio/signals
  - fs/path/url/querystring
  - buffer/text/crypto
  - events/async context/timers
  - streams/child_process
  - net/http/https/tls/dns/dgram
  - os/perf/util/assert/console/test
  - zlib/sqlite/permissions/report
- For VM/V8/native/embedder surfaces, add deterministic fail-closed
  diagnostics unless a source-level reimplementation strategy is approved.

Commit sequence: one commit per API family, not per test case.

### Phase 8: Final Node.js 26 gate

Final required gate becomes:

```bash
pnpm run lint
pnpm run format:check
pnpm run build
pnpm run test
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
pnpm run gate:semantics-parity
pnpm run gate:compliance
pnpm run test:node-corpus:vitest
pnpm run gate:node-corpus-vector-parity
pnpm run gate:node-corpus-parity
pnpm run gate:node-corpus-general-compiler
pnpm run gate:node26-coverage-ledger
./scripts/smoke-m1.sh
```

## Implementation strategy

### Real parity harness first

- Remove hardcoded expected parity responses.
- Compare actual Node execution with actual generated Go execution.
- Record package source, version, license, probe command, and comparator in the
  corpus manifest.
- Lock all 10 test cases as a red gate before making compiler/runtime changes.

### Compiler front-end

- Replace the Rust analyzer string parser with JS/TS AST-based lowering.
- Use tsdown bundle, sourcemap, `.d.ts`, and package metadata as compiler input.
- Extend IR from HTTP route-centered metadata to executable JS program semantics.
- Do not put framework names into IR.

### JS semantics lowering

- Value model: `undefined`, `null`, boolean, number, string, bigint, symbol,
  object, array, function, class.
- Control flow: block, if, switch, loops, break/continue, return, throw,
  try/catch/finally.
- Calls and binding: lexical scope, closure, `this`, prototype/class,
  destructuring, spread/rest.
- Standard objects: Array, Object, Map, Set, Date, RegExp, JSON, Error.
- Async: Promise, async/await, microtask ordering, timer subset.

### Module/package system

- Support ESM, CJS, and dual packages.
- Support `import`, `export`, `require`, package `exports`, package `main`, and
  relative resolution.
- Preserve module cache and circular dependency behavior.
- Bundle the dependency graph into the generated Go project.

### Node runtime compatibility

- `process`: argv, env, cwd, exit, stdout, stderr.
- `fs`: sync, callback, and promise APIs required by the corpus.
- `path`, `url`, `buffer`, `crypto`.
- `child_process` and stream subset for app-level test cases.
- Put shared semantics in a Go helper runtime package named `tsgodownrt`.

## Commit policy

- There is no target commit count.
- Commit by large functional axis:
  - parity harness/corpus gate
  - executable IR/front-end
  - JS core semantics
  - module system
  - Node runtime APIs
  - async/event loop
  - generated Go runtime/codegen
  - final hardening/docs
- Do not make one commit per test case.
- Keep small refactors and fixture churn inside the relevant functional commit.
- Every commit must leave the repo buildable and testable.

## Test plan

Add `pnpm run gate:node-corpus-parity`.

The 10 corpus entries are:

- `semver`
- `minimatch`
- `qs`
- `dotenv`
- `yargs-parser`
- `js-yaml`
- `lru-cache`
- `uuid`
- `fs-extra`
- `execa`

Each test case runs a Node probe and a Go probe with the same input.

## Large package/application corpus

The 10 existing corpus entries are useful but too small. They mostly prove
library/CLI utility semantics. After the Node.js 26 parity ledger is in place,
add a second corpus tier with large framework, build-tool, compiler, server,
GraphQL, and ORM packages.

Important package classification:

- Not every target is authored in TypeScript. `express`, `koa`, `webpack`, and
  other older packages are often JavaScript-authored and TypeScript-consumable
  through declarations.
- This is still in scope. `tsgodown` must compile TypeScript/JavaScript Node
  source after `tsdown` prepares the bundle, sourcemap, `.d.ts`, and package
  metadata.
- The corpus manifest must record source language, declaration source, module
  format, package manager metadata, license, version, and whether any native or
  external binary dependency exists.

Large corpus rules:

- Add 20 more corpus entries under `test-corpus/node-large/`.
- Each entry must have exactly 100 Vitest tests at first landing.
- The same 100 test vectors must run through:
  - original Node.js 26 execution
  - `tsgodown` compile
  - generated Go `go build`
  - generated Go binary execution
  - Node/Go observable parity comparator
- Tests must exercise real package behavior, not toy wrappers.
- Network tests use local loopback only.
- Filesystem tests use temp directories only.
- Database/cache/message-queue tests must avoid external services unless the
  dependency is vendored and deterministic; otherwise the unsupported surface
  must fail closed with diagnostics.
- Package-specific hacks are forbidden. The same syntax/API capability must work
  for holdout apps using different names and data.

Initial large corpus candidates verified from npm metadata on 2026-05-15:

| Corpus id | npm package/version | Class | 100-vector probe focus | Primary parity dimensions |
|---|---|---|---|---|
| `express-app` | `express@5.2.1` | HTTP framework, JS-authored TS-consumable | routing, middleware order, params/query/body, errors, async handlers | status, headers, body, stderr, async order |
| `nestjs-app` | `@nestjs/core@11.1.21`, `@nestjs/common@11.1.21` | TS framework | controllers, providers, DI graph, pipes, filters, guards, module lifecycle | HTTP output, DI behavior, thrown error shape, async order |
| `fastify-app` | `fastify@5.8.5` | HTTP framework | plugin registration, schemas, hooks, encapsulation, route params | status/body/header parity, plugin order |
| `koa-app` | `koa@3.2.0` | HTTP middleware framework | onion middleware order, context mutation, thrown errors, async compose | body/status, side effects, async order |
| `hapi-app` | `@hapi/hapi@21.4.9` | HTTP framework | route config, validation hooks, lifecycle extensions, response toolkit | status/body/header parity, lifecycle order |
| `vite-build` | `vite@8.0.13` | dev/build tool | config loading, plugin hooks, resolve/transform/build manifest | emitted files, stdout/stderr, exit code, plugin order |
| `rollup-build` | `rollup@4.60.4` | bundler | plugin pipeline, tree-shaking, chunk graph, sourcemaps | output chunks, warnings, sourcemaps, exit code |
| `webpack-build` | `webpack@5.106.2` | bundler | loaders/plugins, resolver, asset graph, code splitting | emitted assets, stats JSON, warnings/errors |
| `next-app` | `next@16.2.6` | full-stack framework | config load, file routes, server rendering probes, build output metadata | stdout/stderr, files, HTTP render output |
| `nuxt-app` | `nuxt@4.4.5` | full-stack framework | config/modules, Nitro server output, route rendering probes | output files, HTTP render output, hooks |
| `astro-app` | `astro@6.3.3` | site framework | content collections, integrations, build output, server render probes | output files, HTML, diagnostics |
| `remix-app` | `remix@2.17.4` | web framework/tooling | route modules, loaders/actions, build output, server adapter probes | HTTP/data responses, output files, errors |
| `eslint-engine` | `eslint@10.3.0` | linter engine | config resolution, parser services boundary, rules, formatters | result JSON, stdout/stderr, exit code |
| `prettier-engine` | `prettier@3.8.3` | formatter engine | parser selection, plugin hooks, doc printer, config resolution | formatted text, diagnostics, async plugin order |
| `babel-core` | `@babel/core@7.29.0` | compiler transform engine | parser/traverse/generator, plugins, presets, sourcemaps | generated code, sourcemaps, diagnostics |
| `typescript-compiler` | `typescript@6.0.3` | compiler API | program creation, type checker probes, emit, diagnostics | emitted JS/d.ts, diagnostic shape, exit code |
| `graphql-engine` | `graphql@16.14.0` | query language/runtime | schema build, validation, execution, subscriptions subset | result JSON, error shape, async order |
| `apollo-server-app` | `@apollo/server@5.5.1` | GraphQL server | schema/resolvers/plugins, context, errors, local HTTP execution | JSON result, HTTP status, plugin order |
| `socketio-app` | `socket.io@4.8.3` | realtime server | local loopback connection, rooms, ack callbacks, middleware | event order, payload JSON, close/error shape |
| `typeorm-app` | `typeorm@0.3.29` | ORM | metadata, entity manager, query builder, transaction API with deterministic local driver | SQL/log output, result JSON, error shape |

Large corpus gates:

```bash
pnpm run test:node-large:vitest
pnpm run gate:node-large-vector-parity
pnpm run gate:node-large-parity
pnpm run gate:node-large-general-compiler
```

Large corpus acceptance:

- 20 entries x 100 vectors = 2000 Vitest tests pass on Node.js 26.
- The same 2000 vectors pass through generated Go.
- All generated Go projects pass `go build`.
- Node/Go parity diff is zero.
- Capability coverage report maps every failure to a Node.js 26 ledger row.
- Corpus-specific code branches are a release blocker.

The diff report is grouped by capability:

- language
- module
- node-api
- async
- filesystem
- cli/process

Final required gate:

```bash
pnpm run lint
pnpm run format:check
pnpm run build
pnpm run test
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
pnpm run gate:semantics-parity
pnpm run gate:compliance
./scripts/smoke-m1.sh
pnpm run gate:node-corpus-parity
```

## Corpus logic path

The detailed 10-case corpus logic is recorded in:

- `test-corpus/node-real/README.md`

That document is the initial source of truth for package versions, probe intent,
observed behavior, required capabilities, and comparators. The implementation
gate should later promote it into a machine-readable manifest without changing
the intent.

## Assumptions

- This phase's 100-point score means the selected 10 real Node app/library
  entries fully compile to Go and pass parity.
- The long-term goal remains arbitrary Node code. Proof expands through corpus
  growth.
- Native addon, worker thread, network server, and browser API support are added
  as separate functional axes when the corpus requires them.
