# MDN JS Census Matrix (Issue #117)

## Purpose
Define a compiler-executable census format for MDN JavaScript coverage tracking.

This matrix is the bridge between:
- MDN reference taxonomy (human-readable source index)
- `tsgodown` compiler capability planning (parser/analyzer/IR/emitter/runtime)

## Domain buckets
- `LANG_CORE`: ECMAScript language syntax/semantics
- `BUILTIN`: JS built-in objects and standard library methods
- `HOST_WEB`: Web platform APIs (including non-DOM APIs like `fetch`, streams, workers)
- `HOST_NODE`: Node.js host/runtime APIs

## Status buckets
- `SUPPORTED`: implemented + parity evidence attached
- `PLANNED`: accepted roadmap target, not yet complete
- `EXCEPTION`: explicit non-target (must include rationale and deterministic diagnostics policy)

## Required columns

| capability_id | mdn_reference | domain | status | parser | analyzer | ir | emitter | runtime | diagnostics | parity_evidence | determinism_evidence | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|

## Seed rows (starter set)

| capability_id | mdn_reference | domain | status | parser | analyzer | ir | emitter | runtime | diagnostics | parity_evidence | determinism_evidence | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| lang.if_else | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Statements/if...else | LANG_CORE | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | control-flow baseline |
| lang.switch | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Statements/switch | LANG_CORE | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | fallthrough semantics |
| lang.for_of | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Statements/for...of | LANG_CORE | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | iterator protocol |
| lang.try_catch_finally | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Statements/try...catch | LANG_CORE | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | throw propagation |
| lang.async_function | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Statements/async_function | LANG_CORE | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | microtask behavior |
| lang.optional_chaining | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Operators/Optional_chaining | LANG_CORE | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | null propagation |
| lang.nullish_coalescing | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Operators/Nullish_coalescing | LANG_CORE | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | short-circuit parity |
| builtin.array_core | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Global_Objects/Array | BUILTIN | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | sparse behavior required |
| builtin.object_core | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Global_Objects/Object | BUILTIN | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | descriptor semantics |
| builtin.map_set | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Global_Objects/Map | BUILTIN | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | insertion-order contract |
| builtin.promise | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Global_Objects/Promise | BUILTIN | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | resolution/rejection ordering |
| builtin.regexp | https://developer.mozilla.org/docs/Web/JavaScript/Reference/Global_Objects/RegExp | BUILTIN | PLANNED | TODO | TODO | TODO | TODO | TODO | TODO | TBD | TBD | flags/groups parity |
| host_web.fetch | https://developer.mozilla.org/docs/Web/API/Fetch_API | HOST_WEB | PLANNED | N/A | TODO | TODO | TODO | TODO | TODO | TBD | TBD | host boundary separated from DOM |
| host_web.streams | https://developer.mozilla.org/docs/Web/API/Streams_API | HOST_WEB | PLANNED | N/A | TODO | TODO | TODO | TODO | TODO | TBD | TBD | streaming contract |
| host_web.workers | https://developer.mozilla.org/docs/Web/API/Web_Workers_API | HOST_WEB | PLANNED | N/A | TODO | TODO | TODO | TODO | TODO | TBD | TBD | concurrency semantics |
| host_web.webcrypto | https://developer.mozilla.org/docs/Web/API/Web_Crypto_API | HOST_WEB | PLANNED | N/A | TODO | TODO | TODO | TODO | TODO | TBD | TBD | crypto API policy |
| host_node.fs | https://nodejs.org/api/fs.html | HOST_NODE | PLANNED | N/A | TODO | TODO | TODO | TODO | TODO | TBD | TBD | host-node capability track |
| host_node.path | https://nodejs.org/api/path.html | HOST_NODE | PLANNED | N/A | TODO | TODO | TODO | TODO | TODO | TBD | TBD | path normalization |
| host_node.url | https://nodejs.org/api/url.html | HOST_NODE | PLANNED | N/A | TODO | TODO | TODO | TODO | TODO | TBD | TBD | URL/URLSearchParams |
| host_web.dom_direct_control | https://developer.mozilla.org/docs/Web/API/Document | HOST_WEB | EXCEPTION | N/A | N/A | N/A | N/A | N/A | REQUIRED | N/A | N/A | explicit host-bound exception |

## Rules
1. Every checklist claim in issue `#117` should map to at least one `capability_id`.
2. `EXCEPTION` entries require rationale and deterministic compile-time rejection behavior.
3. `SUPPORTED` requires both parity and determinism evidence fields.
4. The matrix is append-only by default; reclassification requires changelog note in issue `#117`.
