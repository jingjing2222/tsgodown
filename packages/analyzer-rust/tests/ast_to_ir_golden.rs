use std::{fs, path::PathBuf};

use analyzer_rust::{analyze_compiler_entry, JsExprIR, JsStmtIR, JsValueIR, ProgramIR};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn render_ir(ir: &ProgramIR) -> String {
    let mut out = String::new();

    out.push_str("modules:\n");
    for module in &ir.modules {
        out.push_str(&format!(
            "  - id={} source_path={}\n",
            module.id, module.source_path
        ));
        out.push_str("    exports:\n");
        for export in &module.exports {
            out.push_str(&format!("      - {}\n", export));
        }
        out.push_str("    imports:\n");
        for import in &module.imports {
            let bindings = if import.bindings.is_empty() {
                "[]".to_string()
            } else {
                format!(
                    "[{}]",
                    import
                        .bindings
                        .iter()
                        .map(|binding| format!(
                            "{}:{}:{}",
                            binding.local,
                            binding.imported.as_deref().unwrap_or("<none>"),
                            binding.kind
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            out.push_str(&format!(
                "      - spec={} kind={} resolved={} bindings={}\n",
                import.spec,
                import.kind,
                import.resolved.as_deref().unwrap_or("<none>"),
                bindings,
            ));
        }
        out.push_str("    executable:\n");
        if let Some(executable) = &module.executable {
            for stmt in &executable.stmts {
                out.push_str(&format!("      - {}\n", render_js_stmt(stmt)));
            }
        }
    }

    out.push_str("routes:\n");
    for route in &ir.routes {
        out.push_str(&format!(
            "  - method={} path={} handler_ref={}\n",
            route.method, route.path, route.handler_ref
        ));
    }

    out.push_str("handlers:\n");
    for handler in &ir.handlers {
        let params = if handler.params.is_empty() {
            "0".to_string()
        } else {
            let joined = handler
                .params
                .iter()
                .map(|p| format!("{}:{}", p.name, p.role))
                .collect::<Vec<_>>()
                .join(",");
            format!("[{joined}]")
        };
        let semantics = if let Some(semantics) = &handler.semantics {
            format!(
                "mode={} req={} res={} status={} body={} headers={} json={}",
                semantics.response_mode,
                semantics.request_param.as_deref().unwrap_or("<none>"),
                semantics.response_param.as_deref().unwrap_or("<none>"),
                semantics.uses_status,
                semantics.uses_body,
                semantics.uses_headers,
                semantics.uses_json,
            )
        } else {
            "none".to_string()
        };

        out.push_str(&format!(
            "  - id={} async={} params={} semantics={}\n",
            handler.id, handler.r#async, params, semantics
        ));
    }

    out.push_str("diagnostics:\n");
    for diag in &ir.diagnostics {
        out.push_str(&format!(
            "  - level={} code={} message={} source={}\n",
            diag.level,
            diag.code,
            diag.message,
            diag.source
                .as_ref()
                .map(|s| s.file.as_str())
                .unwrap_or("<none>"),
        ));
    }

    out
}

fn render_js_stmt(stmt: &JsStmtIR) -> String {
    match stmt {
        JsStmtIR::Expr(expr) => format!("expr {}", render_js_expr(expr)),
        JsStmtIR::FunctionDecl {
            name,
            params,
            r#async,
            generator,
            body,
            ..
        } => format!(
            "function {} async={}{} params=[{}] body=[{}]",
            name,
            r#async,
            if *generator { " generator=true" } else { "" },
            params.join(","),
            body.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::ClassDecl {
            name,
            super_class,
            methods,
        } => format!(
            "class {} extends={} methods=[{}]",
            name,
            super_class
                .as_ref()
                .map(render_js_expr)
                .unwrap_or_else(|| "<none>".to_string()),
            render_class_methods(methods)
        ),
        JsStmtIR::If {
            test,
            consequent,
            alternate,
        } => format!(
            "if {} then=[{}] else=[{}]",
            render_js_expr(test),
            consequent
                .iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; "),
            alternate
                .iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::For {
            init,
            test,
            update,
            body,
        } => format!(
            "for init=[{}] test={} update={} body=[{}]",
            init.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; "),
            test.as_ref()
                .map(render_js_expr)
                .unwrap_or_else(|| "<none>".to_string()),
            update
                .as_ref()
                .map(render_js_expr)
                .unwrap_or_else(|| "<none>".to_string()),
            body.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::ForOf { left, right, body } => format!(
            "for-of {} in {} body=[{}]",
            left,
            render_js_expr(right),
            body.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::While { test, body } => format!(
            "while {} body=[{}]",
            render_js_expr(test),
            body.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::Switch {
            discriminant,
            cases,
        } => format!(
            "switch {} cases=[{}]",
            render_js_expr(discriminant),
            cases
                .iter()
                .map(|case| format!(
                    "{} => [{}]",
                    case.test
                        .as_ref()
                        .map(render_js_expr)
                        .unwrap_or_else(|| "default".to_string()),
                    case.consequent
                        .iter()
                        .map(render_js_stmt)
                        .collect::<Vec<_>>()
                        .join("; ")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
        } => format!(
            "try body=[{}] catch={} body=[{}] finally=[{}]",
            body.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; "),
            catch_param.as_deref().unwrap_or("<none>"),
            catch_body
                .iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; "),
            finally_body
                .iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::Label { label, body } => format!(
            "label {} [{}]",
            label,
            body.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsStmtIR::Break(label) => format!("break {}", label.as_deref().unwrap_or("<none>")),
        JsStmtIR::Continue(label) => format!("continue {}", label.as_deref().unwrap_or("<none>")),
        JsStmtIR::Return(Some(expr)) => format!("return {}", render_js_expr(expr)),
        JsStmtIR::Return(None) => "return".to_string(),
        JsStmtIR::Throw(expr) => format!("throw {}", render_js_expr(expr)),
        JsStmtIR::Yield {
            value: Some(expr),
            delegate,
        } => {
            if *delegate {
                format!("yield* {}", render_js_expr(expr))
            } else {
                format!("yield {}", render_js_expr(expr))
            }
        }
        JsStmtIR::Yield { value: None, .. } => "yield".to_string(),
        JsStmtIR::VarDecl { name, init } => format!(
            "var {} = {}",
            name,
            init.as_ref()
                .map(render_js_expr)
                .unwrap_or_else(|| "<none>".to_string())
        ),
    }
}

fn render_js_expr(expr: &JsExprIR) -> String {
    match expr {
        JsExprIR::Value(value) => render_js_value(value),
        JsExprIR::Ident(name) => format!("ident({name})"),
        JsExprIR::This => "this".to_string(),
        JsExprIR::Array(items) => format!(
            "array([{}])",
            items
                .iter()
                .map(render_js_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        JsExprIR::ArraySpread(items) => format!(
            "array-spread([{}])",
            items
                .iter()
                .map(|item| format!(
                    "{}{}",
                    if item.spread { "..." } else { "" },
                    render_js_expr(&item.value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        JsExprIR::Object(props) => format!(
            "object({{{}}})",
            props
                .iter()
                .map(|prop| format!("{}: {}", prop.key, render_js_expr(&prop.value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        JsExprIR::ObjectRest { object, excluded } => format!(
            "object-rest({}, [{}])",
            render_js_expr(object),
            excluded.join(",")
        ),
        JsExprIR::Function {
            params,
            r#async,
            generator,
            body,
            ..
        } => format!(
            "function-expr async={}{} params=[{}] body=[{}]",
            r#async,
            if *generator { " generator=true" } else { "" },
            params.join(","),
            body.iter()
                .map(render_js_stmt)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        JsExprIR::Class {
            super_class,
            methods,
        } => format!(
            "class-expr extends={} methods=[{}]",
            super_class
                .as_ref()
                .map(|expr| render_js_expr(expr))
                .unwrap_or_else(|| "<none>".to_string()),
            render_class_methods(methods)
        ),
        JsExprIR::Unary { op, arg } => format!("unary({}, {})", op, render_js_expr(arg)),
        JsExprIR::Await { arg } => format!("await({})", render_js_expr(arg)),
        JsExprIR::Binary { op, left, right } => format!(
            "binary({}, {}, {})",
            op,
            render_js_expr(left),
            render_js_expr(right)
        ),
        JsExprIR::Conditional {
            test,
            consequent,
            alternate,
        } => format!(
            "conditional({}, {}, {})",
            render_js_expr(test),
            render_js_expr(consequent),
            render_js_expr(alternate)
        ),
        JsExprIR::Assign { op, left, right } => format!(
            "assign({}, {}, {})",
            op,
            render_js_expr(left),
            render_js_expr(right)
        ),
        JsExprIR::Update { op, arg, prefix } => {
            format!("update({}, {}, {})", op, render_js_expr(arg), prefix)
        }
        JsExprIR::Call {
            callee,
            args,
            optional,
        } => format!(
            "{}call({}, [{}])",
            if *optional { "optional-" } else { "" },
            render_js_expr(callee),
            args.iter()
                .map(render_js_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        JsExprIR::Spread { arg } => format!("spread({})", render_js_expr(arg)),
        JsExprIR::New { callee, args } => format!(
            "new({}, [{}])",
            render_js_expr(callee),
            args.iter()
                .map(render_js_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        JsExprIR::Member {
            object,
            property,
            computed,
            optional,
        } => match computed {
            Some(computed) => format!(
                "{}member({}, [{}])",
                if *optional { "optional-" } else { "" },
                render_js_expr(object),
                render_js_expr(computed)
            ),
            None => format!(
                "{}member({}, {})",
                if *optional { "optional-" } else { "" },
                render_js_expr(object),
                property
            ),
        },
        JsExprIR::Template { quasis, exprs } => format!(
            "template([{}], [{}])",
            quasis.join(","),
            exprs
                .iter()
                .map(render_js_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        JsExprIR::Sequence(exprs) => format!(
            "sequence([{}])",
            exprs
                .iter()
                .map(render_js_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_class_methods(methods: &[analyzer_rust::JsClassMethodIR]) -> String {
    methods
        .iter()
        .map(|method| {
            format!(
                "{} kind={} static={} async={} params=[{}] body=[{}]",
                method.name,
                method.kind,
                method.is_static,
                method.r#async,
                method.params.join(","),
                method
                    .body
                    .iter()
                    .map(render_js_stmt)
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn render_js_value(value: &JsValueIR) -> String {
    match value {
        JsValueIR::Undefined => "undefined".to_string(),
        JsValueIR::Null => "null".to_string(),
        JsValueIR::Bool(value) => format!("bool({value})"),
        JsValueIR::Number(value) => format!("number({value})"),
        JsValueIR::String(value) => format!("string({value})"),
        JsValueIR::BigInt(value) => format!("bigint({value})"),
        JsValueIR::RegExp { pattern, flags } => format!("regexp({pattern}/{flags})"),
    }
}

fn assert_fixture(fixture_name: &str, golden_name: &str) {
    let source = fs::read_to_string(fixture_path(fixture_name)).unwrap();
    let ir = analyze_compiler_entry(fixture_name, &source);
    let actual = render_ir(&ir);
    let expected = fs::read_to_string(fixture_path(golden_name)).unwrap();
    assert_eq!(actual, expected, "golden drift for fixture={fixture_name}");
}

#[test]
fn supported_shorthand_routes_are_lowered_deterministically() {
    assert_fixture("supported-shorthand.ts", "supported-shorthand.golden.txt");
}

#[test]
fn route_object_literal_is_lowered_deterministically() {
    assert_fixture("route-object-literal.ts", "route-object-literal.golden.txt");
}

#[test]
fn route_object_reference_is_lowered_deterministically() {
    assert_fixture(
        "route-object-reference.ts",
        "route-object-reference.golden.txt",
    );
}

#[test]
fn unsupported_patterns_emit_deterministic_diagnostics() {
    assert_fixture("unsupported-dynamic.ts", "unsupported-dynamic.golden.txt");
}

#[test]
fn semantic_patterns_are_lowered_deterministically() {
    assert_fixture("semantic-patterns.ts", "semantic-patterns.golden.txt");
}

#[test]
fn template_literal_paths_keep_static_literals_and_reject_interpolated_paths() {
    assert_fixture(
        "template-literal-paths.ts",
        "template-literal-paths.golden.txt",
    );
}

#[test]
fn unsupported_register_boundaries_emit_spec_mapped_diagnostics() {
    assert_fixture(
        "unsupported-register-boundaries.ts",
        "unsupported-register-boundaries.golden.txt",
    );
}

#[test]
fn conditional_routes_emit_unsupported_diagnostic() {
    assert_fixture("conditional-route.ts", "conditional-route.golden.txt");
}

#[test]
fn single_line_conditional_routes_emit_unsupported_diagnostic() {
    assert_fixture(
        "conditional-route-single-line.ts",
        "conditional-route-single-line.golden.txt",
    );
}

#[test]
fn conditional_map_delete_does_not_emit_route_diagnostic() {
    let source = r#"
const map = new Map();
if (map.size > 1) {
  map.delete("");
}
"#;
    let ir = analyze_compiler_entry("not-a-route.js", source);

    assert!(
        ir.diagnostics
            .iter()
            .all(|diag| diag.code != "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE"),
        "Map.delete(string) must not be mistaken for DELETE route registration"
    );
}

#[test]
fn executable_control_flow_is_lowered_deterministically() {
    let source = r#"
function scan(items) {
  let total = 0;
  WHILE: while (total < 2) {
    total++;
    continue WHILE;
  }
  for (let i = 0; i < 3; i++) {
    switch (i) {
      case 0:
        continue;
      case 1:
        break;
      default:
        total += i;
    }
  }
  try {
    return total;
  } catch (err) {
    throw err;
  } finally {
    total++;
  }
}
"#;
    let ir = analyze_compiler_entry("control-flow.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("for init=[var i = number(0)]"));
    assert!(rendered.contains("label WHILE [while"));
    assert!(rendered.contains("while binary(<, ident(total), number(2))"));
    assert!(rendered.contains("continue WHILE"));
    assert!(rendered.contains("switch ident(i)"));
    assert!(rendered.contains("continue <none>"));
    assert!(rendered.contains("break <none>"));
    assert!(rendered.contains("try body=[return ident(total)] catch=err"));
    assert!(rendered.contains("update(++, ident(total), false)"));
}

#[test]
fn executable_classes_and_new_expressions_are_lowered_deterministically() {
    let source = r#"
class Cache extends BaseCache {
  constructor(limit) {
    this.limit = limit;
  }
  get(key) {
    return this.store.get(key);
  }
  static create(limit) {
    return new Cache(limit);
  }
}
const cache = new Cache(2);
"#;
    let ir = analyze_compiler_entry("classes.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("class Cache extends=ident(BaseCache)"));
    assert!(rendered.contains("constructor kind=constructor"));
    assert!(rendered.contains("get kind=method"));
    assert!(rendered.contains("create kind=method static=true"));
    assert!(rendered.contains("new(ident(Cache), [number(2)])"));
}

#[test]
fn executable_private_class_members_are_lowered_as_semantic_properties() {
    let source = r#"
class Counter {
  #value = 1;
  #bump(step = 1) {
    this.#value += step;
  }
  static #seed() {
    return 4;
  }
  constructor(start = 2) {
    this.#value = start;
  }
  next() {
    this.#bump();
    return this.#value;
  }
  static read() {
    return this.#seed();
  }
  collect(...items) {
    return items.length;
  }
}
"#;
    let ir = analyze_compiler_entry("private-class.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("expr assign(=, member(this, #value), number(1))"));
    assert!(
        rendered.contains("#bump kind=method static=false async=false params=[__tsgodown_param_0]")
    );
    assert!(rendered.contains(
        "var step = conditional(binary(===, ident(__tsgodown_param_0), undefined), number(1), ident(__tsgodown_param_0))"
    ));
    assert!(rendered.contains("#seed kind=method static=true async=false params=[]"));
    assert!(rendered.contains("call(member(this, #bump), [])"));
    assert!(rendered.contains("return call(member(this, #seed), [])"));
    let class_methods = ir.modules[0]
        .executable
        .as_ref()
        .expect("executable")
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            JsStmtIR::ClassDecl { methods, .. } => Some(methods),
            _ => None,
        })
        .expect("class methods");
    assert!(class_methods
        .iter()
        .any(|method| method.name == "collect" && method.rest_param.as_deref() == Some("items")));
}

#[test]
fn executable_expression_forms_are_lowered_deterministically() {
    let source = r#"
const capture = () => this.done;
async function render(name, count) {
  const pattern = /node/gi;
  const big = 10n;
  const label = `hello ${name}`;
  const escaped = `^a\\s+${name}$`;
  const value = count > 0 ? count : 0;
  const result = await load(label);
  return (result, this.done(value));
}
"#;
    let ir = analyze_compiler_entry("expressions.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("regexp(node/gi)"));
    assert!(rendered.contains("bigint(10n)"));
    assert!(rendered.contains("template([hello ,], [ident(name)])"));
    assert!(rendered.contains(r"template([^a\s+,$], [ident(name)])"));
    assert!(ir.modules[0]
        .executable
        .as_ref()
        .expect("executable")
        .stmts
        .iter()
        .any(|stmt| matches!(
            stmt,
            JsStmtIR::VarDecl {
                name,
                init: Some(JsExprIR::Function {
                    lexical_this: true,
                    ..
                }),
            } if name == "capture"
        )));
    assert!(rendered.contains("conditional(binary(>, ident(count), number(0))"));
    assert!(rendered.contains("await(call(ident(load), [ident(label)]))"));
    assert!(
        rendered.contains("sequence([ident(result), call(member(this, done), [ident(value)])])")
    );
}

#[test]
fn executable_array_spread_is_lowered_deterministically() {
    let source = r#"
function copy(items) {
  return join(0, ...items, 9, ...tail);
}
"#;
    let ir = analyze_compiler_entry("array-spread.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains(
        "call(ident(join), [number(0), spread(ident(items)), number(9), spread(ident(tail))])"
    ));
}

#[test]
fn executable_optional_member_is_lowered_deterministically() {
    let source = r#"
function errorName(result) {
  return result.error?.name ?? null;
}
"#;
    let ir = analyze_compiler_entry("optional-member.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered
        .contains("return binary(??, optional-member(member(ident(result), error), name), null)"));
}

#[test]
fn executable_array_destructuring_arrow_params_are_lowered() {
    let source = r#"
const out = rows.map(([path, pattern, options = {}]) => ({ path, pattern, options }));
"#;
    let ir = analyze_compiler_entry("array-destructure-arrow.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("function-expr async=false params=[__tsgodown_param_0]"));
    assert!(rendered.contains("var __tsgodown_destructure_0 = ident(__tsgodown_param_0)"));
    assert!(rendered.contains("var path = member(ident(__tsgodown_destructure_0), 0)"));
    assert!(rendered.contains(
        "var options = conditional(binary(===, member(ident(__tsgodown_destructure_0), 2), undefined), object({}), member(ident(__tsgodown_destructure_0), 2))"
    ));
}

#[test]
fn executable_object_destructuring_default_params_are_lowered() {
    let source = r#"
const out = rows.map(({ dot = false } = {}) => dot);
"#;
    let ir = analyze_compiler_entry("object-destructure-default-param.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("function-expr async=false params=[__tsgodown_param_0]"));
    assert!(rendered.contains(
        "var __tsgodown_destructure_0 = conditional(binary(===, ident(__tsgodown_param_0), undefined), object({}), ident(__tsgodown_param_0))"
    ));
    assert!(rendered.contains(
        "var dot = conditional(binary(===, member(ident(__tsgodown_destructure_0), dot), undefined), bool(false), member(ident(__tsgodown_destructure_0), dot))"
    ));
}

#[test]
fn executable_object_rest_destructuring_params_are_lowered() {
    let source = r#"
const out = rows.map(({ known = 1, ...rest }) => rest);
"#;
    let ir = analyze_compiler_entry("object-rest-destructure-param.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("var known = conditional(binary(===, member(ident(__tsgodown_destructure_0), known), undefined), number(1), member(ident(__tsgodown_destructure_0), known))"));
    assert!(rendered.contains("var rest = object-rest(ident(__tsgodown_destructure_0), [known])"));
}

#[test]
fn executable_object_method_props_are_lowered() {
    let source = r#"
const handlers = {
  native() {},
  transform({ value }) { return value }
};
"#;
    let ir = analyze_compiler_entry("object-method-props.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("native: function-expr async=false params=[] body=[]"));
    assert!(rendered.contains("transform: function-expr async=false params=[__tsgodown_param_0]"));
    assert!(rendered.contains("var value = member(ident(__tsgodown_destructure_0), value)"));
}

#[test]
fn export_object_destructuring_decl_names_are_collected() {
    let source = r#"
const source = { onExit() {}, load() {}, unload() {} };
export const { onExit, load: start, ...rest } = source;
"#;
    let ir = analyze_compiler_entry("export-object-destructure.js", source);

    assert!(ir.modules[0].exports.contains(&"onExit".to_string()));
    assert!(ir.modules[0].exports.contains(&"start".to_string()));
    assert!(ir.modules[0].exports.contains(&"rest".to_string()));
}

#[test]
fn executable_for_of_destructuring_head_is_lowered() {
    let source = r#"
for (const [chunk] of rows) {
  sink(chunk);
}
"#;
    let ir = analyze_compiler_entry("for-of-destructure.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("for-of __tsgodown_forof_value in ident(rows) body=[var __tsgodown_destructure_0 = ident(__tsgodown_forof_value); var chunk = member(ident(__tsgodown_destructure_0), 0); expr call(ident(sink), [ident(chunk)])]"));
}

#[test]
fn executable_yield_delegate_is_preserved() {
    let source = r#"
function * flatten(chunks) {
  for (const chunk of chunks) {
    yield * transform(chunk);
  }
}
"#;
    let ir = analyze_compiler_entry("yield-delegate.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("yield* call(ident(transform), [ident(chunk)])"));
}

#[test]
fn static_builtin_dynamic_import_is_tracked_without_unsupported_diagnostic() {
    let source = r#"
import('node:diagnostics_channel').then((dc) => dc.channel('x')).catch(() => {});
"#;
    let ir = analyze_compiler_entry("static-builtin-dynamic-import.js", source);

    assert!(
        ir.diagnostics
            .iter()
            .all(|diag| diag.code != "DYNAMIC_IMPORT_DETECTED"),
        "static node: builtin dynamic import should not block corpus graph analysis"
    );
    assert_eq!(ir.modules[0].imports.len(), 1);
    assert_eq!(ir.modules[0].imports[0].spec, "node:diagnostics_channel");
    assert_eq!(ir.modules[0].imports[0].kind, "dynamic");
}

#[test]
fn import_bindings_are_lowered_for_executable_codegen() {
    let source = r#"
import parser from "yargs-parser";
import * as qs from "qs";
import { parse as parseYaml, dump } from "js-yaml";
const fs = require("fs-extra");
const { execa } = require("execa");
"#;
    let ir = analyze_compiler_entry("imports.js", source);
    let imports = &ir.modules[0].imports;

    assert!(imports.iter().any(|import| import.spec == "yargs-parser"
        && import
            .bindings
            .iter()
            .any(|binding| binding.local == "parser"
                && binding.imported.as_deref() == Some("default")
                && binding.kind == "default")));
    assert!(imports.iter().any(|import| import.spec == "qs"
        && import.bindings.iter().any(|binding| binding.local == "qs"
            && binding.imported.as_deref() == Some("*")
            && binding.kind == "namespace")));
    assert!(imports.iter().any(|import| import.spec == "js-yaml"
        && import
            .bindings
            .iter()
            .any(|binding| binding.local == "parseYaml"
                && binding.imported.as_deref() == Some("parse")
                && binding.kind == "named")
        && import.bindings.iter().any(|binding| binding.local == "dump"
            && binding.imported.as_deref() == Some("dump")
            && binding.kind == "named")));
    assert!(imports.iter().any(|import| import.spec == "fs-extra"
        && import.bindings.iter().any(|binding| binding.local == "fs"
            && binding.imported.is_none()
            && binding.kind == "require")));
    assert!(imports.iter().any(|import| import.spec == "execa"
        && import
            .bindings
            .iter()
            .any(|binding| binding.local == "execa"
                && binding.imported.as_deref() == Some("execa")
                && binding.kind == "destructure")));
}

#[test]
fn executable_export_default_and_named_aliases_are_bound() {
    let source = r#"
const callable = () => "ok";
callable.extra = true;
export default callable;
export { callable as "module.exports" };
"#;
    let ir = analyze_compiler_entry("export-default.js", source);
    let rendered = render_ir(&ir);

    assert!(rendered.contains("var default = ident(callable)"));
    assert!(rendered.contains("var module.exports = ident(callable)"));
}
