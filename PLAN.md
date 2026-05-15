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
2. **Project end state**: Node.js 24.15.0 LTS public runtime/language/package semantics
   are either implemented with parity or explicitly fail closed with
   deterministic diagnostics. Stable Node.js 24.15.0 LTS APIs are support targets, not
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

Additional requirements for the latest active Node.js LTS target:

- Node.js 24.15.0 LTS official API docs are the coverage baseline:
  <https://nodejs.org/docs/latest-v24.x/api/>
- Every documented area has a row in the capability ledger.
- Every stable Node.js 24.15.0 LTS area must be `DONE` for Go backend or have a tracked
  blocking issue and deterministic fail-closed diagnostic.
- Experimental/deprecated/native/embedder areas still appear in the ledger; the
  project must choose `DONE`, `TODO`, `BLOCKED`, or `FAIL_CLOSED`, never omit
  them silently.
- Generated Go must not use Node, V8, Node-API, N-API, native addon loading, or
  embedder fallback.

## Final Completeness Contract

This plan is complete only when `tsgodown` can accept every supported `tsdown`
bundle artifact shape and either:

- compile it into a standalone Go project that builds and matches Node.js
  24.15.0 LTS observable behavior, or
- reject it with deterministic diagnostics before codegen.

"All tsdown-bundleable code" does not mean silently supporting impossible
surfaces with partial behavior. It means every input shape and runtime feature
has an explicit contract row: `DONE` with parity evidence, or `FAIL_CLOSED` /
`BLOCKED` with diagnostics and no generated wrong Go.

### Required contracts before completion

| Contract | Completion requirement | Gate |
|---|---|---|
| tsdown artifact contract | Every supported bundle shape is documented: ESM/CJS output, sourcemap, `.d.ts`, package metadata, externals, assets, dynamic import, import attributes, top-level await, code splitting, platform target, and package manager metadata. Unsupported artifact shapes fail closed. | `pnpm run gate:tsdown-artifact-contract` |
| ECMAScript semantics ledger | Every ECMAScript feature reachable from Node.js 24.15.0 LTS is tracked: values, coercion, scope, functions, classes, modules, objects, arrays, typed arrays, promises, async iteration, generators, regexp, date, JSON, errors, intl, and built-ins. | `pnpm run gate:ecmascript-ledger` |
| Node.js LTS API ledger | Every official Node.js 24.15.0 LTS documentation area has a row with contract status, Go backend status, test evidence, diagnostic code, and known gaps. Stable APIs are `DONE` or explicitly `FAIL_CLOSED`/`BLOCKED`. | `pnpm run gate:node-lts-coverage-ledger` |
| Backend provider interface | Rust exposes a backend-neutral provider interface. Go backend is registered only through that interface using adapter/provider pattern. No caller reaches Go emitter directly. | `pnpm run gate:backend-provider-interface` |
| Runtime contract ownership | JS/Node semantic policy lives in backend-neutral IR/runtime contract. Go backend renders/implements contract operations but does not decide JS semantics. | `pnpm run gate:runtime-contract-ownership` |
| No fallback | Generated Go never embeds, shells out to, links against, or requires Node.js, V8, Node-API, N-API, native addons, or corpus-specific helper binaries. | `pnpm run gate:no-node-fallback` |
| No corpus hardcode | Compiler/runtime/codegen has no package-name, corpus-name, or fixture-name special branches. Holdout tests using same syntax/API patterns but different package names and data must pass. | `pnpm run gate:no-corpus-hardcode` |
| Observable parity | Node.js and generated Go match stdout, stderr, exit code, JSON/library result, env, argv, cwd, filesystem side effects, async order, and observed error shape. | `pnpm run gate:full-observable-parity` |
| Corpus proof | Existing 10 small corpus entries and 20 large corpus entries all have 100 Vitest vectors each and all vectors pass through Node.js LTS and generated Go. | `pnpm run gate:all-corpus-parity` |
| Differential/fuzz proof | Spec-axis differential tests and fuzz/holdout suites cover syntax/API combinations beyond the fixed corpus. | `pnpm run gate:differential-fuzz` |

