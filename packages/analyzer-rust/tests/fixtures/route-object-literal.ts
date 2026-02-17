const health = () => ({ ok: true });

app.route({
  method: "GET",
  url: "/health",
  handler: health,
});
