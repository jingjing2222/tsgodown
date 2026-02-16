# IR_SPEC (SSoT)

`tsgodown`의 단일 진실원천(SSoT)은 이 IR 스펙입니다.

## 원칙
- 프레임워크 이름(Fastify/Nest/Express)은 IR에 직접 저장하지 않는다.
- IR은 **의미(semantics)** 만 표현한다.
- Go 변환 가능/불가능 판정은 `Capability Matrix`에서만 수행한다.

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