### Backend provider contract

The Rust compiler core must expose backend providers through one interface.
Go is only the first provider.

Required shape:

```text
Executable IR + Runtime Contract + Target Options
  -> BackendProvider
  -> Generated Project
  -> Build Plan
  -> Runtime Probe Contract
```

Provider rules:

- `BackendProvider` owns target identity, capability status, artifact emission,
  runtime package emission, build command metadata, and diagnostics mapping.
- `GoBackendProvider` adapts the backend-neutral contract into Go source and the
  `tsgodownrt` helper package.
- Backend-neutral IR, analyzer, capability ledger, and runtime contract cannot
  import Go backend modules or mention Go-only implementation concepts.
- Adding a future backend must not change analyzer/IR semantics. It should add a
  provider implementation and capability statuses.
- Tests must prove the compiler can call the Go backend only through the
  provider registry.

### tsdown artifact acceptance contract

`tsgodown` completion requires a written and tested input contract for every
artifact shape `tsdown` can produce in supported mode.

Required rows:

- ESM bundle
- CJS bundle
- dual package output
- `.d.ts` and declaration map input
- source map input and original location diagnostics
- package `exports`, `imports`, `main`, `module`, `type`
- `node:` builtins
- JSON modules and import attributes
- dynamic import
- top-level await
- code splitting/chunks
- external dependencies
- asset/text imports
- shebang/CLI entrypoints
- sourcemap-mapped diagnostics for fail-closed cases

Each row must declare `DONE`, `FAIL_CLOSED`, or `BLOCKED`. `TODO` cannot remain
in the final gate.

### Final project done condition

The project is done only when this command group passes with no WIP allowance:

```bash
mise exec -- pnpm run lint
mise exec -- pnpm run format:check
mise exec -- pnpm run build
mise exec -- pnpm run test
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
mise exec -- pnpm run gate:tsdown-artifact-contract
mise exec -- pnpm run gate:ecmascript-ledger
mise exec -- pnpm run gate:node-lts-coverage-ledger
mise exec -- pnpm run gate:backend-provider-interface
mise exec -- pnpm run gate:runtime-contract-ownership
mise exec -- pnpm run gate:no-node-fallback
mise exec -- pnpm run gate:no-corpus-hardcode
mise exec -- pnpm run gate:semantics-parity
mise exec -- pnpm run gate:compliance
mise exec -- pnpm run test:node-corpus:vitest
mise exec -- pnpm run gate:node-corpus-vector-parity
mise exec -- pnpm run gate:node-corpus-parity
mise exec -- pnpm run test:node-large:vitest
mise exec -- pnpm run gate:node-large-vector-parity
mise exec -- pnpm run gate:node-large-parity
mise exec -- pnpm run gate:full-observable-parity
mise exec -- pnpm run gate:all-corpus-parity
mise exec -- pnpm run gate:differential-fuzz
./scripts/smoke-m1.sh
```

At that point, the project can claim: any TypeScript/JavaScript Node.js code
that `tsdown` can bundle inside the supported artifact contract either compiles
to standalone Go with Node.js LTS parity, or fails closed before producing
incorrect output.

## Existing documentation audit

Current docs are useful but not enough for the latest active Node.js LTS end state.

| Document | Current content | Gap |
|---|---|---|
| `README.md` | States "100% behavioral coverage" only inside the declared semantic envelope; links capability matrix. | Does not claim or track Node.js 24.15.0 LTS full API coverage. Still contains old service/route framing. |
| `docs/specs/CAPABILITY_MATRIX.md` | Backend-aware matrix for 10 coarse capabilities (`route.basic`, `module.esm`, `node.fs.basic`, etc.). | Too small for Node.js 24.15.0 LTS. Missing most core modules, JS language semantics, globals, process/CLI behavior, streams, network, crypto, workers, test runner, diagnostics, and native/embedder decisions. |
| `docs/backlog/NODE_COMPAT_MATRIX.md` | Initial Node matrix with 9 TODO rows. | Backlog only; not synchronized with code or official Node.js 24.15.0 LTS docs. |
| `docs/specs/SEMANTIC_PARITY_CONTRACT.md` | HTTP route parity contract: status/body/headers/method behavior. | Narrow route-era parity. Does not define CLI/library/FS/env/argv/async/error/module side-effect parity for Node corpus. |
| `test-corpus/node-real/manifest.json` | 10 real corpus entries and gate metadata. | Good corpus manifest, but not a full Node.js LTS coverage ledger. |

