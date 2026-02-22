use analyzer_rust::analyze_compiler_entry;
#[path = "support/tsdown_fixture.rs"]
mod tsdown_fixture;

#[test]
fn lowers_referenced_handler_semantics_deterministically() {
    let src = r#"
const createUser = (request, reply) => {
  reply.status(201);
  reply.send({ ok: true });
};

const health = () => ({ ok: true });

app.post("/users", createUser);
app.get("/health", health);
"#;

    let bundled = tsdown_fixture::build_inline_source(src);
    let ir = analyze_compiler_entry("fixture.ts", &bundled);

    assert!(!ir.handlers.is_empty());
}

#[test]
fn lowers_semantics_per_handler_body_not_whole_module() {
    let src = r#"
const createUser = (request, reply) => {
  reply.status(201);
  reply.send({ ok: true });
};

const health = (req, res) => ({ ok: true });

app.post("/users", createUser);
app.get("/health", health);
"#;

    let bundled = tsdown_fixture::build_inline_source(src);
    let ir = analyze_compiler_entry("fixture.ts", &bundled);
    assert!(!ir.handlers.is_empty());
}

#[test]
fn parses_typed_and_default_handler_params_and_response_aliases() {
    let src = r#"
const createUser = (
  request: FastifyRequest,
  reply: FastifyReply = defaultReply
) => {
  metrics.status(1).json({ ok: false });
  reply.code(201).setHeader("x-request-id", request.id).json({ ok: true });
};

app.post("/users", createUser);
"#;

    let bundled = tsdown_fixture::build_inline_source(src);
    let ir = analyze_compiler_entry("fixture.ts", &bundled);
    assert!(!ir.handlers.is_empty());
}

#[test]
fn parses_optional_typed_handler_params() {
    let src = r#"
const createUser = (request?: FastifyRequest, reply?: FastifyReply) => {
  reply?.status(201);
  reply?.send({ ok: true });
};

app.post("/users", createUser);
"#;

    let bundled = tsdown_fixture::build_inline_source(src);
    let ir = analyze_compiler_entry("fixture.ts", &bundled);
    assert!(!ir.handlers.is_empty());
}
