# Testing Strategy (TDD First)

## Non-negotiable rule
모든 기능 구현은 **테스트 먼저(Test First)** 원칙을 따른다.

## Architecture guardrails (M4)
- Rust core is the **only** runtime analysis/build engine.
- TypeScript runtime code is orchestration/UI only.
- **No fallback policy:** runtime path must not fall back to legacy TS analyzer on Rust failures.

## Workflow
1. 실패 테스트 작성
2. 최소 구현으로 테스트 통과
3. 리팩터링
4. 회귀 테스트 추가

## Test layers
- Unit: 패키지 단위 순수 로직 (`packages/*/test`)
- Integration: rust adapter contract + pipeline orchestration
- E2E: 실제 예제 프로젝트 변환 후 CLI/build contract 검증

## M1 acceptance scenario (Fastify -> Go compile success path)
M1 수용 기준은 아래 단일 성공 경로를 명확히 고정한다.

## analyzer-rust boundary contract (M1)
- `packages/analyzer-rust/tests/contract_parity_regression.rs`를 analyzer-rust SSoT 계약 고정 테스트로 유지한다.
- 지원 경계(supported extraction shape)와 비지원 경계(unsupported shape)는 fixture 기반으로 고정한다.
- 비지원 경계는 **DiagnosticIR.code 매핑까지 포함해** 회귀 방지한다.
  - 예: `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`, `ANALYZER_UNSUPPORTED_INLINE_HANDLER`, `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD` 등
- analyzer-rust는 capability policy 진단(`CAPABILITY_*`)을 emit하지 않는다.

1. 입력: Fastify scaffold TypeScript 엔트리(`src/index.ts`)
2. 실행: `runPipeline` 또는 CLI `build`를 Rust adapter 경유로 실행
3. 산출: `dist-go/main.go` 생성 확인
4. 가독성 고정 assertion:
   - Go scaffold shape (`package main`, `func main()`, `GET /health` route binding)
   - Go toolchain 존재 시 `go mod init` + `go build ./...` 성공

주의: 이 시나리오 검증은 acceptance naming/assertion clarity 범위에 한정하며,
`analyzer-rust` / `emitter-go` 내부 구현 세부사항은 테스트 대상에서 제외한다.

## Required checks per PR/turn
- `npm run build` (or `pnpm run build`)
- `npm run test` (or `pnpm run test`)
- Rust enabled 변경 시:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

## Failure handling
- 테스트 실패 시 기능 보고 금지
- 실패 원인/재현 커맨드/해결 계획 3종 세트로 보고
