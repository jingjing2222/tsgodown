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

#[derive(Clone, Debug)]
struct UnsupportedFixtureCase {
    code: &'static str,
    bad_fixture: &'static str,
    fixed_fixture: &'static str,
    expected_fixed_routes: Vec<(&'static str, &'static str, &'static str)>,
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
    assert!(!messages.iter().any(|m| m.contains("non-reference handler")));
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
fn extracts_complex_realworld_routes_with_stable_handler_refs() {
    let (file, src) = fixture("complex-fastify-realworld.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/tenants/:tenantId/users", "userHandlers.listUsers"),
            ("POST", "/tenants/:tenantId/users", "userHandlers.create"),
            (
                "PUT",
                "/tenants/:tenantId/users/:userId",
                "userHandlers.update",
            ),
            ("DELETE", "/tenants/:tenantId/users/:userId", "removeUser"),
            (
                "PATCH",
                "/tenants/:tenantId/users/:userId/profile/:profileId",
                "controller.detail",
            ),
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
                "userHandlers.listUsers",
                vec![("req", "request"), ("reply", "response")],
                true,
            ),
            (
                "userHandlers.create",
                vec![("request", "request"), ("response", "response")],
                false,
            ),
            ("userHandlers.update", vec![("req", "request")], true),
            (
                "removeUser",
                vec![("req", "request"), ("reply", "response")],
                false,
            ),
            ("controller.detail", vec![("request", "request")], true),
        ]
    );

    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_deterministic_boundary_diagnostics_for_unsupported_route_object_patterns() {
    let (file, src) = fixture("unsupported-route-object-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/from-config", "listFromConfig"),
            ("GET", "/from-variable-method", "listUsers"),
            ("GET", "/dynamic", "listAudits"),
            ("POST", "/inline", "__inline__route__b86c00febff9d0f3"),
        ]
    );

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
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
                "unsupported route object method in fastify.route({...}): missing string 'method' or non-empty string array. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                "warn",
                file.as_str(),
            ),
            (
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
                "unsupported route object method in fastify.route({...}): 'OPTIONS'. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                "warn",
                file.as_str(),
            ),
        ]
    );
}

#[test]
fn supports_route_object_method_array_extraction() {
    let (file, src) = fixture("route-object-method-array-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("PUT", "/things/:id", "replaceThing"),
            ("PATCH", "/things/:id", "replaceThing"),
        ]
    );
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn supports_route_object_method_variants_and_register_wrapper_plugin() {
    let (file_route, src_route) = fixture("route-object-method-variants-fastify.fixture.txt");
    let route_ir = analyze_fastify_entry(&file_route, &src_route);

    assert_eq!(
        route_ir
            .routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("PATCH", "/users/:id", "updateUser"),
            ("PUT", "/users/:id", "removeUser"),
            ("DELETE", "/users/:id", "removeUser"),
        ]
    );
    assert!(route_ir.diagnostics.is_empty());

    let (file_register, src_register) = fixture("register-wrapper-fastify.fixture.txt");
    let register_ir = analyze_fastify_entry(&file_register, &src_register);
    assert_eq!(
        register_ir
            .routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![("GET", "/v1/users", "listUsers")]
    );
    assert!(register_ir.diagnostics.is_empty());
}

#[test]
fn supports_deterministic_inline_handler_synthesis() {
    let (file, src) = fixture("inline-handler-synth-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/health", "__inline__get__a38f039b84b03a0a"),
            ("POST", "/users", "__inline__route__73ddf17fee278333"),
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
                "__inline__get__a38f039b84b03a0a",
                vec![("req", "request"), ("reply", "response")],
                true,
            ),
            (
                "__inline__route__73ddf17fee278333",
                vec![("request", "request")],
                false,
            ),
        ]
    );
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_deterministic_diagnostics_for_non_constant_if_blocks() {
    let (file, src) = fixture("conditional-routes-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![("GET", "/always", "alwaysOn")]
    );

    assert_eq!(
        ir.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.level.as_str(), d.message.as_str()))
            .collect::<Vec<_>>(),
        vec![(
            "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE",
            "warn",
            "conditional route registration in if-block is unsupported for deterministic extraction (fastify.get(...)). Move route declaration to top-level plugin scope.",
        )]
    );

    assert!(ir
        .diagnostics
        .iter()
        .all(|d| d.source.as_ref().map(|s| s.file.as_str()) == Some(file.as_str())));
}

