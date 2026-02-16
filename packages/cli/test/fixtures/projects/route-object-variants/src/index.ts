const app: {
  route: (def: {
    method: string | string[];
    url: string;
    handler: () => unknown;
  }) => void;
  get: (path: string, handler: () => unknown) => void;
} = {
  route: (_def) => undefined,
  get: (_p, _h) => undefined,
};

app.route({
  method: "GET",
  url: "/route-object",
  handler: () => ({ ok: true }),
});
app.route({
  method: ["POST", "PUT"],
  url: "/multi-method",
  handler: () => ({ ok: true }),
});
app.get("/direct", () => ({ ok: true }));
