# AOT Emission Ledger

This ledger tracks removal of the generated-Go IR JSON interpreter path.
Generated Go must be typed, ahead-of-time emitted source for supported
semantics. Corpus success does not count if codegen emits
`tsgodownrt.RunProgram("<IR JSON>")` or package-specific shortcuts.

| Key | Area | Contract Status | Go Status | Diagnostic | Evidence | Notes |
|---|---|---|---|---|---|---|
| aot.entry.module | Entry module emission | WIP | WIP | AOT_ENTRY_MODULE_UNSUPPORTED | focused unit tests | Import-free entry module can emit direct `main`; multi-module entry still falls back. |
| aot.module.registry | Module registry | WIP | WIP | AOT_MODULE_REGISTRY_UNSUPPORTED | focused unit tests | Local ESM named function, class, and primitive value imports plus simple CJS `module.exports = functionName` / `module.exports = function (...) { ... }` / `module.exports = ClassName` default `require` imports, `exports.name = functionName` namespace calls, object namespace exports such as `const api = { fn }; module.exports = api`, inline `module.exports = { fn }`, and re-exported CJS default function namespaces can bind function declarations, basic classes, and top-level/default function-expression bindings to direct Go declarations without `RunProgram`; CJS general object values, full default interop, cache records pending. |
| aot.module.init_order | Module init and cache order | TODO | TODO | AOT_MODULE_INIT_ORDER_UNSUPPORTED | planned | Need CJS/ESM evaluation order, cache, and circular dependency parity. |
| aot.function.decl | Function declarations/classes | WIP | WIP | AOT_FUNCTION_DECL_UNSUPPORTED | focused unit tests | Simple numeric, boolean, and string-parameter function declarations plus top-level `const fn = (...) => { return ... }` / `const fn = function () { return ... }` bindings emit direct Go functions; basic constructor field assignment classes emit Go structs, constructors, and methods. Closures, full `this`, rest, async, generator pending. |
| aot.function.call | Function calls | WIP | WIP | AOT_FUNCTION_CALL_UNSUPPORTED | focused unit tests | Direct calls to known AOT functions work for numeric, boolean, and simple string arguments; zero-argument known boolean-return calls can feed boolean predicates. Dynamic call/callable object pending. |
| aot.scope.lexical_slots | Lexical slots | WIP | WIP | AOT_LEXICAL_SLOT_UNSUPPORTED | focused unit tests | Top-level bindings, function params, and function-local typed `var`/`const` slots work for small numeric/string/bool/object subset. TDZ and block scope pending. |
| aot.scope.captured_slots | Captured slots and closures | TODO | TODO | AOT_CAPTURED_SLOT_UNSUPPORTED | planned | Need closure environment structs and mutation semantics. |
| aot.control.if_return | Native if and return control flow | WIP | WIP | AOT_CONTROL_IF_RETURN_UNSUPPORTED | focused unit tests | Top-level `if` and function-local `if` plus `return` emit native Go control flow for numeric predicates. |
| aot.control.loops | Native loop control flow | WIP | WIP | AOT_LOOP_UNSUPPORTED | focused unit tests | Simple top-level and function-local numeric `for` loops plus numeric `while` with unlabeled `break`/`continue` emit native Go control flow. `for-of`, labeled flow, do-while, and iterator closing pending. |
| aot.expr.numeric | Numeric expressions | WIP | WIP | AOT_NUMERIC_EXPR_UNSUPPORTED | focused unit tests | Numeric literals, arithmetic, assignment, update, comparisons, strict equality, typed conditional expressions, string `.indexOf`, and string `.length` for number subset. JS coercion matrix pending. |
| aot.expr.string | String expressions | WIP | WIP | AOT_STRING_EXPR_UNSUPPORTED | focused unit tests | String literals, interpolated template literals for typed string slots, string parameter inference from concatenation/method calls/templates, `typeof === "string"` branch narrowing, string `+`, string `+=`, typed conditional expressions, `typeof`, global `String(...)` for primitive slots, `.trim`, and `.toLowerCase` for local slots emit typed Go strings. Full JS string object/method/coercion semantics pending. |
| aot.expr.boolean | Boolean expressions | WIP | WIP | AOT_BOOLEAN_EXPR_UNSUPPORTED | focused unit tests | Boolean literals, numeric/string/bool/`typeof` comparison predicates, global `Boolean(...)` for primitive slots, logical-not, logical-and, logical-or, typed conditional expressions, string `.includes`, and primitive string-array `.includes` for bool subset. Full truthiness/coercion matrix pending. |
| aot.property.static | Static property access | WIP | WIP | AOT_STATIC_PROPERTY_UNSUPPORTED | focused unit tests | Simple object literal fields emit Go struct fields with direct static property access. Array/string/module property lookup pending. |
| aot.property.dynamic | Dynamic property access | TODO | TODO | AOT_DYNAMIC_PROPERTY_UNSUPPORTED | planned | Need computed keys, symbol keys, prototype lookup, getters/setters. |
| aot.value.model | Typed Value model | WIP | WIP | AOT_VALUE_MODEL_UNSUPPORTED | focused unit tests | Numeric, string, and boolean slots emit typed Go primitives, including local ESM named value imports; simple object literals emit typed Go structs; JSON report-shaped array/object/null values emit `[]any`/`map[string]any` plus direct `JSON.stringify` helper. Full backend-neutral JS value contract still pending. |
| aot.node.builtins | Node builtin helpers | WIP | WIP | AOT_NODE_BUILTIN_UNSUPPORTED | focused unit tests | Node builtin import specs such as `fs` and `node:crypto` are accepted in the module graph and unused bindings do not force fail-closed; `process.stdout.isTTY` emits a Go helper using `os.Stdout.Stat()` and static `process.env.NAME` reads emit `os.Getenv`. Broader builtin operations still need generic Node LTS helper lowering and fail closed when observed. |
| aot.async.promise_timer | Promise and timer ordering | TODO | TODO | AOT_ASYNC_UNSUPPORTED | planned | Need direct async contract ops for Promise jobs, nextTick, timers, immediates. |
| aot.diagnostics.fail_closed | Deterministic fail-closed diagnostics | WIP | WIP | AOT_UNSUPPORTED_SEMANTIC | runtime contract tests, focused AOT tests | Product default now fails closed when Go AOT emission cannot render supported IR and reports deterministic AOT feature details for unsupported classes, module bindings, function bodies, unresolved imports, and observed Node builtin operations; legacy IR interpreter only runs under explicit `legacy-ir-interpreter` test profile. |
| aot.no_ir_json_interpreter | No IR JSON interpreter in generated Go | WIP | WIP | AOT_IR_JSON_INTERPRETER_FORBIDDEN | focused unit tests, generated small corpus | Simple console/function/control-flow, `console.log`/`console.error`, plus local ESM named function and value import cases assert no `RunProgram`; regenerated 10 small corpus generated projects fail closed without `RunProgram` when AOT is incomplete. Legacy IR interpreter runtime is emitted only under explicit `legacy-ir-interpreter` test profile. Real corpus AOT parity still pending. |
| aot.holdout.parity | Holdout parity beyond corpus names | TODO | TODO | AOT_HOLDOUT_PARITY_UNSUPPORTED | planned | Need syntax/API holdouts proving no corpus/test-id hardcode. |
| aot.benchmarks | AOT runtime benchmarks | TODO | TODO | AOT_BENCHMARK_UNTRACKED | planned | Need benchmarks proving direct code path, with execa shortcut excluded from score. |

## Gate Rules

- Required rows are enforced by `pnpm run gate:aot-emission-ledger`.
- `TODO` and `WIP` are allowed while developing.
- Final mode must reject `TODO` and `WIP`:

```bash
node scripts/check-ledger.mjs aot-emission --final
```
