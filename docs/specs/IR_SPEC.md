# IR_SPEC (SSoT)

`tsgodown`의 단일 진실원천(SSoT)은 이 IR 스펙입니다.

## 원칙
- 프레임워크 이름(Fastify/Nest/Express)은 IR에 직접 저장하지 않는다.
- IR은 **의미(semantics)** 만 표현한다.
- Go 변환 가능/불가능 판정은 `Capability Matrix`에서만 수행한다.
- 런타임 분석/IR 추출의 단일 실행 주체는 Rust core다.
- TS 런타임 경로는 IR 생성을 직접 수행하지 않으며, Rust 실패 시 TS 분석기로 fallback 하지 않는다.

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
`packages/analyzer-rust`는 M1에서 **extract/diagnose only** 범위를 유지한다.

### Supported boundary (현재 추출 보장 범위)
- Shorthand route: `fastify.<method>('literal-path', namedHandler)`
  - method: `GET|POST|PUT|DELETE|PATCH`
  - path: 문자열 리터럴
  - handler: 식별자 기반 named reference
- Route object: `fastify.route({ method, url|path, handler })`
  - object: inline object literal
  - method: 문자열 + `GET|POST|PUT|DELETE|PATCH`
  - `url` 또는 `path`: 문자열 리터럴
  - `handler`: named reference
- Register/plugin:
  - inline plugin callback 또는 same-file named plugin reference
  - `register(..., { prefix: '/v1' })` prefix 누적 반영

### Unsupported boundary → DiagnosticIR.code mapping
- `DYNAMIC_IMPORT_DETECTED`
  - trigger: `import(...)` 동적 import 사용
- `ANALYZER_UNRESOLVED_PLUGIN`
  - trigger: `register(pluginRef, ...)`에서 same-file plugin 정의를 해석하지 못함
- `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`
  - trigger: inline callback/same-file named reference가 아닌 register callback 패턴
- `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`
  - trigger: route path(`url`/`path` 포함)가 문자열 리터럴이 아님
- `ANALYZER_UNSUPPORTED_INLINE_HANDLER`
  - trigger: handler가 named reference가 아님(예: inline function)
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`
  - trigger: `fastify.route(...)`가 inline object literal 형태가 아님
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`
  - trigger: route object의 `method` 누락/비문자열/allowlist 외 값

### SSoT boundary
- analyzer-rust는 capability/policy 판정을 수행하지 않는다.
- `CAPABILITY_*` 계열 코드는 analyzer-rust에서 emit하지 않는다.
- 관련 계약은 `packages/analyzer-rust/tests/contract_parity_regression.rs`에서 고정한다.

## Data sources
- tsdown 산출물(JS bundle)
- source map
- d.ts
- manifest.json

## Rule
새 기능은 반드시
1) IR 노드 변경/확장
2) Capability Matrix 항목 추가
를 먼저 수행한 뒤 어댑터/에미터 구현을 진행한다.
