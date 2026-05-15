# tsgodown

## What it does

`tsgodown` aims to compile Node.js projects that `tsdown` can bundle into
standalone Go projects.

You give it TypeScript or JavaScript Node.js source. It uses the same kind of
bundle artifacts you already get from `tsdown`: bundled JavaScript, sourcemaps,
declaration files, and package metadata. The output should be a Go project that
passes `go build` and runs without Node.js.

The target user outcome:

- build a TypeScript/JavaScript package with `tsgodown` instead of `tsdown`
- get a standalone Go project or binary
- run that Go binary and observe the same behavior as the original Node.js
  program
- use real Node workloads, not framework-specific demos

Target workloads include CLIs, libraries, HTTP frameworks, build tools,
compilers, ORMs, GraphQL servers, and full-stack frameworks. Express, NestJS,
Vite, Rollup, Webpack, Next, Nuxt, Astro, ESLint, Prettier, Babel, TypeScript,
GraphQL, Apollo, Socket.IO, and ORM packages are validation targets, not
special cases.

## Compatibility Target

The runtime baseline is the latest active Node.js LTS line. This repository pins
that local development environment with `mise`:

```bash
mise install
mise exec -- node --version
mise exec -- pnpm --version
```

Current pin:

- Node.js `24.15.0`
- pnpm `10.22.0`

Observable parity means the generated Go output matches the original Node.js
program for:

- stdout, stderr, and exit code
- returned JSON/library results
- environment variables, argv, and cwd
- filesystem side effects
- async completion order
- observed error shape

Generated Go must not embed Node.js, shell out to Node.js, use V8, rely on
Node-API/N-API, or load native addon fallback paths.

When a source program uses unsupported behavior, `tsgodown` must fail closed
with deterministic diagnostics instead of generating wrong Go.

## Status

Current green phase:

- 10 real Node utility/library corpus entries are vendored.
- Each has 100 Vitest vectors.
- Node execution, generated Go build/run, and Node/Go vector parity are green
  for that phase.

Not done yet:

- full latest-LTS Node.js API coverage ledger
- large package/application corpus
- complete backend-neutral plugin boundary
- runtime contract cleanup
- removal of remaining route-era implementation assumptions

Next corpus tiers:

- `test-corpus/node-real/`: 10 utility/library corpus entries, 100 vectors each.
- `test-corpus/node-large/`: planned 20 framework/tooling/application corpus
  entries, 100 vectors each.
- Every corpus must prove the same thing: original Node.js LTS behavior equals
  generated standalone Go behavior, with no corpus-specific compiler hacks.

## Milestone lock (execution sequence)

Documentation and gate execution follow this fixed sequence:

`M5 -> M1 -> M2 -> M3 -> M4`

- `M5`: compiler-mode direction lock and contract freeze
- `M1`: canonical compile-success gate (`pnpm run gate:m1`)
- `M2`: generated runtime reachability acceptance
- `M3`: deterministic runtime behavior/perf guard extensions
- `M4`: architecture guardrails, triage/release discipline, and DoD closure policy

The sequence is locked for planning/reporting consistency and should be used in issue/PR text, roadmap updates, and release evidence.

## Legacy route-extraction grammar (current implementation snapshot)

The current extractor still supports deterministic route-extraction patterns
from the earlier service-focused milestone. These examples describe current
implementation residue, not the product boundary.

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

## Unsupported route patterns + diagnostics codes

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

cd examples/generic-simple-cli  # default neutral reference fixture
pnpm install
pnpm run build:go
```

Output:

- `examples/generic-simple-cli/dist-go/main.go`

Scaffold-oriented compatibility sample (Fastify-shaped) is available at:

- `examples/fastify-scaffold-real/src/app.ts`
- `examples/fastify-scaffold-real/src/routes/*`

Framework-agnostic simple CLI workspace sample is available at:

- `examples/generic-simple-cli/src/index.ts`

Compatibility-track framework samples (optional references, non-default) are available at:

- `examples/COMPAT_TRACK.md`
- `examples/fastify-scaffold-real/src/app.ts`
- `examples/hono-scaffold-real/src/index.ts`

## CLI behavior

- `tsgodown` (no subcommand) runs the compiler build flow (`build`).
- Supported subcommands: `build`, `check`, `report`, `stages`.
- The removed `compiler` transitional command is no longer accepted.

## Runtime contracts (404/405/Allow)

Generated Go runtime keeps HTTP mismatch behavior deterministic:

- **404 Not Found**: no route matched by path
- **405 Method Not Allowed**: path matched but HTTP method is unsupported
- **Allow header on 405**: includes allowed methods for the matched path (comma-separated)

This contract is verified in emitter/CLI tests and in smoke workflows.

## Diagnostics/Fix guide pointers

When build output is incomplete, start here:

- Failure triage playbook: [`docs/operations/FAILURE_TRIAGE_PLAYBOOK.md`](docs/operations/FAILURE_TRIAGE_PLAYBOOK.md)
- M1 release gate and acceptance criteria: [`docs/specs/M1_RELEASE_GATE.md`](docs/specs/M1_RELEASE_GATE.md)
- Testing strategy and boundaries: [`docs/specs/TESTING_STRATEGY.md`](docs/specs/TESTING_STRATEGY.md)
- Canonical compiler-mode spec lock (supported subset / out-of-scope / fail-closed): [`docs/specs/COMPILER_MODE_CONTRACTS.md`](docs/specs/COMPILER_MODE_CONTRACTS.md)

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
- `pnpm run docs:scaffold:sync`
- `./scripts/smoke-m1.sh`

## License

MIT
