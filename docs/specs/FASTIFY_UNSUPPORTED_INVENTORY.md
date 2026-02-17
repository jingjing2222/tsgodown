# FASTIFY_UNSUPPORTED_INVENTORY

Canonical inventory of diagnostics emitted by `packages/analyzer-rust` (codes + verbatim message templates).

This file is auto-managed by `scripts/check-fastify-diagnostics-sync.mjs`.

| Code | Message template (verbatim) | Source file(s) |
| --- | --- | --- |
| `ANALYZER_UNRESOLVED_PLUGIN` | `register plugin '{}' could not be resolved in current file. Ensure plugin is declared in the same file or use an inline callback.` | `packages/analyzer-rust/src/register.rs` |
| `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE` | `conditional route registration in if-block is unsupported for deterministic extraction ({}.{}(...)). Move route declaration to top-level plugin scope.` | `packages/analyzer-rust/src/traversal.rs` |
| `ANALYZER_UNSUPPORTED_DYNAMIC_PATH` | `unsupported dynamic path in {}.{}(...). Use string literal path (e.g. '/users/:id') for IR extraction.` | `packages/analyzer-rust/src/routes.rs` |
| `ANALYZER_UNSUPPORTED_DYNAMIC_PATH` | `unsupported route object path in {}.route({{...}}). Provide string literal 'url' or 'path' (e.g. '/users/:id').` | `packages/analyzer-rust/src/routes.rs` |
| `ANALYZER_UNSUPPORTED_INLINE_HANDLER` | `unsupported non-reference handler in {}.{}('{}', handler). Extract handler to a named function and pass its identifier.` | `packages/analyzer-rust/src/routes.rs` |
| `ANALYZER_UNSUPPORTED_INLINE_HANDLER` | `unsupported route object handler in {}.route({{...}}). Provide named handler reference in 'handler' field.` | `packages/analyzer-rust/src/routes.rs` |
| `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK` | `unsupported register callback pattern on {}.register(...). Use inline function(plugin) {{ ... }} or named local plugin reference.` | `packages/analyzer-rust/src/register.rs` |
| `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD` | `unsupported route object method in {}.route({{...}}): '{}'. Supported methods: GET\|POST\|PUT\|DELETE\|PATCH.` | `packages/analyzer-rust/src/routes.rs` |
| `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD` | `unsupported route object method in {}.route({{...}}): missing string 'method' or non-empty string array. Supported methods: GET\|POST\|PUT\|DELETE\|PATCH.` | `packages/analyzer-rust/src/routes.rs` |
| `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE` | `unsupported route object pattern in {}.route(...). Provide an inline object literal (e.g. {{ method: 'GET', url: '/users', handler: listUsers }}).` | `packages/analyzer-rust/src/routes.rs` |
| `DYNAMIC_IMPORT_DETECTED` | `dynamic import detected; use static import declarations for deterministic IR extraction.` | `packages/analyzer-rust/src/lib.rs` |

## Regeneration

- Check only: `node scripts/check-fastify-diagnostics-sync.mjs`
- Rewrite docs blocks + inventory: `node scripts/check-fastify-diagnostics-sync.mjs --write`
