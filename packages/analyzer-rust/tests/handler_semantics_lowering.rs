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
fn does_not_leak_response_object_semantics_between_handlers() {
    let src = r#"
const withReply = (request, reply) => {
  reply.status(204);
  reply.send();
};

const returnsValue = () => {
  return { ok: true };
};

app.get("/with-reply", withReply);
app.get("/returns", returnsValue);
"#;

    let ir = analyze_compiler_entry("fixture.ts", src);

    let with_reply = ir.handlers.iter().find(|h| h.id == "withReply").unwrap();
    assert_eq!(
        with_reply
            .semantics
            .as_ref()
            .map(|s| s.response_mode.as_str()),
        Some("response-object")
    );

    let returns_value = ir.handlers.iter().find(|h| h.id == "returnsValue").unwrap();
    assert_eq!(
        returns_value
            .semantics
            .as_ref()
            .map(|s| s.response_mode.as_str()),
        Some("return")
    );
    assert_eq!(
        returns_value.semantics.as_ref().map(|s| s.uses_status),
        Some(false)
    );
    assert_eq!(
        returns_value.semantics.as_ref().map(|s| s.uses_body),
        Some(false)
    );
}

#[test]
fn lowers_next_callback_response_mode_when_no_response_param_is_present() {
    let src = r#"
const withoutResponse = (request, next) => {
  next();
};

app.get("/mw", withoutResponse);
"#;

    let ir = analyze_compiler_entry("fixture.ts", src);

    let without_response = ir
        .handlers
        .iter()
        .find(|h| h.id == "withoutResponse")
        .unwrap();
    assert_eq!(
        without_response
            .semantics
            .as_ref()
            .map(|s| s.response_mode.as_str()),
        Some("next-callback")
    );
}
