use analyzer_rust::analyze_compiler_entry;

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

    let ir = analyze_compiler_entry("fixture.ts", src);

    assert_eq!(ir.handlers.len(), 2);

    let create_user = ir.handlers.iter().find(|h| h.id == "createUser").unwrap();
    assert_eq!(create_user.params.len(), 2);
    assert_eq!(create_user.params[0].name, "request");
    assert_eq!(create_user.params[0].role, "request");
    assert_eq!(create_user.params[1].name, "reply");
    assert_eq!(create_user.params[1].role, "response");
    assert_eq!(
        create_user
            .semantics
            .as_ref()
            .map(|s| s.response_mode.as_str()),
        Some("response-object")
    );

    let health = ir.handlers.iter().find(|h| h.id == "health").unwrap();
    assert!(health.params.is_empty());
    assert_eq!(
        health.semantics.as_ref().map(|s| s.response_mode.as_str()),
        Some("return")
    );
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

    let ir = analyze_compiler_entry("fixture.ts", src);
    let health = ir.handlers.iter().find(|h| h.id == "health").unwrap();
    let semantics = health.semantics.as_ref().unwrap();

    assert!(!semantics.uses_status);
    assert!(!semantics.uses_body);
    assert!(!semantics.uses_json);
    assert!(!semantics.uses_headers);
}

#[test]
fn parses_typed_and_default_handler_params_and_fastify_code_alias() {
    let src = r#"
const createUser = (
  request: FastifyRequest,
  reply: FastifyReply = defaultReply
) => {
  reply.code(201).send({ ok: true });
};

app.post("/users", createUser);
"#;

    let ir = analyze_compiler_entry("fixture.ts", src);
    let create_user = ir.handlers.iter().find(|h| h.id == "createUser").unwrap();
    let semantics = create_user.semantics.as_ref().unwrap();

    assert_eq!(create_user.params.len(), 2);
    assert_eq!(create_user.params[0].name, "request");
    assert_eq!(create_user.params[0].role, "request");
    assert_eq!(create_user.params[1].name, "reply");
    assert_eq!(create_user.params[1].role, "response");

    assert!(semantics.uses_status);
    assert!(semantics.uses_body);
}
