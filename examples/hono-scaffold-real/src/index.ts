import { Hono } from "hono";

const app = new Hono();

app.get("/health", (c) => c.text("ok"));
app.get("/users", (c) => c.json([{ id: 1, name: "kim" }]));

export default app;
