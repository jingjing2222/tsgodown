const handler = () => ({ ok: true });
const prefix = "/api";

app.get(`${prefix}/users`, handler);
app.route({ method: "GET", url: `${prefix}/v1`, handler });
app.route({ method: ["POST"][0], url: "/ok", handler });
app.route({ method: "GET", url: "/inline", handler: () => ({ ok: true }) });
