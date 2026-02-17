import Fastify from "fastify";

<<<<<<< HEAD
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
=======
const app = Fastify();

function healthHandler() {
  return { ok: true };
}

function createUserHandler() {
  return { id: "u1" };
}

function updateUserHandler() {
  return { ok: true };
}

function deleteUserHandler() {
  return { ok: true };
}

app.get("/health", healthHandler);
app.post("/users", createUserHandler);
app.put("/users/:id", updateUserHandler);
app.delete("/users/:id", deleteUserHandler);
>>>>>>> 848e2ce (feat(devx): one-command fastify-complex build-run-verify flow)
