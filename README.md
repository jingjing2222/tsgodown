# tsgodown

tsdown 스타일 인터페이스를 유지하면서 Fastify 프로젝트를 Go 스캐폴드로 내리는 실험용 컴파일러 뼈대입니다.

## 핵심 목표
- `defineConfig` + `tsgodown.config.ts` 경험을 tsdown과 유사하게 유지
- 내부는 Fastify 분석 -> IR -> Go emitter 분리

## 현재 상태 (v0.1 skeleton)
- config 로딩/정규화
- Fastify route 감지(기초)
- Go main.go scaffold 생성
- SSoT 초안: `docs/specs/IR_SPEC.md`, `docs/specs/CAPABILITY_MATRIX.md`

## 실행
```bash
cd /Users/kimhyeongjeong/Desktop/code/tsgodown
npm install
npm run build
cd examples/fastify-min
node --import tsx ../../packages/cli/src/index.ts build
```

생성물: `examples/fastify-min/dist-go/main.go`
