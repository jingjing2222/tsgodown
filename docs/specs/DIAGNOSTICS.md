# DIAGNOSTICS

This document explains analyzer diagnostics and how to fix common extraction blockers.

## Fastify unsupported patterns (analyzer-rust)

The following codes are emitted when Fastify route extraction cannot proceed deterministically.

> Message wording below is aligned to current analyzer messages in:
> - `packages/analyzer-rust/src/routes.rs`
> - `packages/analyzer-rust/src/register.rs`
> - `packages/analyzer-rust/src/traversal.rs`

---

### `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE`

**Current message shape**

`conditional route registration in if-block is unsupported for deterministic extraction (<instance>.<method>(...)). Move route declaration to top-level plugin scope.`

**Bad**

```ts
if (process.env.NODE_ENV === "development") {
  fastify.get("/health", health);
}
```

**Fixed**

```ts
fastify.get("/health", health);
```

**Rationale**

Conditional route registration makes compile-time extraction non-deterministic. Routes must be declared at top-level plugin scope.

---

### `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`

**Current message shape**

`unsupported register callback pattern on <instance>.register(...). Use inline function(plugin) { ... } or named local plugin reference.`

**Bad**

```ts
const pluginFactory = makePlugin();
fastify.register(pluginFactory);
```

**Fixed**

```ts
function usersPlugin(plugin: any) {
  plugin.get("/users", listUsers);
}

fastify.register(usersPlugin);
```

_or_

```ts
fastify.register(function (plugin) {
  plugin.get("/users", listUsers);
});
```

**Rationale**

Only inline plugin callbacks and same-file named plugin references are resolved reliably during static analysis.

---

### `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`

**Current message shapes**

- `unsupported dynamic path in <instance>.<method>(...). Use string literal path (e.g. '/users/:id') for IR extraction.`
- `unsupported route object path in <instance>.route({...}). Provide string literal 'url' or 'path' (e.g. '/users/:id').`

**Bad**

```ts
const path = `/users/${id}`;
fastify.get(path, getUser);
```

_or_

```ts
const url = computeUrl();
fastify.route({ method: "GET", url, handler: getUser });
```

**Fixed**

```ts
fastify.get("/users/:id", getUser);
```

_or_

```ts
fastify.route({ method: "GET", url: "/users/:id", handler: getUser });
```

**Rationale**

Route paths must be string literals so analyzer output is deterministic and IR-ready.

---

### `ANALYZER_UNSUPPORTED_INLINE_HANDLER`

**Current message shapes**

- `unsupported non-reference handler in <instance>.<method>('<path>', handler). Extract handler to a named function and pass its identifier.`
- `unsupported route object handler in <instance>.route({...}). Provide named handler reference in 'handler' field.`

**Bad**

```ts
fastify.post("/users", async (req, reply) => {
  reply.send({ ok: true });
});
```

_or_

```ts
fastify.route({
  method: "POST",
  url: "/users",
  handler: async (req, reply) => {
    reply.send({ ok: true });
  },
});
```

**Fixed**

```ts
async function createUser(req: unknown, reply: any) {
  reply.send({ ok: true });
}

fastify.post("/users", createUser);
```

_or_

```ts
fastify.route({ method: "POST", url: "/users", handler: createUser });
```

**Rationale**

Named handler references allow stable handler IDs and metadata extraction.

---

### `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`

**Current message shape**

`unsupported route object pattern in <instance>.route(...). Provide an inline object literal (e.g. { method: 'GET', url: '/users', handler: listUsers }).`

**Bad**

```ts
const routeDef = { method: "GET", url: "/users", handler: listUsers };
fastify.route(routeDef);
```

**Fixed**

```ts
fastify.route({ method: "GET", url: "/users", handler: listUsers });
```

**Rationale**

Analyzer requires an inline object literal for `fastify.route(...)` to avoid unresolved indirection.

---

### `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`

**Current message shapes**

- `unsupported route object method in <instance>.route({...}): missing string 'method' or non-empty string array. Supported methods: GET|POST|PUT|DELETE|PATCH.`
- `unsupported route object method in <instance>.route({...}): '<method>'. Supported methods: GET|POST|PUT|DELETE|PATCH.`

**Bad**

```ts
fastify.route({ url: "/users", handler: listUsers });
```

_or_

```ts
fastify.route({ method: "OPTIONS", url: "/users", handler: listUsers });
```

**Fixed**

```ts
fastify.route({ method: "GET", url: "/users", handler: listUsers });
```

_or_

```ts
fastify.route({ method: ["GET", "POST"], url: "/users", handler: usersHandler });
```

**Rationale**

Route-object methods must be present and limited to currently supported HTTP methods for deterministic extraction and emission.
