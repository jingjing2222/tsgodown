use std::collections::{BTreeMap, BTreeSet};

use crate::{
    DiagnosticIR, DiagnosticSourceIR, HandlerIR, HandlerParamIR, HandlerSemanticsIR, ImportIR,
    ModuleIR, ProgramIR, RouteIR,
};

const ALLOWED_METHODS: [&str; 5] = ["GET", "POST", "PUT", "DELETE", "PATCH"];

#[derive(Debug, Clone)]
struct HandlerDef {
    is_async: bool,
    params: Vec<HandlerParamIR>,
    semantics: HandlerSemanticsIR,
}

pub fn build_program_ir(file: &str, src: &str) -> ProgramIR {
    let mut diagnostics = Vec::new();

    let imports = collect_imports(src, &mut diagnostics, file);
    let exports = collect_exports(src);
    let handler_defs = collect_handler_defs(src);
    let plugin_defs = collect_plugin_defs(src);
    let route_object_defs = collect_route_object_defs(src);
    collect_conditional_route_diagnostics(src, &mut diagnostics, file);
    collect_class_private_element_diagnostics(src, &mut diagnostics, file);
    collect_class_extends_diagnostics(src, &mut diagnostics, file);
    collect_class_public_field_diagnostics(src, &mut diagnostics, file);
    collect_class_static_member_diagnostics(src, &mut diagnostics, file);

    let mut routes = Vec::new();
    let mut referenced_handlers = BTreeSet::new();
    for stmt in collect_statements(src) {
        if let Some(route) = parse_shorthand_route(stmt, &mut diagnostics, file) {
            referenced_handlers.insert(route.handler_ref.clone());
            routes.push(route);
            continue;
        }
        if let Some(route) = parse_route_object(stmt, &route_object_defs, &mut diagnostics, file) {
            referenced_handlers.insert(route.handler_ref.clone());
            routes.push(route);
            continue;
        }
        parse_register_boundary(stmt, &plugin_defs, &mut diagnostics, file);
    }

    routes.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.method.cmp(&b.method))
            .then_with(|| a.handler_ref.cmp(&b.handler_ref))
    });

    let mut handlers = referenced_handlers
        .into_iter()
        .map(|id| {
            if let Some(def) = handler_defs.get(&id) {
                HandlerIR {
                    r#async: def.is_async,
                    id,
                    params: def.params.clone(),
                    semantics: Some(def.semantics.clone()),
                }
            } else {
                HandlerIR {
                    r#async: false,
                    id,
                    params: vec![],
                    semantics: None,
                }
            }
        })
        .collect::<Vec<_>>();

    handlers.sort_by(|a, b| a.id.cmp(&b.id));

    sort_diagnostics(&mut diagnostics);

    ProgramIR {
        modules: vec![ModuleIR {
            id: file.to_string(),
            source_path: file.to_string(),
            exports,
            imports,
        }],
        routes,
        handlers,
        diagnostics,
    }
}

fn collect_statements(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_bracket = 0i32;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in src.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {}
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            ';' if depth_paren == 0 && depth_brace == 0 && depth_bracket == 0 => {
                out.push(src[start..=idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }

    if start < src.len() {
        let tail = src[start..].trim();
        if !tail.is_empty() {
            out.push(tail);
        }
    }
    out
}

fn collect_exports(src: &str) -> Vec<String> {
    let mut exports = BTreeSet::new();
    for line in src.lines() {
        let trimmed = line.trim();
        for prefix in [
            "export const ",
            "export function ",
            "export async function ",
            "export class ",
            "export default class ",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some(name) = take_identifier(rest) {
                    exports.insert(name.to_string());
                }
            }
        }
    }
    exports.into_iter().collect()
}

fn collect_imports(src: &str, diagnostics: &mut Vec<DiagnosticIR>, file: &str) -> Vec<ImportIR> {
    let mut imports = Vec::new();

    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.contains("import(") {
            diagnostics.push(diag(
                "error",
                "DYNAMIC_IMPORT_DETECTED",
                "dynamic import(...) is unsupported in compiler mode",
                file,
            ));
        }

        if !trimmed.starts_with("import ") {
            continue;
        }

        if let Some(spec) = extract_quoted(trimmed) {
            imports.push(ImportIR {
                spec: spec.to_string(),
                kind: "esm".to_string(),
                resolved: None,
            });
        }
    }

    imports.sort_by(|a, b| a.spec.cmp(&b.spec).then_with(|| a.kind.cmp(&b.kind)));
    imports
}

