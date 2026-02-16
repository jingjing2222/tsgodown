import { type RegisterableApp, registerUserRoutes } from "./users";

const fastify: {
  register: (plugin: (app: RegisterableApp) => void, opts?: unknown) => void;
} = {
  register: (_plugin, _opts) => undefined,
};
fastify.register(registerUserRoutes, { prefix: "/v1" });
