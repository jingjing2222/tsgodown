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
- [ ] Freeze milestone naming and progression policy for all reports
- [ ] Publish "supported/unsupported/planned" transition criteria

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

## Exception baseline (explicitly allowed unsupported)
- Direct browser DOM manipulation (`window`, `document`, layout/paint/event-loop coupling to browser engine)
- Browser rendering pipeline dependent behavior (CSSOM/layout timing)
- Host embeddings requiring platform-specific runtime injection outside compiler scope

All other language-level syntax/semantics are roadmap implementation targets.

## Checklist summary
- Total checklist items: 81
- Completed now: 2
- Remaining: 79
