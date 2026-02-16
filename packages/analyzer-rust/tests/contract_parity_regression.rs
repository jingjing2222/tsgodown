use std::fs;
use std::path::PathBuf;

use analyzer_rust::analyze_fastify_entry;
use pretty_assertions::assert_eq;

#[derive(Debug)]
struct ContractFixture {
    name: &'static str,
    expected_routes: Vec<(&'static str, &'static str, &'static str)>,
    expected_diag_codes: Vec<&'static str>,
}

fn fixture(name: &str) -> (String, String) {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    let src = fs::read_to_string(&path).expect("fixture must exist");
    (path.to_string_lossy().to_string(), src)
}

fn contract_fixtures() -> Vec<ContractFixture> {
    vec![
        ContractFixture {
            name: "basic-fastify.fixture.txt",
            expected_routes: vec![
                ("GET", "/users", "listUsers"),
                ("POST", "/users", "createUser"),
                ("PATCH", "/users/:id", "updateUser"),
            ],
            expected_diag_codes: vec![],
        },
        ContractFixture {
            name: "nested-register-route-object-fastify.fixture.txt",
            expected_routes: vec![
                ("GET", "/api/users", "listApiUsers"),
                ("GET", "/api/v2/users/:id", "listApiUserById"),
                ("GET", "/private/devices", "listPrivateDevices"),
                ("POST", "/private/devices", "createPrivateDevice"),
                ("GET", "/internal/metrics", "listInternalMetrics"),
            ],
            expected_diag_codes: vec![],
        },
        ContractFixture {
            name: "register-prefix-fastify.fixture.txt",
            expected_routes: vec![
                ("GET", "/v1/users", "listV1Users"),
                ("GET", "/v1/users/:id", "showV1User"),
                ("GET", "/v1/admin/accounts", "listAccounts"),
            ],
            expected_diag_codes: vec![],
        },
        ContractFixture {
            name: "unsupported-fastify.fixture.txt",
            expected_routes: vec![],
            expected_diag_codes: vec![
                "ANALYZER_UNRESOLVED_PLUGIN",
                "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
                "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            ],
        },
    ]
}

#[test]
fn rust_contract_parity_routes_and_diagnostics_are_stable() {
    for case in contract_fixtures() {
        let (file, src) = fixture(case.name);
        let ir = analyze_fastify_entry(&file, &src);

        let actual_routes = ir
            .routes
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            actual_routes, case.expected_routes,
            "route contract drifted for fixture: {}",
            case.name
        );

        let mut actual_diag_codes = ir
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>();
        actual_diag_codes.sort();

        let mut expected_diag_codes = case.expected_diag_codes;
        expected_diag_codes.sort();

        assert_eq!(
            actual_diag_codes, expected_diag_codes,
            "diagnostic contract drifted for fixture: {}",
            case.name
        );

        for diagnostic in &ir.diagnostics {
            assert_eq!(
                diagnostic.level, "warn",
                "diagnostic level drifted for fixture: {} code={}",
                case.name, diagnostic.code
            );
            assert_eq!(
                diagnostic
                    .source
                    .as_ref()
                    .map(|source| source.file.as_str()),
                Some(file.as_str()),
                "diagnostic source.file drifted for fixture: {} code={}",
                case.name,
                diagnostic.code
            );
        }

        if case.name == "unsupported-fastify.fixture.txt" {
            let mut actual_diag_details = ir
                .diagnostics
                .iter()
                .map(|d| {
                    (
                        d.code.as_str(),
                        d.message.as_str(),
                        d.level.as_str(),
                        d.source
                            .as_ref()
                            .map(|source| source.file.as_str())
                            .unwrap_or("<missing-source-file>"),
                    )
                })
                .collect::<Vec<_>>();
            actual_diag_details.sort();

            let mut expected_diag_details = vec![
                (
                    "ANALYZER_UNRESOLVED_PLUGIN",
                    "register plugin 'externalPlugin' could not be resolved in current file. Ensure plugin is declared in the same file or use an inline callback.",
                    "warn",
                    file.as_str(),
                ),
                (
                    "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
                    "unsupported dynamic path in fastify.get(...). Use string literal path (e.g. '/users/:id') for IR extraction.",
                    "warn",
                    file.as_str(),
                ),
                (
                    "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
                    "unsupported non-reference handler in fastify.post('/inline', handler). Extract handler to a named function and pass its identifier.",
                    "warn",
                    file.as_str(),
                ),
            ];
            expected_diag_details.sort();

            assert_eq!(
                actual_diag_details, expected_diag_details,
                "diagnostic code/message/level/source.file contract drifted for fixture: {}",
                case.name
            );
        }

        assert_eq!(
            ir.modules.len(),
            0,
            "SSoT boundary violated (modules should remain empty in extract/diagnose phase): {}",
            case.name
        );
    }
}

#[test]
fn rust_contract_parity_ssot_boundary_extract_and_diagnose_only() {
    let (file, src) = fixture("ssot-boundary-fastify.fixture.txt");
    let ir = analyze_fastify_entry(&file, &src);

    let actual_routes = ir
        .routes
        .iter()
        .map(|r| (r.method.as_str(), r.path.as_str(), r.handler_ref.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual_routes,
        vec![
            ("GET", "/policy/allow", "allowAdminOnly"),
            ("POST", "/policy/deny", "denyGuest"),
        ],
        "route extraction drifted for SSoT boundary fixture"
    );

    let mut diagnostic_codes = ir
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect::<Vec<_>>();
    diagnostic_codes.sort();

    assert!(
        !diagnostic_codes.contains(&"CAPABILITY_UNMET"),
        "SSoT boundary drift: analyzer emitted policy/capability diagnostic CAPABILITY_UNMET"
    );

    assert!(
        !diagnostic_codes
            .iter()
            .any(|code| code.starts_with("CAPABILITY_")),
        "SSoT boundary drift: analyzer emitted policy/capability diagnostic prefix CAPABILITY_"
    );

    assert_eq!(
        ir.modules.len(),
        0,
        "SSoT boundary violated (modules should remain empty in extract/diagnose phase)"
    );
}
