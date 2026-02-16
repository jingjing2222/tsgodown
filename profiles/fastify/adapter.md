# Fastify Adapter

입력 패턴을 IR로 변환만 수행:
- fastify.get/post/put/delete/patch
- fastify.route({...})
- register(plugin) 구조 추적

주의:
- 대응 가능 여부 판단은 capability matrix에 위임
- Go emission 로직은 금지 (emitter-go에서만 처리)