Action: create a dedicated latest active Node.js LTS coverage ledger and make
`docs/specs/CAPABILITY_MATRIX.md` a generated/backend summary of that ledger
instead of a small hand-maintained table.

## Latest Active Node.js LTS Coverage Ledger Baseline

Status legend:

- `DONE`: implemented and covered by Node/Go differential tests.
- `WIP`: partially implemented; accepted only in explicit WIP gates.
- `TODO`: required for latest active Node.js LTS target, not yet implemented.
- `FAIL_CLOSED`: intentionally unsupported for now, with deterministic
  diagnostic and no silent codegen.
- `BLOCKED`: incompatible with no-Node/no-V8/no-native-fallback policy unless
  re-scoped as source-level rewrite or separate runtime feature.

Current score against full latest active Node.js LTS coverage is approximately **70/100**:
the 10-corpus Go parity phase is green, but the full Node.js 24.15.0 LTS API/semantics
ledger and backend plugin split are not complete.

| Area | Node.js 24.15.0 LTS required surface | Current repo evidence | Current status | Gap to 100 |
|---|---|---|---|---|
| JS value model | `undefined`, `null`, booleans, numbers, strings, bigint, symbol, object identity, arrays, functions, classes | Runtime code in `crates/engine-core/src/emit_go.rs`; focused tests in `crates/engine-core/src/lib.rs` | WIP | Complete BigInt, Symbol registry, property descriptors, prototypes, getters/setters, typed arrays, equality/coercion matrix. |
| JS control flow | block, if, switch, loops, labels, break/continue, return, throw, try/catch/finally | Labeled control flow recently added in Rust IR/codegen tests | WIP | Exhaustive completion semantics, finally override rules, iterator closing, generator/async-generator semantics. |
| JS functions/binding | lexical scope, closures, `this`, call/apply/bind, constructors, classes, private fields, destructuring, rest/spread | Corpus vectors and engine tests cover subset | WIP | Full hoist/TDZ, `super`, static blocks, decorators if emitted by TS, advanced destructuring defaults. |
| JS standard built-ins | `Array`, `Object`, `Map`, `Set`, `Date`, `RegExp`, `JSON`, `Error`, `Promise`, `Intl` | Corpus covers subset; Date/RegExp/JSON/Error have focused runtime work | WIP | Full ECMAScript built-in matrix for the Node.js 24.15.0 LTS runtime. |
| Async/event loop | Promise jobs, `async`/`await`, microtasks, timers, `nextTick`, immediates | Corpus async vectors pass for current subset | WIP | Precise Node ordering: `process.nextTick`, microtask queue, timers, immediates, unhandled rejection, abort signals. |
| Module system | CJS, ESM, dual packages, `exports`, `imports`, `main`, `type`, `node:` specifiers, JSON modules, TypeScript module docs | Analyzer/module graph tests; corpus uses package graph | WIP | Full Node.js 24.15.0 LTS package resolution, loader hooks policy, CJS/ESM interop edge cases, circular dependency parity. |
| TypeScript input | Node.js 24.15.0 LTS TypeScript module handling plus tsdown bundle/source map/`.d.ts` compiler input | README and CLI path describe tsdown input | WIP | Exact TS syntax/lowering support matrix; source map diagnostics for every fail-closed path. |
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

## Completion Sequence From Current State

Do this in order. No short/mid/long split. Each step must leave the repo
buildable/testable and commit by functional axis.

