import Fastify from "fastify";

const app = Fastify();

function expHandler() {
  return { ok: true };
}

if (process.env.EXPERIMENTAL === "1") {
  app.get("/exp", expHandler);
}
