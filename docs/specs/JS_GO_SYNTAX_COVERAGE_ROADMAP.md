# JS -> Go Syntax Coverage Roadmap (Issue #117)

## Goal
Build `tsgodown` into a production-grade JS/TS artifact -> Go compiler that covers language-level syntax/semantics broadly, with only explicit host-bound exceptions.

## Scope rule
- In scope: language-level JavaScript/TypeScript semantics (expressions, statements, modules, types, async/control-flow, data model)
- Out of scope by default: host-bound runtime APIs that require browser/process embedding not representable as pure compiler output (example: direct browser DOM control)
- Policy: unsupported items are temporary backlog unless they are explicit host-bound exceptions

## Milestones and checklist

### M0 — Source of truth and policy lock
- [x] Align issue/docs/gates to one roadmap source
- [x] Reconcile stale completion claims and add drift log
- [ ] Define canonical ownership for roadmap updates and approvals
- [ ] Add script that verifies issue checklist count/sync against docs mirror
- [ ] Add mandatory PR template section for syntax-capability evidence
- [ ] Define exception process for host-bound non-target syntax/API
- [x] Freeze milestone naming and progression policy for all reports
- [ ] Publish supported/unsupported/planned transition criteria

### M1 — Syntax capability taxonomy (JS vs Go mapping)
- [ ] Define stable syntax capability IDs by ECMAScript category
- [ ] Map each capability to parser/analyzer/IR/emitter/runtime stages
- [ ] Add per-capability owner and acceptance evidence format
- [ ] Define dependency graph between capabilities (prerequisite matrix)
- [ ] Define risk class per capability (semantic drift impact)
- [ ] Add deterministic diagnostic code namespace for syntax failures
- [ ] Add coverage accounting spec by capability ID
- [ ] Add automated report generation for supported/unsupported/planned counts
- [ ] Add policy that framework names cannot appear in capability IDs

### M2 — Expressions and operators
- [ ] Literals: number/string/bool/null/bigint/symbol literal handling
- [ ] Template literals and tagged templates
- [ ] Destructuring assignment/object-rest/array-rest
- [ ] Optional chaining and nullish coalescing
- [ ] Logical assignment operators (`&&=`, `||=`, `??=`)
- [ ] Unary/binary operator precedence parity
- [ ] Equality semantics (`==`, `===`, `Object.is`) contract
- [ ] Short-circuit evaluation side-effect parity
- [ ] Computed property access and dynamic key semantics

### M3 — Statements and control flow
- [ ] `if/else` and conditional expression equivalence
- [ ] `switch` fallthrough and default behavior parity
- [ ] `for`, `while`, `do-while` control-flow lowering parity
- [ ] `for...of` iterator protocol mapping
- [ ] `for...in` enumerable property semantics
- [ ] `break`/`continue` with labels
- [ ] `try/catch/finally` propagation and rethrow behavior
- [ ] `throw` value semantics and stack mapping policy
- [ ] `return`/`yield`/`await` control edge handling

### M4 — Functions, closures, classes, prototypes
- [ ] Function declaration/expression/arrow semantics parity
- [ ] Default/rest parameters and arguments object behavior
- [ ] Closure capture and lexical environment correctness
- [ ] `this` binding rules (`call/apply/bind`, arrow lexical `this`)
- [ ] Class fields, methods, static blocks, private members
- [ ] Inheritance (`extends`, `super`) dispatch parity
- [ ] Prototype chain lookup/write behavior
- [ ] Accessor properties (`get`/`set`) semantics
- [ ] Constructor return/value edge cases

### M5 — Modules, type artifacts, and build artifacts
- [ ] ESM import/export forms parity (default/named/namespace/re-export)
- [ ] CommonJS interop semantics (require/module.exports)
- [ ] Circular dependency initialization ordering contract
- [ ] Dynamic import behavior policy and diagnostics
- [ ] Side-effect-only imports and execution ordering
- [ ] d.ts symbol surface normalization rules
- [ ] Sourcemap provenance invariants across map variants
- [ ] Multi-file module identity stability across builds
- [ ] Tree-shaken artifact semantics preservation contract

### M6 — Built-ins and standard library semantics
- [ ] `Object` core operations (`assign`, descriptors, keys/entries/values)
- [ ] `Array` methods and sparse array semantics
- [ ] `Map`/`Set` insertion order and iteration parity
- [ ] `Date` parsing/formatting timezone policy
- [ ] `RegExp` flags and match group behavior
- [ ] `JSON` parse/stringify edge semantics
- [ ] `Promise` state transition and chaining behavior
- [ ] `Error` subclasses and cause/stack policy
- [ ] `Math` numeric behavior and NaN/Infinity handling

