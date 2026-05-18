# ECMAScript Semantics Ledger

This ledger tracks JavaScript semantics separately from Node.js APIs. Corpus
success is not enough; each language feature must be implemented generically or
fail closed before codegen.

| Key | Area | Contract Status | Go Status | Diagnostic | Evidence | Notes |
|---|---|---|---|---|---|---|
| es.values.primitives | Primitive values | WIP | WIP | ES_PRIMITIVE_UNSUPPORTED | corpus subset | undefined/null/boolean/number/string subset exists. |
| es.values.bigint | BigInt | TODO | TODO | ES_BIGINT_UNSUPPORTED | planned | Arithmetic, comparison, JSON errors. |
| es.values.symbol | Symbol | WIP | WIP | ES_SYMBOL_UNSUPPORTED | focused runtime subset | Registry, property keys, descriptions pending. |
| es.values.object_identity | Object identity | WIP | WIP | ES_OBJECT_IDENTITY_UNSUPPORTED | corpus subset | Reference equality and mutation semantics. |
| es.coercion | Coercion and equality | TODO | TODO | ES_COERCION_UNSUPPORTED | planned | ToPrimitive, ==, ===, relational comparisons. |
| es.scope.lexical | Lexical scope | WIP | WIP | ES_SCOPE_UNSUPPORTED | corpus subset | let/const/function scope and closures. |
| es.scope.hoist_tdz | Hoisting and TDZ | TODO | TODO | ES_HOIST_TDZ_UNSUPPORTED | planned | var/function/class hoist and TDZ errors. |
| es.functions.calls | Function calls | WIP | WIP | ES_FUNCTION_CALL_UNSUPPORTED | corpus subset | args, returns, closures. |
| es.functions.this_bind | `this`, call/apply/bind | TODO | TODO | ES_THIS_BIND_UNSUPPORTED | planned | Strict/non-strict this behavior. |
| es.functions.construct | Constructors/new | WIP | WIP | ES_CONSTRUCT_UNSUPPORTED | corpus subset | new, prototype, return override pending. |
| es.classes | Classes | WIP | WIP | ES_CLASS_UNSUPPORTED | focused runtime subset | Private members subset; super/static blocks pending. |
| es.objects.properties | Object properties | WIP | WIP | ES_PROPERTY_UNSUPPORTED | corpus subset | Descriptors/getters/setters pending. |
| es.objects.prototype | Prototype chain | WIP | WIP | ES_PROTOTYPE_UNSUPPORTED | focused AOT tests | `Object.create(proto)` lookup, `in` prototype-chain lookup, `Object.setPrototypeOf(...)`, and `Object.getPrototypeOf(...)` subsets lower without `RunProgram`; full function prototype identity, descriptors, accessors, null-prototype mutation, and `instanceof` edge cases pending. |
| es.objects.destructuring | Destructuring | WIP | WIP | ES_DESTRUCTURING_UNSUPPORTED | corpus subset | Defaults/rest/nested patterns pending. |
| es.objects.spread_rest | Spread/rest | WIP | WIP | ES_SPREAD_REST_UNSUPPORTED | corpus subset | Array/object/call spread edge cases pending. |
| es.arrays | Array semantics | WIP | WIP | ES_ARRAY_UNSUPPORTED | corpus subset | Holes, length, iteration, methods pending. |
| es.typed_arrays | ArrayBuffer/DataView/TypedArray | TODO | TODO | ES_TYPED_ARRAY_UNSUPPORTED | planned | Binary view semantics. |
| es.control.block_if_switch | Blocks/if/switch | WIP | WIP | ES_CONTROL_UNSUPPORTED | corpus subset | Switch completion edge cases pending. |
| es.control.loops_labels | Loops and labels | WIP | WIP | ES_LABEL_UNSUPPORTED | focused runtime tests | Labeled break/continue subset exists. |
| es.control.try_finally | try/catch/finally | WIP | WIP | ES_TRY_FINALLY_UNSUPPORTED | corpus subset | finally completion override pending. |
| es.iteration | Iterators/for-of | WIP | WIP | ES_ITERATION_UNSUPPORTED | corpus subset | Iterator closing/errors pending. |
| es.generators | Generators | TODO | TODO | ES_GENERATOR_UNSUPPORTED | planned | yield/return/throw. |
| es.async.promises | Promise semantics | WIP | WIP | ES_PROMISE_UNSUPPORTED | corpus async subset | Resolution/rejection/microtasks pending. |
| es.async.async_await | async/await | WIP | WIP | ES_ASYNC_AWAIT_UNSUPPORTED | corpus subset | Ordering/error propagation pending. |
| es.async.async_iteration | Async iteration | TODO | TODO | ES_ASYNC_ITERATION_UNSUPPORTED | planned | for-await and async iterators. |
| es.modules | ECMAScript modules | WIP | WIP | ES_MODULE_UNSUPPORTED | corpus module graph subset | Live bindings/TLA/cycles pending. |
| es.regexp | RegExp | WIP | WIP | ES_REGEXP_UNSUPPORTED | corpus/focused tests | Flags, sticky/unicode/groups pending. |
| es.date | Date | WIP | WIP | ES_DATE_UNSUPPORTED | focused runtime tests | Parsing/time zones/full method matrix pending. |
| es.json | JSON | WIP | WIP | ES_JSON_UNSUPPORTED | corpus/focused tests | Reviver/replacer/toJSON edge cases pending. |
| es.error | Error objects | WIP | WIP | ES_ERROR_UNSUPPORTED | corpus subset | Stack/cause/codes/errors pending. |
| es.map_set | Map/Set/WeakMap/WeakSet | WIP | WIP | ES_MAP_SET_UNSUPPORTED | corpus subset | Weak collections and iteration edge cases pending. |
| es.intl | Intl | TODO | TODO | ES_INTL_UNSUPPORTED | planned | ICU-backed behavior. |
| es.proxy_reflect | Proxy/Reflect | TODO | TODO | ES_PROXY_REFLECT_UNSUPPORTED | planned | Trap invariants and reflection. |
| es.eval_dynamic | eval/Function dynamic code | FAIL_CLOSED | FAIL_CLOSED | ES_DYNAMIC_EVAL_UNSUPPORTED | fail-closed planned | No JS engine fallback; source-level strategy required. |

## Gate Rules

- Required rows are enforced by `pnpm run gate:ecmascript-ledger`.
- `TODO` and `WIP` are allowed while developing.
- Final mode must reject `TODO` and `WIP`:

```bash
node scripts/check-ledger.mjs ecmascript --final
```
