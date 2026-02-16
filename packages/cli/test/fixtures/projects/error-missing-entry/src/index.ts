const app: { get: (path: string, handler: () => unknown) => void } = {
  get: (_path, _handler) => undefined,
};
app.get("/health", () => ({ ok: true }));
