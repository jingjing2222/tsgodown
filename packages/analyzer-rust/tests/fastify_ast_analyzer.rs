use std::fs;
use std::path::PathBuf;

use analyzer_rust::analyze_fastify_entry;
use pretty_assertions::assert_eq;

fn fixture(name: &str) -> (String, String) {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    let src = fs::read_to_string(&path).expect("fixture must exist");
    (path.to_string_lossy().to_string(), src)
}

#[test]
fn extracts_method_path_handler_from_shorthand_and_route_object() {
    let (file, src) = fixture("basic-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .into_iter()
            .map(|r| (r.method, r.path, r.handler_ref))
            .collect::<Vec<_>>(),
        vec![
            ("GET".into(), "/users".into(), "listUsers".into()),
            ("POST".into(), "/users".into(), "createUser".into()),
            ("PATCH".into(), "/users/:id".into(), "updateUser".into()),
        ]
    );
    assert_eq!(ir.diagnostics.len(), 0);
}

#[test]
fn applies_register_prefix_for_inline_and_named_plugins() {
    let (file, src) = fixture("register-prefix-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .into_iter()
            .map(|r| (r.method, r.path, r.handler_ref))
            .collect::<Vec<_>>(),
        vec![
            ("GET".into(), "/v1/users".into(), "listV1Users".into()),
            ("GET".into(), "/v1/users/:id".into(), "showV1User".into()),
            (
                "GET".into(),
                "/v1/admin/accounts".into(),
                "listAccounts".into(),
            ),
        ]
    );
    assert_eq!(ir.diagnostics.len(), 0);
}

#[test]
fn emits_explicit_diagnostics_for_unsupported_patterns() {
    let (file, src) = fixture("unsupported-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    let mut codes = ir
        .diagnostics
        .iter()
        .map(|d| d.code.clone())
        .collect::<Vec<_>>();
    codes.sort();
    assert_eq!(
        codes,
        vec![
            "ANALYZER_UNRESOLVED_PLUGIN".to_string(),
            "ANALYZER_UNSUPPORTED_DYNAMIC_PATH".to_string(),
            "ANALYZER_UNSUPPORTED_INLINE_HANDLER".to_string(),
        ]
    );

    let messages = ir
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|m| m.contains("dynamic path") && m.contains("Use string literal path")));
    assert!(messages.iter().any(|m| m.contains("non-reference handler")
        && m.contains("Extract handler to a named function")));
    assert!(messages
        .iter()
        .any(|m| m.contains("register plugin 'externalPlugin'")
            && m.contains("Ensure plugin is declared in the same file")));
}

#[test]
fn handles_nested_register_prefix_route_object_variants() {
    let (file, src) = fixture("nested-register-route-object-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .into_iter()
            .map(|r| (r.method, r.path, r.handler_ref))
            .collect::<Vec<_>>(),
        vec![
            ("GET".into(), "/api/users".into(), "listApiUsers".into()),
            (
                "GET".into(),
                "/api/v2/users/:id".into(),
                "listApiUserById".into(),
            ),
            (
                "GET".into(),
                "/private/devices".into(),
                "listPrivateDevices".into(),
            ),
            (
                "POST".into(),
                "/private/devices".into(),
                "createPrivateDevice".into(),
            ),
            (
                "GET".into(),
                "/internal/metrics".into(),
                "listInternalMetrics".into(),
            ),
        ]
    );
    assert_eq!(ir.diagnostics.len(), 0);
}

#[test]
fn handles_nested_route_schema_object() {
    let (file, src) = fixture("ast-nested-route-object-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .into_iter()
            .map(|r| (r.method, r.path, r.handler_ref))
            .collect::<Vec<_>>(),
        vec![(
            "GET".into(),
            "/users/advanced".into(),
            "listUsersAdvanced".into(),
        )]
    );
    assert_eq!(ir.diagnostics.len(), 0);
}

#[test]
fn keeps_ssot_boundary_extraction_and_diagnostics_only() {
    let (file, src) = fixture("ssot-boundary-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .into_iter()
            .map(|r| (r.method, r.path, r.handler_ref))
            .collect::<Vec<_>>(),
        vec![
            (
                "GET".into(),
                "/policy/allow".into(),
                "allowAdminOnly".into(),
            ),
            ("POST".into(), "/policy/deny".into(), "denyGuest".into(),),
        ]
    );

    let codes = ir
        .diagnostics
        .into_iter()
        .map(|d| d.code)
        .collect::<Vec<_>>();
    assert!(!codes.iter().any(|c| c == "CAPABILITY_UNMET"));
    assert!(!codes.iter().any(|c| c.starts_with("CAPABILITY_")));
}

