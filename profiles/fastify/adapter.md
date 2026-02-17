# Fastify Adapter

Only converts input patterns into IR:
- fastify.get/post/put/delete/patch
- fastify.route({...})
- register(plugin) structure tracing

Note:
- supportability decisions are delegated to the capability matrix
- Go emission logic is forbidden here (must be handled only in emitter-go)
