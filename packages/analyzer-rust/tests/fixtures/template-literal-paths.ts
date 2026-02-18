const health = () => ({ ok: true });
const users = () => [{ id: "u1" }];

// biome-ignore lint/style/noUnusedTemplateLiteral: fixture validates static template-literal route extraction.
app.get(`/health`, health);
app.get(`${prefix}/users`, users);
