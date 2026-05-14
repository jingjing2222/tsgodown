use analyzer_rust::{
    DiagnosticIR, DiagnosticSourceIR, ExecutableModuleIR, HandlerIR, HandlerParamIR,
    HandlerSemanticsIR, ImportIR, JsExprIR, JsStmtIR, JsValueIR, ModuleIR, ProgramIR, RouteIR,
};

#[test]
fn program_ir_normalize_applies_v1_deterministic_ordering_contract() {
    let normalized = ProgramIR {
        modules: vec![
            ModuleIR {
                id: "z_module".to_string(),
                source_path: "src/z.ts".to_string(),
                exports: vec!["b".to_string(), "a".to_string()],
                imports: vec![
                    ImportIR {
                        spec: "zod".to_string(),
                        kind: "esm".to_string(),
                        resolved: None,
                    },
                    ImportIR {
                        spec: "@scope/a".to_string(),
                        kind: "cjs".to_string(),
                        resolved: Some("/abs/a".to_string()),
                    },
                ],
                executable: Some(ExecutableModuleIR {
                    stmts: vec![JsStmtIR::VarDecl {
                        name: "answer".to_string(),
                        init: Some(JsExprIR::Value(JsValueIR::Number("42".to_string()))),
                    }],
                }),
            },
            ModuleIR {
                id: "a_module".to_string(),
                source_path: "src/a.ts".to_string(),
                exports: vec!["index".to_string()],
                imports: vec![],
                executable: Some(ExecutableModuleIR { stmts: vec![] }),
            },
        ],
        routes: vec![
            RouteIR {
                method: "POST".to_string(),
                path: "/users".to_string(),
                handler_ref: "createUser".to_string(),
            },
            RouteIR {
                method: "GET".to_string(),
                path: "/health".to_string(),
                handler_ref: "health".to_string(),
            },
        ],
        handlers: vec![
            HandlerIR {
                id: "z_handler".to_string(),
                params: vec![HandlerParamIR {
                    name: "req".to_string(),
                    role: "request".to_string(),
                }],
                r#async: true,
                semantics: Some(HandlerSemanticsIR {
                    response_mode: "return".to_string(),
                    request_param: Some("req".to_string()),
                    response_param: None,
                    uses_status: false,
                    uses_body: false,
                    uses_headers: false,
                    uses_json: false,
                }),
            },
            HandlerIR {
                id: "a_handler".to_string(),
                params: vec![],
                r#async: false,
                semantics: None,
            },
        ],
        diagnostics: vec![
            DiagnosticIR {
                level: "warn".to_string(),
                code: "W_B".to_string(),
                message: "second".to_string(),
                source: None,
            },
            DiagnosticIR {
                level: "error".to_string(),
                code: "E_A".to_string(),
                message: "first".to_string(),
                source: Some(DiagnosticSourceIR {
                    file: "src/index.ts".to_string(),
                    line: Some(9),
                    column: Some(2),
                    via_source_map: Some(true),
                }),
            },
        ],
    }
    .normalize();

    assert_eq!(normalized.modules[0].id, "a_module");
    assert_eq!(normalized.modules[1].exports, vec!["a", "b"]);
    assert_eq!(normalized.modules[1].imports[0].spec, "@scope/a");

    assert_eq!(normalized.routes[0].path, "/health");
    assert_eq!(normalized.handlers[0].id, "a_handler");

    assert_eq!(normalized.diagnostics[0].level, "error");
    assert_eq!(
        normalized.diagnostics[0]
            .source
            .as_ref()
            .expect("source should be present")
            .line,
        Some(9)
    );
}

#[test]
fn program_ir_normalize_is_idempotent() {
    let once = ProgramIR {
        modules: vec![],
        routes: vec![RouteIR {
            method: "GET".to_string(),
            path: "/health".to_string(),
            handler_ref: "health".to_string(),
        }],
        handlers: vec![],
        diagnostics: vec![],
    }
    .normalize();

    let twice = once.clone().normalize();

    assert_eq!(once, twice);
}
