export type RegisterableApp = {
  get: (path: string, handler: () => unknown) => void;
};

export function registerUserRoutes(app: RegisterableApp) {
  app.get("/users", () => ({ ok: true }));
  app.get("/users/:id", () => ({ ok: true }));
}
