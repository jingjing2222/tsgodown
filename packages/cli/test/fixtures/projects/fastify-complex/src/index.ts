<<<<<<< HEAD
const app = {
  get: (_path: string, _handler: () => unknown) => undefined,
  post: (_path: string, _handler: () => unknown) => undefined,
  patch: (_path: string, _handler: () => unknown) => undefined,
  delete: (_path: string, _handler: () => unknown) => undefined,
};

const health = () => ({ ok: true });
const createUser = () => ({ id: "u1" });
const updateUser = () => ({ ok: true });
const removeUser = () => ({ ok: true });

app.get("/health", health);
app.post("/users", createUser);
app.patch("/users/:id", updateUser);
app.delete("/users/:id", removeUser);
=======
import Fastify from "fastify";

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