#[test]
fn extracts_routes_from_compile_time_constant_if_branches_only() {
    let (file, src) = fixture("conditional-constant-routes-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    assert_eq!(
        ir.routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/always", "alwaysOn"),
            ("GET", "/gated-on", "gatedOn"),
            ("POST", "/fallback", "fallback"),
        ]
    );

    assert!(ir.diagnostics.is_empty());

    assert!(ir
        .routes
        .iter()
        .all(|r| r.path != "/gated-off" && r.path != "/never"));
}

#[test]
fn supports_static_template_literal_paths_and_rejects_dynamic_templates() {
    let (supported_file, supported_src) = fixture("template-literal-static-fastify.fixture.txt");
    let supported_ir = analyze_fastify_entry(&supported_file, &supported_src);

    assert_eq!(
        supported_ir
            .routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/users/:id", "getUsers"),
            ("GET", "/accounts/:id", "getAccounts"),
        ]
    );
    assert!(supported_ir.diagnostics.is_empty());

    let (rejected_file, rejected_src) = fixture("template-literal-dynamic-fastify.fixture.txt");
    let rejected_ir = analyze_fastify_entry(&rejected_file, &rejected_src);

    assert!(rejected_ir.routes.is_empty());
    assert_eq!(
        rejected_ir
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>(),
        vec![(
            "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
            "unsupported dynamic path in fastify.get(...). Use string literal path (e.g. '/users/:id') for IR extraction.",
        )]
    );
    assert!(rejected_ir
        .diagnostics
        .iter()
        .all(|d| d.source.as_ref().map(|s| s.file.as_str()) == Some(rejected_file.as_str())));
}

#[test]
fn fixture_matrix_for_fastify_unsupported_diagnostics_bad_and_fixed_pairs() {
    let cases = vec![
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE",
            bad_fixture: "fastify-unsupported-conditional-route.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-conditional-route.fixed.fixture.txt",
            expected_fixed_routes: vec![("GET", "/health", "health")],
        },
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK",
            bad_fixture: "fastify-unsupported-register-callback.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-register-callback.fixed.fixture.txt",
            expected_fixed_routes: vec![("GET", "/users", "listUsers")],
        },
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
            bad_fixture: "fastify-unsupported-dynamic-path.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-dynamic-path.fixed.fixture.txt",
            expected_fixed_routes: vec![("GET", "/users/:id", "getUser")],
        },
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            bad_fixture: "fastify-unsupported-inline-handler.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-inline-handler.fixed.fixture.txt",
            expected_fixed_routes: vec![("POST", "/users", "createUser")],
        },
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE",
            bad_fixture: "fastify-unsupported-route-object-shape.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-route-object-shape.fixed.fixture.txt",
            expected_fixed_routes: vec![("GET", "/users", "listUsers")],
        },
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
            bad_fixture: "fastify-unsupported-route-object-method.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-route-object-method.fixed.fixture.txt",
            expected_fixed_routes: vec![("GET", "/users", "listUsers")],
        },
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
            bad_fixture: "fastify-unsupported-route-object-path.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-route-object-path.fixed.fixture.txt",
            expected_fixed_routes: vec![("GET", "/users/:id", "getUser")],
        },
        UnsupportedFixtureCase {
            code: "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            bad_fixture: "fastify-unsupported-route-object-handler.bad.fixture.txt",
            fixed_fixture: "fastify-unsupported-route-object-handler.fixed.fixture.txt",
            expected_fixed_routes: vec![("POST", "/users", "createUser")],
        },
    ];

    for case in cases {
        let (bad_file, bad_src) = fixture(case.bad_fixture);
        let bad_ir = analyze_fastify_entry(&bad_file, &bad_src);

        let mut bad_codes = bad_ir
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>();
        bad_codes.sort();

        assert_eq!(
            bad_codes,
            vec![case.code],
            "unexpected diagnostics for bad fixture: {}",
            case.bad_fixture,
        );

        let (fixed_file, fixed_src) = fixture(case.fixed_fixture);
        let fixed_ir = analyze_fastify_entry(&fixed_file, &fixed_src);

        assert_eq!(
            fixed_ir
                .routes
                .iter()
                .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
                .collect::<Vec<_>>(),
            case.expected_fixed_routes,
            "route extraction drifted for fixed fixture: {}",
            case.fixed_fixture,
        );

        assert!(
            fixed_ir.diagnostics.is_empty(),
            "fixed fixture should not emit diagnostics: {}",
            case.fixed_fixture,
        );
    }
}