1. **Pin and verify Node.js LTS toolchain**
   - Keep `.mise.toml` pinned to Node.js `24.15.0` and pnpm `10.22.0`.
   - Run all JS gates through `mise exec --`.
   - Add or update preflight docs/scripts if any command bypasses mise.
   - Commit: `tooling: pin node lts runtime`

2. **Keep docs and gates on latest-LTS policy**
   - README, PLAN, coverage docs, gate names, and diagnostics must say
     "latest active Node.js LTS" or explicit `24.15.0`.
   - Future Node upgrade policy: update mise pin, regenerate coverage ledger,
     then rerun all corpus parity gates.
   - Commit: `docs: align parity target with node lts`

3. **Create Node.js LTS coverage ledger**
   - Add `docs/specs/NODE_LTS_COVERAGE_LEDGER.md`.
   - Include every official Node.js 24.15.0 LTS documentation/API area.
   - Track: contract status, Go status, test evidence, diagnostic code, known
     semantic gaps, corpus coverage.
   - Add `pnpm run gate:node-lts-coverage-ledger`.
   - Commit: `docs: add node lts coverage ledger`

4. **Create tsdown artifact contract**
   - Add `docs/specs/TSDOWN_ARTIFACT_CONTRACT.md`.
   - Track ESM/CJS, sourcemaps, declarations, exports/imports metadata,
     dynamic import, top-level await, chunks, externals, assets, and CLI
     entrypoints.
   - Add `pnpm run gate:tsdown-artifact-contract`.
   - Commit: `docs: add tsdown artifact contract`

5. **Create ECMAScript semantics ledger**
   - Add `docs/specs/ECMASCRIPT_SEMANTICS_LEDGER.md`.
   - Track language and built-in semantics separately from Node APIs.
   - Add `pnpm run gate:ecmascript-ledger`.
   - Commit: `docs: add ecmascript semantics ledger`

6. **Sync capability matrix to the coverage ledgers**
   - Make `docs/specs/CAPABILITY_MATRIX.md` generated from ledger data or guard
     these files for 1:1 row/status consistency.
   - Expand capability keys beyond current coarse route-era set.
   - Add backend columns for Go/Rust/C++ while only Go is implemented.
   - Commit: `node-compat: sync capabilities with node lts ledger`

7. **Harden no-hardcode and no-fallback gates**
   - Expand `pnpm run gate:node-corpus-general-compiler`.
   - Ban corpus-name/package-name branches in compiler/codegen/runtime.
   - Keep generated Go free of Node/V8/shell-out fallback.
   - Add holdout fixtures with same syntax/API patterns but different names and
     data.
   - Commit: `test: enforce generic compiler parity`

8. **Introduce backend provider interface and registry**
   - Add Rust backend trait/registry.
   - Register Go as only enabled backend.
   - Unsupported backend names produce deterministic diagnostics.
   - Backend-neutral IR/contract cannot contain Go-specific concepts.
   - Add `pnpm run gate:backend-provider-interface`.
   - Commit: `engine: introduce backend provider registry`

9. **Move Go emission behind Go provider**
   - Split `emit_go.rs` into backend-specific modules.
   - Keep Go emitter focused on rendering target code.
   - Remove semantic policy decisions from Go emitter call sites.
   - Prove every Go generation path reaches emitter through provider registry.
   - Commit: `engine: isolate go backend provider`

10. **Extract runtime contract**
   - Promote JS operation rules into backend-neutral runtime contract.
   - Cover value operations, property access, call/construct/this, completion
     records, module cache, async queue, and Node API contracts.
   - Make generated Go runtime consume contract definitions.
   - Add `pnpm run gate:runtime-contract-ownership`.
   - Commit: `engine: extract runtime contract`

11. **Make existing 10-corpus gates run on Node.js 24.15.0 LTS**
   - Regenerate vectors if Node LTS behavior differs.
   - Run Node original, Go build, Go run, vector parity.
   - Keep 100 vectors per corpus.
   - Commit only if fixtures/vectors/gates change.