### M7 — Async runtime model and event semantics
- [ ] Microtask vs macrotask scheduling contract
- [ ] `async`/`await` suspension and resume semantics
- [ ] Promise combinators (`all`, `allSettled`, `race`, `any`)
- [ ] Async iterators and `for await...of`
- [ ] Cancellation/timeout policy surface (AbortSignal mapping)
- [ ] Unhandled rejection behavior contract
- [ ] Timer semantics (`setTimeout`, `setInterval`, clear ops) policy
- [ ] Deterministic ordering tests for concurrent async scenarios
- [ ] Async diagnostic attribution to original source locations

### M8 — Quality gates, coverage ratchet, production criteria
- [ ] Capability-level differential parity harness across TS runtime vs Go runtime
- [ ] Determinism gate: byte-level stable output across repeated builds
- [ ] Determinism gate: clean-room reproducibility across fresh workspaces
- [ ] Coverage ratchet: no regression in supported capability count
- [ ] Coverage ratchet: no regression in per-capability scenario/case counts
- [ ] Mandatory evidence bundle for every capability promotion
- [ ] Release block rule for incorrect SUPPORTED classification
- [ ] Production incident playbook for semantic regression
- [ ] Monthly roadmap review to convert unsupported backlog into implementation items

### M9 — TypeScript type-system fidelity and erasure boundaries
- [ ] Define type-erasure contract so runtime semantics stay unchanged by TS-only syntax removal
- [ ] Support generic function/class syntax lowering without runtime behavioral drift
- [ ] Map conditional/mapped types metadata into diagnostics-quality hints (non-runtime)
- [ ] Define `enum`/`const enum` lowering policy and parity tests
- [ ] Define namespace/declaration-merging handling policy and deterministic diagnostics
- [ ] Add decorator syntax policy (supported subset vs deterministic reject) with migration guidance
- [ ] Add `.d.ts`-only symbol resolution ambiguity diagnostics with precise source ranges
- [ ] Add type-only import/export elision parity tests for side-effect safety
- [ ] Add `satisfies`/`as const` compile-surface handling policy and coverage cases

### M10 — Runtime interop, observability, and hardening
- [ ] Define JS number semantics vs Go numeric type mapping contract (overflow/precision boundaries)
- [ ] Define UTF-16 string semantic edge handling policy (surrogate pairs, indexing caveats)
- [ ] Add panic/error boundary contract to preserve JS-style error observability guarantees
- [ ] Add source-map-linked stack trace normalization spec for generated Go runtime failures
- [ ] Add debugger/profiler metadata emission contract for generated Go artifacts
- [ ] Add sandbox/security policy for dynamic code surfaces (`eval`, `Function` constructor)
- [ ] Add module loading trust policy for remote/file URL edge cases and deterministic rejection paths
- [ ] Add memory/GC-sensitive semantics watchlist and differential stress tests
- [ ] Add long-run stability suite for async/resource leak regression detection

### M11 — Conformance corpus and fuzzing expansion
- [ ] Build ECMAScript grammar-category fixture corpus with per-capability traceability IDs
- [ ] Add reducer-based minimization pipeline for failing JS->Go parity cases
- [ ] Add AST-level mutation fuzzing for expression/control-flow stress coverage
- [ ] Add artifact-level fuzzing for malformed sourcemap/d.ts/manifest resilience
- [ ] Add differential oracle checks across Node LTS variants for reference stability
- [ ] Add seeded randomized scenario packs with deterministic replay metadata
- [ ] Add flaky-case quarantine policy with auto-expiry and owner escalation
- [ ] Add nightly long-matrix conformance runs with trend snapshots
- [ ] Add coverage heatmap export for capability buckets vs fixture families

### M12 — Migration UX and ecosystem compatibility
- [ ] Add unsupported-syntax to migration-hint catalog with codemod-ready recipes
- [ ] Define compatibility profile tiers (`strict`, `balanced`, `max`) and gate impacts
- [ ] Add compiler guidance for replacing host-bound APIs with portable abstractions
- [ ] Add deterministic deprecation lifecycle policy for temporary syntax shims
- [ ] Add package-boundary interop tests for mixed JS/TS monorepo layouts
- [ ] Add generated-Go API stability policy for downstream integration points
- [ ] Add error-message localization/normalization policy for CI readability
- [ ] Add adoption playbook templates for phased rollout in existing services
- [ ] Add success metrics rubric (migration lead time, rollback rate, parity defect rate)

## Exception baseline (explicitly allowed unsupported)
- Direct browser DOM manipulation (`window`, `document`, layout/paint/event-loop coupling to browser engine)
- Browser rendering pipeline dependent behavior (CSSOM/layout timing)
- Host embeddings requiring platform-specific runtime injection outside compiler scope

All other language-level syntax/semantics are roadmap implementation targets.

## Checklist summary
- Total checklist items: 116
- Completed now: 3
- Remaining: 113
