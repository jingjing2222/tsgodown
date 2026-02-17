import type { FastifyPluginAsync } from "fastify";

const userRoutes: FastifyPluginAsync = async (app) => {
  app.post("/", async (_request, reply) => {
    reply.code(201);
    return { id: "u1" };
  });
};

export default userRoutes;