fn collect_conditional_route_diagnostics(
    src: &str,
    diagnostics: &mut Vec<DiagnosticIR>,
    file: &str,
) {
    let mut conditional_depth: i32 = 0;

    for line in src.lines() {
        let trimmed = line.trim();

        if conditional_depth == 0 && (trimmed.starts_with("if(") || trimmed.starts_with("if (")) {
            if has_route_invocation(trimmed) {
                diagnostics.push(diag(
                    "error",
                    "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE",
                    "conditional route registration is unsupported",
                    file,
                ));
                return;
            }

            conditional_depth = brace_delta(line).max(1);
        } else if conditional_depth > 0 {
            if has_route_invocation(trimmed) {
                diagnostics.push(diag(
                    "error",
                    "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE",
                    "conditional route registration is unsupported",
                    file,
                ));
                return;
            }
            conditional_depth += brace_delta(line);
            if conditional_depth <= 0 {
                conditional_depth = 0;
            }
        }
    }
}

fn collect_class_private_element_diagnostics(
    src: &str,
    diagnostics: &mut Vec<DiagnosticIR>,
    file: &str,
) {
    let mut class_depth: i32 = 0;

    for line in src.lines() {
        let trimmed = line.trim();
        let starts_class = trimmed.contains("class ");
        if starts_class {
            class_depth = (class_depth + brace_delta(line)).max(1);
        } else if class_depth > 0 {
            class_depth += brace_delta(line);
        }

        if class_depth > 0
            && trimmed.contains('#')
            && trimmed
                .chars()
                .skip_while(|ch| *ch != '#')
                .nth(1)
                .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        {
            diagnostics.push(diag(
                "error",
                "ANALYZER_UNSUPPORTED_CLASS_PRIVATE_ELEMENTS",
                "class private elements are currently unsupported in compiler mode",
                file,
            ));
            return;
        }

        if class_depth <= 0 {
            class_depth = 0;
        }
    }
}

fn collect_class_extends_diagnostics(src: &str, diagnostics: &mut Vec<DiagnosticIR>, file: &str) {
    for stmt in collect_statements(src) {
        let trimmed = stmt.trim();
        let class_rest = if let Some(rest) = trimmed.strip_prefix("class ") {
            Some(rest)
        } else if let Some(rest) = trimmed.strip_prefix("export class ") {
            Some(rest)
        } else {
            trimmed.strip_prefix("export default class ")
        };

        let Some(class_rest) = class_rest else {
            continue;
        };

        let after_name = if let Some((_, rest)) = split_identifier_and_rest(class_rest) {
            rest.trim_start()
        } else {
            class_rest.trim_start()
        };

        let Some(after_extends) = after_name.strip_prefix("extends ") else {
            continue;
        };

        if parse_simple_extends_target(after_extends).is_none() {
            diagnostics.push(diag(
                "error",
                "ANALYZER_UNSUPPORTED_CLASS_EXTENDS_EXPRESSION",
                "class extends target must be a simple identifier in compiler mode",
                file,
            ));
            return;
        }
    }
}

fn parse_simple_extends_target(input: &str) -> Option<&str> {
    let target = input.trim_start();
    let id = take_identifier(target)?;
    let tail = target[id.len()..].trim_start();
    if tail.is_empty()
        || tail.starts_with('{')
        || tail.starts_with("implements ")
        || tail.starts_with("/*")
        || tail.starts_with("//")
    {
        Some(id)
    } else {
        None
    }
}

fn collect_class_public_field_diagnostics(
    src: &str,
    diagnostics: &mut Vec<DiagnosticIR>,
    file: &str,
) {
    let mut class_depth: i32 = 0;

    for line in src.lines() {
        let trimmed = line.trim();

        if trimmed.contains("class ") {
            class_depth = (class_depth + brace_delta(line)).max(1);
            continue;
        }
        if class_depth <= 0 {
            continue;
        }

        let candidate = trimmed.trim_end_matches(';').trim_start();
        let looks_like_field = !candidate.is_empty()
            && !candidate.starts_with('#')
            && !candidate.starts_with("constructor(")
            && !candidate.starts_with("get ")
            && !candidate.starts_with("set ")
            && !candidate.starts_with("async ")
            && !candidate.contains('(')
            && !candidate.starts_with("return ");

        if looks_like_field && candidate.starts_with('[') {
            diagnostics.push(diag(
                "error",
                "ANALYZER_UNSUPPORTED_COMPUTED_CLASS_FIELD",
                "computed class field names are unsupported in compiler mode",
                file,
            ));
            return;
        }

        class_depth += brace_delta(line);
        if class_depth <= 0 {
            class_depth = 0;
        }
    }
}

