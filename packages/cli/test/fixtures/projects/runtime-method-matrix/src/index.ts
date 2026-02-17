import Fastify from "fastify";

const app = Fastify();

function health() {
  return { ok: true };
}

function createUser() {
  return { id: "u1" };
}

function updateUser() {
  return { ok: true };
}

app.get("/health", health);
app.post("/users", createUser);
app.put("/users/:id", updateUser);
