const app = {
  register: (_plugin: unknown, _opts?: unknown) => undefined,
};

app.register(factoryPlugin(), { prefix: "/v1" });
app.register(missingPlugin, { prefix: "/v2" });
