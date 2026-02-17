const fastify = {
  get: (_path: string, _handler: () => unknown) => undefined,
  post: (_path: string, _handler: () => unknown) => undefined,
  patch: (_path: string, _handler: () => unknown) => undefined,
  delete: (_path: string, _handler: () => unknown) => undefined,
};

const health = () => ({ ok: true });
const createUser = () => ({ id: "u1" });
const updateUser = () => ({ ok: true });
const removeUser = () => ({ ok: true });

fastify.get("/health", health);
fastify.post("/users", createUser);
fastify.patch("/users/:id", updateUser);
fastify.delete("/users/:id", removeUser);
