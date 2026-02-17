# FASTIFY_SUPPORT_MATRIX

Status: **SSOT** for Fastify mapping support in analyzer → IR extraction.

This document defines what Fastify route declaration patterns are currently mapped, what is conditionally supported, and what is explicitly unsupported with deterministic diagnostics.

---

## 1) Supported patterns

The following patterns are supported and expected to extract deterministic `RouteIR` + `HandlerIR`.

### 1.1 Shorthand route registration (named handler refs)

```ts
fastify.get('/users', listUsers)
fastify.post('/users', createUser)
fastify.patch('/users/:id', updateUser)
fastify.put('/users/:id', replaceUser)
fastify.delete('/users/:id', removeUser)
```

Notes:
- Path must be a string literal.
- Handler must be a reference (identifier/member ref), not inline.

### 1.2 Chained calls

```ts
fastify
  .get('/users', listUsers)
  .post('/users', createUser)
  .patch('/users/:id', updateUser)
```

Also supported inside nested plugins when analyzable.

### 1.3 `fastify.route({...})` object literal

```ts
fastify.route({ method: 'GET', url: '/users', handler: listUsers })
fastify.route({ method: 'PATCH', path: '/users/:id', handler: updateUser })
```

### 1.4 Route object method array + normalized method variants

```ts
fastify.route({ method: ['PUT', 'PATCH'], url: '/things/:id', handler: replaceThing })
fastify.route({ method: 'patch', url: '/users/:id', handler: updateUser })
fastify.route({ method: ['put', 'del'], url: '/users/:id', handler: replaceUser })
```

Supported method set for route object extraction (case-insensitive, with `DEL` alias → `DELETE`):
- `GET`
- `POST`
- `PUT`
- `DELETE`
- `PATCH`

### 1.5 Prefix propagation through `register(...)`

```ts
fastify.register(apiPlugin, { prefix: '/api' })
fastify.register(fp(apiPlugin), { prefix: '/api' })
// plugin routes become /api/*
```

Nested `register(..., { prefix })` composition is supported when plugin callback/definition is statically analyzable in-file.
Single-argument wrapper calls are unwrapped deterministically (e.g. `fp(pluginRef)`).

### 1.6 Handler reference forms

Named/member references are supported, including object/class member references when statically resolvable:

```ts
fastify.get('/users', userHandlers.list)
fastify.delete('/users/:id', controller.remove)
```

### 1.7 Deterministic inline handler synthesis

Inline function/arrow handlers are supported when their signature is statically parseable.
Analyzer synthesizes stable handler refs and emits `HandlerIR` with inferred params/async flag.

```ts
fastify.get('/health', async (req, reply) => reply.send({ ok: true }))
fastify.route({ method: 'POST', url: '/users', handler: function (request) { return {} } })
```

---

## 2) Limited / conditional behavior

These cases are supported with constraints (or intentionally bounded for deterministic extraction):

- **Conditional route declarations are partially supported for compile-time constant conditions only.**
  - Supported condition subset: boolean literals with `!`, `&&`, `||`, and parentheses (e.g. `if (true && !false) { ... } else { ... }`).
  - Analyzer extracts only the statically active branch.
  - Non-constant conditions still emit `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE`.

- **Dynamic path expressions are partially supported for static template literals only.**
  - Supported: backtick literals without interpolation (e.g. `` `/users/:id` ``).
  - Rejected: template literals containing `${...}` and non-literal path expressions.
  - Rejected cases emit `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`.

- **Plugin/register analysis is static and file-local.**
  - Prefix composition + nested plugin traversal works for analyzable callbacks/definitions.
  - External/dynamic plugin resolution is outside this matrix’s supported boundary.

- **Method support is intentionally constrained in route-object mode.**
  - Route object methods must be one of `GET|POST|PUT|DELETE|PATCH`.

### 2.1 Partial-support policy table

| Pattern class | Supported subset | Rejected subset | Rationale |
| --- | --- | --- | --- |
| Conditional route registration | `if` condition statically reducible to boolean using literals + `!`/`&&`/`||`/parentheses; only active branch extracted | Runtime/env/data-dependent conditions | Keep route graph deterministic at compile time |
| Path literal shape | `'...'`, `"..."`, and static `` `...` `` literals (no `${}`) | Interpolated templates and computed expressions | Require concrete compile-time path strings |

---

## 3) Unsupported patterns (diagnostic SSOT)

For each unsupported boundary below, the diagnostic code is canonical.

## 3.1 `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`

When it triggers:
- Shorthand route path is non-literal/dynamic.
- Route object `url`/`path` is missing or non-literal.
- Template literal path includes interpolation (e.g. `` `/users/${id}` ``).

Note:
- Static template literals without interpolation (e.g. `` `/users/:id` ``) are supported.

Why unsupported:
- Deterministic IR extraction requires concrete compile-time path strings.

Recommended rewrite:

