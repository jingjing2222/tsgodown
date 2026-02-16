# Testing Strategy (TDD First)

## Non-negotiable rule
모든 기능 구현은 **테스트 먼저(Test First)** 원칙을 따른다.

## Workflow
1. 실패 테스트 작성
2. 최소 구현으로 테스트 통과
3. 리팩터링
4. 회귀 테스트 추가

## Test layers
- Unit: 패키지 단위 순수 로직 (`packages/*/test`)
- Integration: `pipeline` 연결 (artifact -> IR -> capability -> emit)
- E2E: 실제 예제 프로젝트 변환 후 Go 실행/응답 검증

## Required checks per PR/turn
- `npm run build`
- `npm run test`
- 변경된 capability 관련 시 `node-compat` 테스트 필수

## Failure handling
- 테스트 실패 시 기능 보고 금지
- 실패 원인/재현 커맨드/해결 계획 3종 세트로 보고