#[test]
fn handles_single_param_arrow_plugins_and_handlers() {
    let (file, src) = fixture("single-param-arrow-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/api/v1/users", "listUsers"),
            ("PATCH", "/api/v1/users/:id", "updateUser"),
        ]
    );

    assert_eq!(
        ir.handlers
            .iter()
            .map(|h| {
                (
                    h.id.as_str(),
                    h.params
                        .iter()
                        .map(|p| (p.name.as_str(), p.role.as_str()))
                        .collect::<Vec<_>>(),
                    h.r#async,
                    h.semantics
                        .as_ref()
                        .map(|s| s.response_mode.as_str())
                        .unwrap_or("<missing>"),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("listUsers", vec![("req", "request")], true, "unknown",),
            ("updateUser", vec![("reply", "response")], false, "unknown",),
        ]
    );

    assert!(ir.diagnostics.is_empty());
}

#[test]
fn extracts_typed_async_handlers_with_try_catch_error_flow() {
    let (file, src) = fixture("typed-async-error-handling-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/users/:id", "getUser"),
            ("POST", "/users", "createUser"),
        ]
    );

    assert_eq!(
        ir.handlers
            .iter()
            .map(|h| {
                (
                    h.id.as_str(),
                    h.params
                        .iter()
                        .map(|p| (p.name.as_str(), p.role.as_str()))
                        .collect::<Vec<_>>(),
                    h.r#async,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                "getUser",
                vec![("req", "request"), ("reply", "response")],
                true,
            ),
            (
                "createUser",
                vec![("req", "request"), ("reply", "response")],
                true,
            ),
        ]
    );

    assert!(ir.diagnostics.is_empty());
}

#[test]
fn handles_chained_routes_nested_plugins_and_single_param_function_plugins() {
    let cases = vec![
        (
            "chaining-routes-fastify.fixture.txt",
            vec![
                ("GET", "/users", "listUsers"),
                ("POST", "/users", "createUser"),
                ("PATCH", "/users/:id", "updateUser"),
            ],
        ),
        (
            "nested-plugin-chaining-fastify.fixture.txt",
            vec![
                ("GET", "/api/users", "listApiUsers"),
                ("POST", "/api/users", "createApiUser"),
                ("GET", "/api/v2/users", "listNestedUsers"),
            ],
        ),
        (
            "single-param-function-plugin-fastify.fixture.txt",
            vec![
                ("GET", "/admin/audit/logs", "listAuditLogs"),
                ("POST", "/admin/audit/logs", "createAuditLog"),
            ],
        ),
        (
            "put-delete-chaining-fastify.fixture.txt",
            vec![
                ("PUT", "/users/:id", "replaceUser"),
                ("DELETE", "/users/:id", "removeUser"),
            ],
        ),
    ];

    for (name, expected_routes) in cases {
        let (file, src) = fixture(name);
        let ir = analyze_fastify_entry(&file, &src);

        assert_eq!(
            ir.routes
                .iter()
                .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
                .collect::<Vec<_>>(),
            expected_routes,
            "route extraction drifted for fixture: {}",
            name
        );
        assert!(
            ir.diagnostics.is_empty(),
            "unexpected diagnostics for fixture: {}",
            name
        );
    }
}

#[test]
fn extracts_handlers_from_object_literals_and_class_instances() {
    let (file, src) = fixture("class-object-literal-handlers-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/users", "userHandlers.list"),
            ("POST", "/users", "userHandlers.create"),
            ("GET", "/users/:id", "controller.detail"),
            ("DELETE", "/users/:id", "controller.remove"),
        ]
    );

    assert_eq!(
        ir.handlers
            .iter()
            .map(|h| {
                (
                    h.id.as_str(),
                    h.params
                        .iter()
                        .map(|p| (p.name.as_str(), p.role.as_str()))
                        .collect::<Vec<_>>(),
                    h.r#async,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                "userHandlers.list",
                vec![("req", "request"), ("reply", "response")],
                false,
            ),
            ("userHandlers.create", vec![("request", "request")], true),
            ("controller.detail", vec![("request", "request")], true),
            (
                "controller.remove",
                vec![("req", "request"), ("reply", "response")],
                false,
            ),
        ]
    );

    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_deterministic_boundary_diagnostics_for_unsupported_route_object_patterns() {
    let (file, src) = fixture("unsupported-route-object-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert!(ir.routes.is_empty());

    let actual = ir
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.code.as_str(),
                d.message.as_str(),
                d.level.as_str(),
                d.source
                    .as_ref()
                    .map(|s| s.file.as_str())
                    .unwrap_or("<missing-source-file>"),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE",
                "unsupported route object pattern in fastify.route(...). Provide an inline object literal (e.g. { method: 'GET', url: '/users', handler: listUsers }).",
                "warn",
                file.as_str(),
            ),
            (
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
                "unsupported route object method in fastify.route({...}): missing string 'method'. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                "warn",
                file.as_str(),
            ),
            (
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
                "unsupported route object method in fastify.route({...}): 'OPTIONS'. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                "warn",
                file.as_str(),
            ),
            (
                "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
                "unsupported route object path in fastify.route({...}). Provide string literal 'url' or 'path' (e.g. '/users/:id').",
                "warn",
                file.as_str(),
            ),
            (
                "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
                "unsupported route object handler in fastify.route({...}). Provide named handler reference in 'handler' field.",
                "warn",
                file.as_str(),
            ),
        ]
    );
}
