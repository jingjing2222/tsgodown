import util from "node:util";

export const health = () => ({ ok: true });
const users = () => [{ id: "u1" }];

app.get("/users", users);
app.get("/health", health);
