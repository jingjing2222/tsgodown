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

### 1.4 Route object method array

```ts
fastify.route({ method: ['PUT', 'PATCH'], url: '/things/:id', handler: replaceThing })
```

Supported method set for route object extraction:
- `GET`
- `POST`
- `PUT`
- `DELETE`
- `PATCH`

### 1.5 Prefix propagation through `register(...)`

```ts
fastify.register(apiPlugin, { prefix: '/api' })
// plugin routes become /api/*
```

Nested `register(..., { prefix })` composition is supported when plugin callback/definition is statically analyzable in-file.

### 1.6 Handler reference forms

Named/member references are supported, including object/class member references when statically resolvable:

```ts
fastify.get('/users', userHandlers.list)
fastify.delete('/users/:id', controller.remove)
```

---

## 2) Limited / conditional behavior

These cases are supported with constraints (or intentionally bounded for deterministic extraction):

- **Conditional route declarations are not extracted inside `if` blocks.**
  - Determinism boundary: route graph must be statically stable.
  - Emits `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE` (see Unsupported section).

- **Plugin/register analysis is static and file-local.**
  - Prefix composition + nested plugin traversal works for analyzable callbacks/definitions.
  - External/dynamic plugin resolution is outside this matrix’s supported boundary.

- **Method support is intentionally constrained in route-object mode.**
  - Route object methods must be one of `GET|POST|PUT|DELETE|PATCH`.

---

## 3) Unsupported patterns (diagnostic SSOT)

For each unsupported boundary below, the diagnostic code is canonical.

## 3.1 `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`

When it triggers:
- Shorthand route path is non-literal/dynamic.
- Route object `url`/`path` is missing or non-literal.

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
- Shorthand route handler is inline (arrow/function expression) instead of handler reference.
- Route object `handler` is inline/non-reference.

Why unsupported:
- IR contracts require stable handler identity (`handler_ref`) for downstream mapping.

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
- Fastify route registration appears in `if`-block conditional scope.

Why unsupported:
- Conditional registration can vary by runtime state/env, violating deterministic static extraction guarantees.

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

---

## 4) Governance

- This file is the **single source of truth** for Fastify analyzer mapping status.
- If extractor behavior changes, update this file in the same PR as tests/implementation.
- Keep diagnostic code spellings exact and stable.
