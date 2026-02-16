use std::fs;
use std::path::PathBuf;

use analyzer_rust::analyze_fastify_entry;
use pretty_assertions::assert_eq;

fn fixture(name: &str) -> (String, String) {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../analyzer/test/fixtures");
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
        .into_iter()
        .map(|d| d.code)
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