```ts
// ❌ unsupported
fastify.get(`/users/${id}`, listUser)

// ✅ supported
fastify.get('/users/:id', listUser)
```

Route object form:

```ts
// ❌ unsupported
fastify.route({ method: 'GET', url: buildPath(), handler: listUsers })

// ✅ supported
fastify.route({ method: 'GET', url: '/users', handler: listUsers })
```

## 3.2 `ANALYZER_UNSUPPORTED_INLINE_HANDLER`

When it triggers:
- Shorthand route handler expression is non-reference and signature cannot be deterministically parsed.
- Route object `handler` expression is non-reference and signature cannot be deterministically parsed.

Why unsupported:
- IR contracts require stable handler identity (`handler_ref`) and statically parseable handler signature.

Recommended rewrite:

```ts
// ❌ unsupported
fastify.get('/health', (req, reply) => reply.send('ok'))

// ✅ supported
const health = (req, reply) => reply.send('ok')
fastify.get('/health', health)
```

Route object form:

```ts
// ❌ unsupported
fastify.route({ method: 'POST', url: '/users', handler: async (req) => {} })

// ✅ supported
async function createUser(req) {}
fastify.route({ method: 'POST', url: '/users', handler: createUser })
```

## 3.3 `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`

When it triggers:
- `fastify.route(...)` is not passed an inline object literal shape analyzable by current extractor.

Why unsupported:
- Analyzer relies on direct object-literal inspection for deterministic extraction.

Recommended rewrite:

```ts
// ❌ unsupported
const opts = makeRouteOptions()
fastify.route(opts)

// ✅ supported
fastify.route({ method: 'GET', url: '/users', handler: listUsers })
```

## 3.4 `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`

When it triggers:
- Route object `method` missing/invalid shape.
- Route object uses unsupported method token (e.g. `OPTIONS`).

Why unsupported:
- Current IR mapping contract only accepts `GET|POST|PUT|DELETE|PATCH`.

Recommended rewrite:

```ts
// ❌ unsupported (missing method)
fastify.route({ url: '/users', handler: listUsers })

// ✅ supported
fastify.route({ method: 'GET', url: '/users', handler: listUsers })
```

```ts
// ❌ unsupported (unsupported method token)
fastify.route({ method: 'OPTIONS', url: '/users', handler: optionsUsers })

// ✅ supported (current boundary)
fastify.route({ method: 'GET', url: '/users', handler: listUsers })
```

## 3.5 `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE`

When it triggers:
- Fastify route registration appears in an `if`-block whose condition is not compile-time constant in the supported subset.

Why unsupported:
- Non-constant conditional registration can vary by runtime state/env, violating deterministic static extraction guarantees.

Recommended rewrite:

```ts
// ❌ unsupported
if (featureFlag) {
  fastify.get('/beta', betaHandler)
}

// ✅ supported (deterministic top-level declaration)
fastify.get('/beta', betaHandler)
```

If feature gating is needed, keep route declaration deterministic and gate behavior inside handler/service logic.

## 3.6 `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`

When it triggers:
- `.register(...)` callback/plugin reference is not an inline function or same-file named plugin reference resolvable by analyzer.

Why unsupported:
- Current plugin resolution boundary is intentionally static and file-local.

Recommended rewrite:

```ts
// ❌ unsupported
const pluginFactory = makePlugin()
fastify.register(pluginFactory)

// ✅ supported
function usersPlugin(plugin) {
  plugin.get('/users', listUsers)
}
fastify.register(usersPlugin)
```

or

```ts
fastify.register(function (plugin) {
  plugin.get('/users', listUsers)
})
```

## 3.7 `ANALYZER_UNRESOLVED_PLUGIN`

When it triggers:
- `register(<pluginRef>)` references a plugin not declared/resolvable in the same file under current static analysis boundary.

Why unsupported:
- Cross-file/dynamic plugin resolution is currently outside deterministic extraction scope.

Recommended rewrite:

```ts
// ❌ unsupported
import usersPlugin from './users-plugin'
fastify.register(usersPlugin)

// ✅ supported (current boundary)
function usersPlugin(plugin) {
  plugin.get('/users', listUsers)
}
fastify.register(usersPlugin)
```

---

## 4) Governance

### 4.1 Diagnostic code inventory linkage

The full code+message inventory is maintained in `docs/specs/FASTIFY_UNSUPPORTED_INVENTORY.md`.

<!-- AUTO-GENERATED:DIAGNOSTIC_CODES:START -->
- `ANALYZER_UNRESOLVED_PLUGIN`
- `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE`
- `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`
- `ANALYZER_UNSUPPORTED_INLINE_HANDLER`
- `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`
- `DYNAMIC_IMPORT_DETECTED`
<!-- AUTO-GENERATED:DIAGNOSTIC_CODES:END -->

- This file is the **single source of truth** for Fastify analyzer mapping status.
- If extractor behavior changes, update this file in the same PR as tests/implementation.
- Keep diagnostic code spellings exact and stable.
