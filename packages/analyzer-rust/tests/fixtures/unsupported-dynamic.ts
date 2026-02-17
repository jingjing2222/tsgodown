const handler = () => ({ ok: true });
const dynamicPath = "/users/:id";

import("./lazy");
app.get(dynamicPath, handler);
app.post("/inline", async () => ({ ok: true }));
app.route({ method: ["GET"], url: "/x", handler });