fn collect_class_static_member_diagnostics(
    src: &str,
    diagnostics: &mut Vec<DiagnosticIR>,
    file: &str,
) {
    let mut class_depth: i32 = 0;

    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.contains("class ") {
            class_depth = (class_depth + brace_delta(line)).max(1);
            continue;
        }
        if class_depth <= 0 {
            continue;
        }

        if let Some(after_static) = trimmed.strip_prefix("static ") {
            if !after_static.starts_with('{') && !is_supported_static_member(after_static) {
                diagnostics.push(diag(
                    "error",
                    "ANALYZER_UNSUPPORTED_STATIC_CLASS_MEMBER",
                    "static class member name must be a simple identifier in compiler mode",
                    file,
                ));
                return;
            }
        }

        class_depth += brace_delta(line);
        if class_depth <= 0 {
            class_depth = 0;
        }
    }
}

fn is_supported_static_member(member_src: &str) -> bool {
    let src = member_src.trim_start();
    let src = src
        .strip_prefix("async ")
        .map_or(src, |rest| rest.trim_start());
    let Some(name) = take_identifier(src) else {
        return false;
    };
    let tail = src[name.len()..].trim_start();
    tail.starts_with('(') || tail.starts_with('=') || tail.starts_with(';')
}

fn has_route_invocation(line: &str) -> bool {
    ALLOWED_METHODS
        .iter()
        .map(|method| method.to_ascii_lowercase())
        .any(|method| line.contains(format!(".{method}(").as_str()))
        || line.contains(".route(")
}

fn brace_delta(raw: &str) -> i32 {
    let opens = raw.chars().filter(|ch| *ch == '{').count() as i32;
    let closes = raw.chars().filter(|ch| *ch == '}').count() as i32;
    opens - closes
}

fn collect_handler_defs(src: &str) -> BTreeMap<String, HandlerDef> {
    let mut handlers = BTreeMap::new();

    for stmt in collect_statements(src) {
        let trimmed = stmt.trim();

        for prefix in ["const ", "let ", "var "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some((name, after_name)) = split_identifier_and_rest(rest) {
                    let after_name = after_name.trim_start();
                    if !after_name.starts_with('=') {
                        continue;
                    }
                    let after_eq = after_name.trim_start_matches('=').trim_start();
                    if let Some((is_async, params, body)) = parse_arrow_fn(after_eq) {
                        let lowered_params = lower_params(&params);
                        handlers.insert(
                            name.to_string(),
                            HandlerDef {
                                is_async,
                                params: lowered_params.clone(),
                                semantics: lower_semantics(&lowered_params, &body),
                            },
                        );
                    }
                }
            }
        }

        if let Some(rest) = trimmed.strip_prefix("async function ") {
            if let Some((name, params, body)) = parse_function_decl(rest) {
                let lowered_params = lower_params(&params);
                handlers.insert(
                    name.to_string(),
                    HandlerDef {
                        is_async: true,
                        params: lowered_params.clone(),
                        semantics: lower_semantics(&lowered_params, &body),
                    },
                );
            }
        }

        if let Some(rest) = trimmed.strip_prefix("function ") {
            if let Some((name, params, body)) = parse_function_decl(rest) {
                let lowered_params = lower_params(&params);
                handlers.insert(
                    name.to_string(),
                    HandlerDef {
                        is_async: false,
                        params: lowered_params.clone(),
                        semantics: lower_semantics(&lowered_params, &body),
                    },
                );
            }
        }
    }

    handlers
}

