const health = () => ({ ok: true });

const routeDef = {
  method: "GET",
  url: "/health",
  handler: health,
};

app.route(routeDef);
