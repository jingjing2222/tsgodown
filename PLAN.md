# TS -> Go Full Parity Plan

## Summary

The goal is not "11 commits to make a few test cases pass."

The goal is a compiler/runtime where TypeScript and Node.js code compiles to Go
without framework dependency, and generated Go behavior matches Node behavior.

The 10 real Node test cases are the scorecard and release gate. Implementation
work is grouped by TypeScript/Node semantics, not by individual test case.

Commits should not be split too finely. Commit by large functional axis.

## 100-point definition

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
