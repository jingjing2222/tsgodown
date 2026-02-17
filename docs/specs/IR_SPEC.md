# IR_SPEC (SSoT)

This IR spec is the single source of truth (SSoT) for `tsgodown`.

## Principles
- Do not store framework names (Fastify/Nest/Express) directly in IR.
- IR expresses **semantics** only.
- Go convertibility decisions must be performed only in the `Capability Matrix`.
- Rust core is the single runtime executor for analysis/IR extraction.
- The TS runtime path does not generate IR directly and does not fall back to the TS analyzer when Rust fails.

## Core IR Nodes

### ProgramIR
```ts
interface ProgramIR {
  modules: ModuleIR[]
  routes: RouteIR[]
  handlers: HandlerIR[]
  diagnostics: DiagnosticIR[]
}
```

### ModuleIR
```ts
interface ModuleIR {
  id: string
  sourcePath: string
  exports: string[]
  imports: Array<{ spec: string; kind: 'esm' | 'cjs'; resolved?: string }>
}
```

### RouteIR
```ts
interface RouteIR {
  method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'
  path: string
  handlerRef: string
  middlewareRefs?: string[]
}
```

### HandlerIR
```ts
type HandlerResponseMode = 'return' | 'response-object' | 'next-callback' | 'unknown'

interface HandlerIR {
  id: string
  params: Array<{ name: string; role: 'request' | 'response' | 'next' | 'custom' }>
  bodyRef?: string
  async: boolean
  semantics?: {
    // pragmatic v1: response handling strategy hint for emitters
    responseMode: HandlerResponseMode
  }
}
```

### DiagnosticIR
```ts
interface DiagnosticIR {
  level: 'error' | 'warn' | 'info'
  code: string
  message: string
  source?: {
    file: string
    line?: number
    column?: number
    viaSourceMap?: boolean
  }
}
```

## analyzer-rust Fastify boundary (M1)
`packages/analyzer-rust` keeps an **extract/diagnose only** scope in M1.

### Supported boundary (currently guaranteed extraction range)
- Shorthand route: `fastify.<method>('literal-path', namedHandler)`
  - method: `GET|POST|PUT|DELETE|PATCH`
  - path: string literal
  - handler: identifier-based named reference
- Route object: `fastify.route({ method, url|path, handler })`
  - object: inline object literal
  - method: string + `GET|POST|PUT|DELETE|PATCH`
  - `url` or `path`: string literal
  - `handler`: named reference
- Register/plugin:
  - inline plugin callback or same-file named plugin reference
  - apply prefix accumulation for `register(..., { prefix: '/v1' })`

### Unsupported boundary → DiagnosticIR.code mapping
- `DYNAMIC_IMPORT_DETECTED`
  - trigger: use of dynamic `import(...)`
- `ANALYZER_UNRESOLVED_PLUGIN`
  - trigger: failed to resolve same-file plugin definition in `register(pluginRef, ...)`
- `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`
  - trigger: register callback pattern that is neither inline callback nor same-file named reference
- `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`
  - trigger: route path (including `url`/`path`) is not a string literal
- `ANALYZER_UNSUPPORTED_INLINE_HANDLER`
  - trigger: handler is not a named reference (e.g. inline function)
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`
  - trigger: `fastify.route(...)` is not an inline object literal
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`
  - trigger: route object `method` is missing / non-string / outside allowlist

### SSoT boundary
- analyzer-rust does not perform capability/policy decisions.
- analyzer-rust does not emit `CAPABILITY_*` family codes.
- The related contract is fixed in `packages/analyzer-rust/tests/contract_parity_regression.rs`.

## Data sources
- tsdown artifacts (JS bundle)
- source map
- d.ts
- manifest.json

## Rule
For every new feature, do this first:
1) change/extend IR nodes
2) add a Capability Matrix entry
and only then implement adapter/emitter changes.
