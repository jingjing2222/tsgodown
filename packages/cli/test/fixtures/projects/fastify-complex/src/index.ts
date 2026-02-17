const app = {
  get: (_path: string, _handler: () => unknown) => undefined,
  post: (_path: string, _handler: () => unknown) => undefined,
  patch: (_path: string, _handler: () => unknown) => undefined,
  delete: (_path: string, _handler: () => unknown) => undefined,
};

const health = () => ({ ok: true });
const createUser = () => ({ id: "u1" });
const updateUser = () => ({ ok: true });
const removeUser = () => ({ ok: true });

app.get("/health", health);
app.post("/users", createUser);
app.patch("/users/:id", updateUser);
app.delete("/users/:id", removeUser);
