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
- Guardrail: runtime path dependency/import checks for legacy analyzer regression

## Required checks per PR/turn
- `pnpm run guard:runtime`
- `npm run build` (or `pnpm run build`)
- `npm run test` (or `pnpm run test`)
- Rust enabled 변경 시:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

## Runtime regression guard
- Script: `scripts/guard-no-legacy-ts-analyzer.mjs`
- Fails when runtime packages (`cli`, `core`, `pipeline`) either:
  - depend on `@tsgodown/analyzer`, or
  - import `@tsgodown/analyzer` from runtime source.

## Failure handling
- 테스트 실패 시 기능 보고 금지
- 실패 원인/재현 커맨드/해결 계획 3종 세트로 보고
