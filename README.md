# tsgodown

## What is tsgodown

`tsgodown` compiles a constrained Fastify codebase into a Go HTTP server.

- **Input**: TypeScript/JavaScript app code (Fastify-first profile)
- **Compiler core**: Rust analyzer + IR pipeline
- **Output**: Go project (`dist-go/main.go`) with deterministic routing behavior
- **Goal**: keep a familiar `tsdown`-style build UX while producing production-oriented Go artifacts

This repository is intentionally strict: when a Fastify pattern cannot be extracted deterministically, the compiler skips it and emits explicit diagnostics.

## Supported Fastify patterns

The current extractor supports patterns that are stable for backend builds:

- `fastify.<method>("/literal", handlerRef)` where method is `get|post|put|delete|patch`
- `fastify.route({ ... })` with inline object literal:
  - `method`: string (`"GET"`) or non-empty array (`["PUT", "PATCH"]`)
  - `url` or `path`: string literal
  - `handler`: named handler reference
- `fastify.register(...)` with:
  - inline callback plugin, or
  - named local plugin reference
  - optional `prefix` composition across nested plugins
- named handlers (including common member references) that the analyzer can resolve

Supported example:

```ts
import Fastify from "fastify";

const app = Fastify();

async function listUsers(req, reply) {
  return [{ id: "u1" }];
}

function replaceThing(req, reply) {
  reply.send({ ok: true });
}

app.get("/users", listUsers);
app.route({
  method: ["PUT", "PATCH"],
  url: "/things/:id",
  handler: replaceThing,
});
```

## Unsupported Fastify patterns + diagnostics codes

When unsupported patterns are found, `tsgodown` emits warnings with deterministic codes.

Common unsupported patterns:

- dynamic/non-literal route paths
- inline/anonymous route handlers where named references are required
- `fastify.route(...)` object shapes that are not directly analyzable
- unresolved plugin references
- conditional route registration (`if (...) { fastify.get(...) }`)

Example (unsupported):

```ts
import Fastify from "fastify";

const app = Fastify();

const dynamicPath = "/users/" + process.env.VERSION;
app.get(dynamicPath, async (req, reply) => {
  reply.send({ ok: true });
});

if (process.env.EXPERIMENTAL === "1") {
  app.get("/exp", expHandler);
}
```

Diagnostics you will see:

- `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`
- `ANALYZER_UNSUPPORTED_INLINE_HANDLER`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`
- `ANALYZER_UNRESOLVED_PLUGIN`
- `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`
- `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE`

## Quickstart

```bash
pnpm install --frozen-lockfile
pnpm run build

# Optional but recommended for local Rust-engine runs
cargo build -p engine-core
export TSGODOWN_RUST_ENGINE_BIN="$(pwd)/scripts/rust-engine-launcher.sh"
export TSGODOWN_ENGINE_CORE_BIN="$(pwd)/target/debug/engine-core"

cd examples/fastify-min
node --import tsx ../../packages/cli/src/index.ts build
```

Output:

- `examples/fastify-min/dist-go/main.go`

Scaffold-oriented sample (real Fastify app structure) is available at:

- `examples/fastify-scaffold-real/src/app.ts`
- `examples/fastify-scaffold-real/src/routes/*`

## Runtime contracts (404/405/Allow)

Generated Go runtime keeps HTTP mismatch behavior deterministic:

- **404 Not Found**: no route matched by path
- **405 Method Not Allowed**: path matched but HTTP method is unsupported
- **Allow header on 405**: includes allowed methods for the matched path (comma-separated)

This contract is verified in emitter/CLI tests and in smoke workflows.

## Diagnostics/Fix guide pointers

When build output is incomplete, start here:

- Fastify complex runbook: [`docs/FASTIFY_COMPLEX_RUNBOOK.md`](docs/FASTIFY_COMPLEX_RUNBOOK.md)
- Failure triage playbook: [`docs/operations/FAILURE_TRIAGE_PLAYBOOK.md`](docs/operations/FAILURE_TRIAGE_PLAYBOOK.md)
- M1 release gate and acceptance criteria: [`docs/specs/M1_RELEASE_GATE.md`](docs/specs/M1_RELEASE_GATE.md)
- Testing strategy and boundaries: [`docs/specs/TESTING_STRATEGY.md`](docs/specs/TESTING_STRATEGY.md)

Additional project docs:

- Architecture overview: [`docs/architecture/OVERVIEW.md`](docs/architecture/OVERVIEW.md)
- IR spec: [`docs/specs/IR_SPEC.md`](docs/specs/IR_SPEC.md)
- Capability matrix: [`docs/specs/CAPABILITY_MATRIX.md`](docs/specs/CAPABILITY_MATRIX.md)
- Artifact schema: [`docs/specs/ARTIFACT_SCHEMA.md`](docs/specs/ARTIFACT_SCHEMA.md)

## Development commands

- `pnpm run lint`
- `pnpm run format:check`
- `pnpm run test:tdd`
- `pnpm run perf:baseline`
- `pnpm run devx:fastify-complex`
- `./scripts/smoke-m1.sh`

## License

MIT
