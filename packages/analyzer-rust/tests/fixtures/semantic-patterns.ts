const createUser = (request, reply) => {
  reply.status(201);
  reply.header("x-request-id", request.id);
  reply.send({ ok: true });
};

app.post("/users", createUser);
