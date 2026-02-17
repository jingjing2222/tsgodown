import type { FastifyPluginAsync } from "fastify";

const userRoutes: FastifyPluginAsync = async (app) => {
  app.post("/", async (_request, reply) => {
    reply.code(201);
    return { id: "u1" };
  });

  app.patch("/:id", async () => {
    return { ok: true };
  });

  app.delete("/:id", async () => {
    return { ok: true };
  });
};

export default userRoutes;
