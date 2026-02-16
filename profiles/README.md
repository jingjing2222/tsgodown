# Profiles (Thin Adapter Only)

profiles는 SSoT가 아닙니다.

- 역할: 입력 프레임워크 코드를 `IR_SPEC` 형태로 변환하는 얇은 어댑터
- 금지: profile 내부에서 임의의 변환 정책/런타임 정책 결정
- 모든 판정: `CAPABILITY_MATRIX`를 통해 수행

즉, profile은 parser adapter이며, 컴파일 정책의 주인이 아닙니다.
