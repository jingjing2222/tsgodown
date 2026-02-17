import Fastify from "fastify";

const fastify = Fastify();

function health(_req: unknown, reply: { send: (value: unknown) => void }) {
  reply.send({ ok: true });
}

function createUser(
  _req: unknown,
  reply: { code: (status: number) => { send: (value: unknown) => void } },
) {
  reply.code(201).send({ id: "u1" });
}

function updateUser(_req: unknown, reply: { send: (value: unknown) => void }) {
  reply.send({ ok: true });
}

function removeUser(_req: unknown, reply: { send: (value: unknown) => void }) {
  reply.send({ ok: true });
}

fastify.get("/health", health);
fastify.post("/users", createUser);
fastify.patch("/users/:id", updateUser);
fastify.delete("/users/:id", removeUser);

export { health, createUser, updateUser, removeUser };
