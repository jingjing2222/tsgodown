const fastify = {
  get: (_path: string, _handler: () => unknown) => undefined,
  post: (_path: string, _handler: () => unknown) => undefined,
};

const handler = () => ({ ok: true });
const dynamicPath = "/users/:id";

fastify.get(dynamicPath, handler);
fastify.post("/inline", async () => ({ ok: true }));
