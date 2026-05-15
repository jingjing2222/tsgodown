# AOT Emission Ledger

This ledger tracks removal of the generated-Go IR JSON interpreter path.
Generated Go must be typed, ahead-of-time emitted source for supported
semantics. Corpus success does not count if codegen emits
`tsgodownrt.RunProgram("<IR JSON>")` or package-specific shortcuts.

| Key | Area | Contract Status | Go Status | Diagnostic | Evidence | Notes |
|---|---|---|---|---|---|---|
| aot.entry.module | Entry module emission | WIP | WIP | AOT_ENTRY_MODULE_UNSUPPORTED | focused unit tests | Import-free entry module can emit direct `main`; multi-module entry still falls back. |
| aot.module.registry | Module registry | WIP | WIP | AOT_MODULE_REGISTRY_UNSUPPORTED | focused unit tests | Local ESM named function and primitive value imports can bind to direct Go declarations without `RunProgram`; CJS, default interop, cache records pending. |
| aot.module.init_order | Module init and cache order | TODO | TODO | AOT_MODULE_INIT_ORDER_UNSUPPORTED | planned | Need CJS/ESM evaluation order, cache, and circular dependency parity. |
| aot.function.decl | Function declarations | WIP | WIP | AOT_FUNCTION_DECL_UNSUPPORTED | focused unit tests | Simple numeric functions emit direct Go functions. Closures, `this`, rest, async, generator pending. |
| aot.function.call | Function calls | WIP | WIP | AOT_FUNCTION_CALL_UNSUPPORTED | focused unit tests | Direct calls to known AOT functions work for numeric arguments. Dynamic call/callable object pending. |
| aot.scope.lexical_slots | Lexical slots | WIP | WIP | AOT_LEXICAL_SLOT_UNSUPPORTED | focused unit tests | Top-level bindings and function params work for small numeric subset. TDZ and block scope pending. |
| aot.scope.captured_slots | Captured slots and closures | TODO | TODO | AOT_CAPTURED_SLOT_UNSUPPORTED | planned | Need closure environment structs and mutation semantics. |
| aot.control.if_return | Native if and return control flow | WIP | WIP | AOT_CONTROL_IF_RETURN_UNSUPPORTED | focused unit tests | Top-level `if` and function-local `if` plus `return` emit native Go control flow for numeric predicates. |
| aot.control.loops | Native loop control flow | WIP | WIP | AOT_LOOP_UNSUPPORTED | focused unit tests | Simple top-level numeric `for` loops emit native Go control flow. `while`, `for-of`, labels, and iterator closing pending. |
| aot.expr.numeric | Numeric expressions | WIP | WIP | AOT_NUMERIC_EXPR_UNSUPPORTED | focused unit tests | Numeric literals, arithmetic, assignment, update, comparisons, and strict equality for number subset. JS coercion matrix pending. |
| aot.expr.boolean | Boolean expressions | WIP | WIP | AOT_BOOLEAN_EXPR_UNSUPPORTED | focused unit tests | Boolean literals, comparison predicates, logical-not, logical-and, logical-or for bool subset. Truthiness pending. |
| aot.property.static | Static property access | WIP | WIP | AOT_STATIC_PROPERTY_UNSUPPORTED | focused unit tests | Simple object literal fields emit Go struct fields with direct static property access. Array/string/module property lookup pending. |
| aot.property.dynamic | Dynamic property access | TODO | TODO | AOT_DYNAMIC_PROPERTY_UNSUPPORTED | planned | Need computed keys, symbol keys, prototype lookup, getters/setters. |
| aot.value.model | Typed Value model | WIP | WIP | AOT_VALUE_MODEL_UNSUPPORTED | focused unit tests | Numeric, string, and boolean slots emit typed Go primitives, including local ESM named value imports; simple object literals emit typed Go structs. Full backend-neutral JS value contract still pending. |
| aot.node.builtins | Node builtin helpers | TODO | TODO | AOT_NODE_BUILTIN_UNSUPPORTED | planned | Builtins must render generic Node LTS helpers, not package branches. |
| aot.async.promise_timer | Promise and timer ordering | TODO | TODO | AOT_ASYNC_UNSUPPORTED | planned | Need direct async contract ops for Promise jobs, nextTick, timers, immediates. |
| aot.diagnostics.fail_closed | Deterministic fail-closed diagnostics | WIP | WIP | AOT_UNSUPPORTED_SEMANTIC | runtime contract tests | Product default now fails closed when Go AOT emission cannot render supported IR; legacy IR interpreter only runs under explicit `legacy-ir-interpreter` test profile. |
| aot.no_ir_json_interpreter | No IR JSON interpreter in generated Go | WIP | WIP | AOT_IR_JSON_INTERPRETER_FORBIDDEN | focused unit tests, generated small corpus | Simple console/function/control-flow plus local ESM named function and value import cases assert no `RunProgram`; regenerated 10 small corpus `main.go` and `vector_suite.go` files fail closed without `RunProgram` when AOT is incomplete. Runtime interpreter removal from generated helper package and real corpus AOT parity still pending. |
| aot.holdout.parity | Holdout parity beyond corpus names | TODO | TODO | AOT_HOLDOUT_PARITY_UNSUPPORTED | planned | Need syntax/API holdouts proving no corpus/test-id hardcode. |
| aot.benchmarks | AOT runtime benchmarks | TODO | TODO | AOT_BENCHMARK_UNTRACKED | planned | Need benchmarks proving direct code path, with execa shortcut excluded from score. |

## Gate Rules

- Required rows are enforced by `pnpm run gate:aot-emission-ledger`.
- `TODO` and `WIP` are allowed while developing.
- Final mode must reject `TODO` and `WIP`:

```bash
node scripts/check-ledger.mjs aot-emission --final
```