fn collect_plugin_defs(src: &str) -> BTreeSet<String> {
    let mut defs = BTreeSet::new();

    for stmt in collect_statements(src) {
        let trimmed = stmt.trim();

        if let Some(rest) = trimmed.strip_prefix("async function ") {
            if let Some(name) = take_identifier(rest) {
                defs.insert(name.to_string());
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("function ") {
            if let Some(name) = take_identifier(rest) {
                defs.insert(name.to_string());
            }
            continue;
        }

        for prefix in ["const ", "let ", "var "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some((name, after_name)) = split_identifier_and_rest(rest) {
                    let after_name = after_name.trim_start();
                    if !after_name.starts_with('=') {
                        continue;
                    }
                    let after_eq = after_name.trim_start_matches('=').trim_start();
                    if parse_arrow_fn(after_eq).is_some()
                        || after_eq.starts_with("function")
                        || after_eq.starts_with("async function")
                    {
                        defs.insert(name.to_string());
                    }
                }
            }
        }
    }

    defs
}

fn collect_route_object_defs(src: &str) -> BTreeMap<String, String> {
    let mut defs = BTreeMap::new();

    for stmt in collect_statements(src) {
        let trimmed = stmt.trim_end_matches(';').trim();

        for prefix in ["const ", "let ", "var "] {
            let Some(rest) = trimmed.strip_prefix(prefix) else {
                continue;
            };

            let Some((name_raw, rhs_raw)) = rest.split_once('=') else {
                continue;
            };

            let name = name_raw.trim();
            if take_identifier(name) != Some(name) {
                continue;
            }

            let rhs = rhs_raw.trim();
            let rhs = rhs.strip_suffix(" as const").unwrap_or(rhs).trim();
            if rhs.starts_with('{') && rhs.ends_with('}') {
                defs.insert(name.to_string(), rhs.to_string());
            }
        }
    }

    defs
}

fn parse_arrow_fn(raw: &str) -> Option<(bool, Vec<String>, String)> {
    let mut trimmed = raw.trim();
    let is_async = if let Some(rest) = trimmed.strip_prefix("async") {
        trimmed = rest.trim_start();
        true
    } else {
        false
    };

    let arrow = trimmed.find("=>")?;
    let params_raw = trimmed[..arrow].trim();
    let body = trimmed[arrow + 2..]
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string();

    let params = parse_params(params_raw)?;
    Some((is_async, params, body))
}

fn parse_function_decl(raw: &str) -> Option<(&str, Vec<String>, String)> {
    let name = take_identifier(raw)?;
    let rest = raw[name.len()..].trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close = rest.find(')')?;
    let params_raw = &rest[..=close];
    let body = rest[close + 1..]
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string();
    let params = parse_params(params_raw)?;
    Some((name, params, body))
}

fn parse_params(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    let inner = if trimmed.starts_with('(') {
        let close = trimmed.find(')')?;
        &trimmed[1..close]
    } else {
        trimmed
    };

    if inner.trim().is_empty() {
        return Some(vec![]);
    }

    let mut params = Vec::new();
    for token in split_top_level_commas(inner) {
        let token = normalize_param_token(token.trim());
        let param = take_identifier(token.as_str())?;
        params.push(param.to_string());
    }
    Some(params)
}

fn normalize_param_token(raw: &str) -> String {
    let trimmed = raw.trim();

    let no_default = trimmed
        .split_once('=')
        .map(|(left, _)| left.trim())
        .unwrap_or(trimmed);

    let no_type = no_default
        .split_once(':')
        .map(|(left, _)| left.trim())
        .unwrap_or(no_default);

    no_type
        .trim_start_matches("...")
        .trim_end_matches('?')
        .trim()
        .to_string()
}

fn lower_params(params: &[String]) -> Vec<HandlerParamIR> {
    params
        .iter()
        .enumerate()
        .map(|(index, name)| HandlerParamIR {
            name: name.clone(),
            role: infer_param_role(name, index),
        })
        .collect()
}

fn infer_param_role(name: &str, index: usize) -> String {
    let lower = name.to_ascii_lowercase();
    if lower == "request" || lower == "req" {
        return "request".to_string();
    }
    if lower == "reply" || lower == "response" || lower == "res" {
        return "response".to_string();
    }
    if lower == "next" {
        return "next".to_string();
    }
    match index {
        0 => "request".to_string(),
        1 => "response".to_string(),
        2 => "next".to_string(),
        _ => "custom".to_string(),
    }
}

fn lower_semantics(params: &[HandlerParamIR], body: &str) -> HandlerSemanticsIR {
    let request_param = params
        .iter()
        .find(|param| param.role == "request")
        .map(|param| param.name.clone());
    let response_param = params
        .iter()
        .find(|param| param.role == "response")
        .map(|param| param.name.clone());

    let body_compact = body
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    let response_param_lower = response_param
        .as_ref()
        .map(|name| name.to_ascii_lowercase());

    let has_chained_call = |receiver: &str, via: &str, target: &str| {
        let anchor = format!("{receiver}.{via}(");
        let target_call = format!(".{target}(");

        body_compact
            .match_indices(anchor.as_str())
            .any(|(start, _)| {
                let rest = &body_compact[start..];
                let statement_end = rest.find(';').unwrap_or(rest.len());
                rest[..statement_end].contains(target_call.as_str())
            })
    };

    let has_call = |fn_name: &str| {
        response_param_lower
            .as_ref()
            .map(|response_name| {
                body_compact.contains(format!("{response_name}.{fn_name}(").as_str())
                    || has_chained_call(response_name, "status", fn_name)
                    || has_chained_call(response_name, "code", fn_name)
            })
            .unwrap_or(false)
    };

    let uses_status = has_call("status") || has_call("code");
    let uses_headers = has_call("header") || has_call("headers") || has_call("setheader");
    let uses_json = has_call("json");
    let uses_body = has_call("send") || has_call("body");

    let response_mode = if response_param.is_some() {
        "response-object"
    } else {
        "return"
    }
    .to_string();

    HandlerSemanticsIR {
        response_mode,
        request_param,
        response_param,
        uses_status,
        uses_body,
        uses_headers,
        uses_json,
    }
}

fn parse_shorthand_route(
    stmt: &str,
    diagnostics: &mut Vec<DiagnosticIR>,
    file: &str,
) -> Option<RouteIR> {
    if !stmt.contains('.') || !stmt.contains('(') || !stmt.ends_with(')') && !stmt.ends_with(");") {
        return None;
    }

    let trimmed = stmt.trim_end_matches(';').trim();
    let open = trimmed.find('(')?;
    let close = trimmed.rfind(')')?;
    let callee = trimmed[..open].trim();
    let args_raw = trimmed[open + 1..close].trim();

    let dot = callee.rfind('.')?;
    let method_raw = callee[dot + 1..].trim();
    let method = method_raw.to_ascii_uppercase();
    if !ALLOWED_METHODS.contains(&method.as_str()) {
        return None;
    }

    let args = split_top_level_commas(args_raw);
    if args.len() < 2 {
        return None;
    }

    let path_arg = args[0].trim();
    let handler_arg = args[1].trim();

    let path = if let Some(v) = extract_quoted(path_arg) {
        v.to_string()
    } else {
        diagnostics.push(diag(
            "error",
            "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
            "route path must be a string literal",
            file,
        ));
        return None;
    };

    let handler = if let Some(v) = parse_named_handler(handler_arg) {
        v
    } else {
        diagnostics.push(diag(
            "error",
            "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            "route handler must be a named reference",
            file,
        ));
        return None;
    };

    Some(RouteIR {
        method,
        path,
        handler_ref: handler,
    })
}

fn parse_route_object(
    stmt: &str,
    route_object_defs: &BTreeMap<String, String>,
    diagnostics: &mut Vec<DiagnosticIR>,
    file: &str,
) -> Option<RouteIR> {
    if !stmt.contains(".route(") {
        return None;
    }

    let trimmed = stmt.trim_end_matches(';').trim();
    let open = trimmed.find(".route(")? + ".route(".len();
    let close = trimmed.rfind(')')?;
    let arg = trimmed[open..close].trim();

    let object_literal = if arg.starts_with('{') && arg.ends_with('}') {
        arg.to_string()
    } else if let Some(route_ref) = parse_named_handler(arg) {
        if let Some(object_literal) = route_object_defs.get(&route_ref) {
            object_literal.clone()
        } else {
            diagnostics.push(diag(
                "error",
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE",
                "route object call requires an inline object literal",
                file,
            ));
            return None;
        }
    } else {
        diagnostics.push(diag(
            "error",
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE",
            "route object call requires an inline object literal",
            file,
        ));
        return None;
    };

    let method = if let Some(v) = extract_prop_quoted(&object_literal, "method") {
        let upper = v.to_ascii_uppercase();
        if !ALLOWED_METHODS.contains(&upper.as_str()) {
            diagnostics.push(diag(
                "error",
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
                "route object method must be one of GET|POST|PUT|DELETE|PATCH",
                file,
            ));
            return None;
        }
        upper
    } else {
        diagnostics.push(diag(
            "error",
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
            "route object method must be one of GET|POST|PUT|DELETE|PATCH",
            file,
        ));
        return None;
    };

    let path = extract_prop_quoted(&object_literal, "url")
        .or_else(|| extract_prop_quoted(&object_literal, "path"));
    let path = if let Some(v) = path {
        v
    } else {
        diagnostics.push(diag(
            "error",
            "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
            "route path must be a string literal",
            file,
        ));
        return None;
    };

    let handler = if let Some(v) = extract_prop_identifier(&object_literal, "handler") {
        v
    } else {
        diagnostics.push(diag(
            "error",
            "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            "route handler must be a named reference",
            file,
        ));
        return None;
    };

    Some(RouteIR {
        method,
        path,
        handler_ref: handler,
    })
}

fn parse_register_boundary(
    stmt: &str,
    plugin_defs: &BTreeSet<String>,
    diagnostics: &mut Vec<DiagnosticIR>,
    file: &str,
) {
    if !stmt.contains(".register(") {
        return;
    }

    let trimmed = stmt.trim_end_matches(';').trim();
    let Some(open) = trimmed.find(".register(") else {
        return;
    };
    let Some(close) = trimmed.rfind(')') else {
        return;
    };

    let args_raw = trimmed[open + ".register(".len()..close].trim();
    let args = split_top_level_commas(args_raw);
    let Some(callback_arg) = args.first().map(|v| v.trim()) else {
        return;
    };

    if callback_arg.starts_with("function")
        || callback_arg.starts_with("async function")
        || callback_arg.contains("=>")
    {
        return;
    }

    if let Some(plugin_ref) = parse_named_handler(callback_arg) {
        if plugin_defs.contains(&plugin_ref) {
            return;
        }

        diagnostics.push(diag(
            "error",
            "ANALYZER_UNRESOLVED_PLUGIN",
            "register plugin reference could not be resolved in current module",
            file,
        ));
        return;
    }

    diagnostics.push(diag(
        "error",
        "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK",
        "register callback must be an inline function or same-file named reference",
        file,
    ));
}

fn split_top_level_commas(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_bracket = 0i32;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in raw.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {}
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            ',' if depth_paren == 0 && depth_brace == 0 && depth_bracket == 0 => {
                out.push(raw[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }

    if start < raw.len() {
        out.push(raw[start..].trim());
    }

    out
}

fn extract_prop_quoted(obj_literal: &str, key: &str) -> Option<String> {
    let key_idx = obj_literal.find(key)?;
    let after = obj_literal[key_idx + key.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    extract_quoted(after).map(ToString::to_string)
}

fn extract_prop_identifier(obj_literal: &str, key: &str) -> Option<String> {
    let key_idx = obj_literal.find(key)?;
    let after = obj_literal[key_idx + key.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    parse_named_handler(after)
}

fn parse_named_handler(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with("async ") || trimmed.starts_with("function") || trimmed.contains("=>") {
        return None;
    }

    let ident = take_identifier(trimmed)?;
    let rest = trimmed[ident.len()..].trim_start();
    if rest.is_empty() || rest.starts_with(',') || rest.starts_with('}') || rest.starts_with(')') {
        Some(ident.to_string())
    } else {
        None
    }
}

fn extract_quoted(raw: &str) -> Option<&str> {
    for quote in ['\'', '"', '`'] {
        if let Some(start) = raw.find(quote) {
            let rest = &raw[start + 1..];
            if let Some(end) = rest.find(quote) {
                let value = &rest[..end];
                if quote == '`' && value.contains("${") {
                    return None;
                }
                return Some(value);
            }
        }
    }
    None
}

fn take_identifier(raw: &str) -> Option<&str> {
    let mut end = 0usize;
    for (idx, ch) in raw.char_indices() {
        let ok = ch == '_' || ch == '$' || ch.is_ascii_alphanumeric();
        if idx == 0 && !ok {
            return None;
        }
        if !ok {
            break;
        }
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        None
    } else {
        Some(&raw[..end])
    }
}

fn split_identifier_and_rest(raw: &str) -> Option<(&str, &str)> {
    let ident = take_identifier(raw)?;
    let rest = &raw[ident.len()..];
    Some((ident, rest))
}

fn diag(level: &str, code: &str, message: &str, file: &str) -> DiagnosticIR {
    DiagnosticIR {
        level: level.to_string(),
        code: code.to_string(),
        message: message.to_string(),
        source: Some(DiagnosticSourceIR {
            file: file.to_string(),
            line: None,
            column: None,
            via_source_map: None,
        }),
    }
}

fn sort_diagnostics(diagnostics: &mut [DiagnosticIR]) {
    diagnostics.sort_by(|a, b| {
        a.code
            .cmp(&b.code)
            .then_with(|| a.message.cmp(&b.message))
            .then_with(|| a.level.cmp(&b.level))
            .then_with(|| {
                a.source
                    .as_ref()
                    .map(|s| s.file.as_str())
                    .cmp(&b.source.as_ref().map(|s| s.file.as_str()))
            })
    });
}
