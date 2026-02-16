import Fastify from "fastify";

const fastify = Fastify();

function healthHandler(
  _req: unknown,
  reply: { send: (value: unknown) => void },
) {
  reply.send({ ok: true });
}

function usersHandler(
  _req: unknown,
  reply: { send: (value: unknown) => void },
) {
  reply.send([{ id: 1, name: "kim" }]);
}

fastify.get("/health", healthHandler);
fastify.get("/users", usersHandler);

export { healthHandler, usersHandler };
