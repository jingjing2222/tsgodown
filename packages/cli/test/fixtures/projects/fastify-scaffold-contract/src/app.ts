import Fastify from "fastify";
import healthRoutes from "./routes/health";
import userRoutes from "./routes/users";

export function buildApp() {
  const app = Fastify();
  app.register(healthRoutes);
  app.register(userRoutes, { prefix: "/users" });
  return app;
}