12. **Add large corpus harness skeleton**
    - Create `test-corpus/node-large/manifest.json`.
    - Add shared vector runner, generated-Go runner, and parity report format.
    - Add scripts:
      - `pnpm run test:node-large:vitest`
      - `pnpm run gate:node-large-vector-parity`
      - `pnpm run gate:node-large-parity`
      - `pnpm run gate:node-large-general-compiler`
    - Commit: `test: add large node corpus harness`

13. **Vendor large corpus entries**
    - Add the 20 framework/tooling/application targets listed below.
    - Record package version, license, source language, module format, native or
      external dependency status, probe command, and comparator.
    - Do not implement package-specific compiler branches.
    - Commit: `test: vendor large node corpus`

14. **Add 100 Vitest vectors per large corpus**
    - Total: 20 entries x 100 vectors = 2000 tests.
    - Tests must hit real behavior: routing, plugin hooks, config resolution,
      module loading, build output, diagnostics, FS effects, async order, error
      shape.
    - Commit in large functional groups, not one package per commit unless the
      diff becomes unreviewable.
    - Commit: `test: add large corpus vectors`

15. **Add differential/fuzz proof**
    - Add syntax/API fuzzers for ECMAScript and Node API combinations inside
      supported artifact contract.
    - Add holdout packages/apps that are not in fixed corpus.
    - Add `pnpm run gate:differential-fuzz`.
    - Commit: `test: add differential fuzz parity`

16. **Implement general JS semantics by failing gate order**
    - Use failing 10-corpus, holdout, and large-corpus reports to choose next
      semantic axis.
    - Implement language semantics generally: scope/hoist/TDZ, prototype,
      descriptors, equality/coercion, class/super/private, destructuring,
      spread/rest, finally completion, Promise/microtask/timers, built-ins.
    - Add semantic-axis differential tests before or with implementation.
    - Commit sequence: one commit per semantic axis.

17. **Implement Node.js LTS APIs by failing gate order**
    - Implement API families generally:
      - process/CLI/env/stdio/signals
      - fs/path/url/querystring
      - buffer/text/crypto
      - events/async context/timers
      - streams/child_process
      - net/http/https/tls/dns/dgram
      - os/perf/util/assert/console/test
      - zlib/sqlite/permissions/report
    - VM/V8/native/embedder surfaces fail closed unless source-level
      reimplementation is explicitly designed.
    - Commit sequence: one commit per API family.

18. **Remove route-era product assumptions**
    - Keep route fixtures only as compatibility samples.
    - README/docs/CLI output must describe user workflow around tsdown-bundleable
      Node packages, not Fastify/Hono route extraction.
    - Compiler core must not branch on framework names.
    - Commit: `docs: remove route-era product framing`

19. **Run final release gate**

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
pnpm run gate:tsdown-artifact-contract
pnpm run gate:ecmascript-ledger
pnpm run gate:backend-provider-interface
pnpm run gate:runtime-contract-ownership
pnpm run gate:no-node-fallback
pnpm run gate:no-corpus-hardcode
pnpm run test:node-corpus:vitest
pnpm run gate:node-corpus-vector-parity
pnpm run gate:node-corpus-parity
pnpm run gate:node-corpus-general-compiler
pnpm run gate:node-lts-coverage-ledger
pnpm run test:node-large:vitest
pnpm run gate:node-large-vector-parity
pnpm run gate:node-large-parity
pnpm run gate:full-observable-parity
pnpm run gate:all-corpus-parity
pnpm run gate:differential-fuzz
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
library/CLI utility semantics. After the Node.js LTS parity ledger is in place,
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
  - original Node.js 24.15.0 LTS execution
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

- 20 entries x 100 vectors = 2000 Vitest tests pass on Node.js 24.15.0 LTS.
- The same 2000 vectors pass through generated Go.
- All generated Go projects pass `go build`.
- Node/Go parity diff is zero.
- Capability coverage report maps every failure to a Node.js LTS ledger row.
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
