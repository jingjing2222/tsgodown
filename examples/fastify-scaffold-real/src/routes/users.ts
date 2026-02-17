import type { FastifyPluginAsync } from "fastify";

type UserReply = { id: string } | { ok: true };

const userRoutes: FastifyPluginAsync = async (app) => {
  app.post<{ Reply: UserReply }>("/", async (_request, reply) => {
    reply.code(201);
    return { id: "u1" };
  });

  app.patch<{ Params: { id: string }; Reply: UserReply }>("/:id", async () => {
    return { ok: true };
  });

  app.delete<{ Params: { id: string }; Reply: UserReply }>("/:id", async () => {
    return { ok: true };
  });
};

export default userRoutes;
