const app = {
  get: (_path: string, _handler: () => unknown) => undefined,
};

const health = () => ({ ok: true });
const users = () => [{ id: "u1" }];

app.get("/health", health);
app.get("/users", users);
