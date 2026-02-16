use regex::Regex;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramIR {
    pub modules: Vec<ModuleIR>,
    pub routes: Vec<RouteIR>,
    pub handlers: Vec<HandlerIR>,
    pub diagnostics: Vec<DiagnosticIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleIR {
    pub id: String,
    pub source_path: String,
    pub exports: Vec<String>,
    pub imports: Vec<ImportIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIR {
    pub spec: String,
    pub kind: String,
    pub resolved: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteIR {
    pub method: String,
    pub path: String,
    pub handler_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerIR {
    pub id: String,
    pub params: Vec<HandlerParamIR>,
    pub r#async: bool,
    pub semantics: Option<HandlerSemanticsIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerParamIR {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerSemanticsIR {
    pub response_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticIR {
    pub level: String,
    pub code: String,
    pub message: String,
    pub source: Option<DiagnosticSourceIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSourceIR {
    pub file: String,
}

#[derive(Clone, Debug)]
struct PluginDef {
    param_name: String,
    body: String,
}

#[derive(Clone, Debug)]
struct HandlerDef {
    params: Vec<String>,
    is_async: bool,
}

pub fn analyze_fastify_entry(file: &str, src: &str) -> ProgramIR {
    let mut diagnostics = vec![];
    let mut routes = vec![];
    let mut handlers = vec![];

    let plugin_defs = collect_plugin_definitions(src);
    let handler_defs = collect_handler_definitions(src);

    let instance_name = detect_root_instance_name(src).unwrap_or_else(|| "fastify".to_string());
    analyze_scope(
        src,
        file,
        &instance_name,
        "",
        &plugin_defs,
        &handler_defs,
        &mut routes,
        &mut handlers,
        &mut diagnostics,
    );

    if src.contains("import(") {
        diagnostics.push(diag(file, "DYNAMIC_IMPORT_DETECTED", "dynamic import detected"));
    }

    ProgramIR { modules: vec![], routes, handlers, diagnostics }
}

#[allow(clippy::too_many_arguments)]
fn analyze_scope(
    body: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    plugin_defs: &HashMap<String, PluginDef>,
    handler_defs: &HashMap<String, HandlerDef>,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let mut idx = 0usize;
    while let Some(pos) = body[idx..].find(&format!("{}.", instance_name)) {
        let start = idx + pos;
        let tail = &body[start + instance_name.len() + 1..];
        let mut advanced = false;

        for method in ["get", "post", "put", "delete", "patch"] {
            if let Some(args) = capture_call_args(tail, method) {
                extract_shorthand_route(
                    &args,
                    file,
                    method,
                    instance_name,
                    prefix,
                    routes,
                    handlers,
                    handler_defs,
                    diagnostics,
                );
                idx = start + instance_name.len() + 1 + method.len() + args.len() + 2;
                advanced = true;
                break;
            }
        }
        if advanced {
            continue;
        }

        if let Some(args) = capture_call_args(tail, "route") {
            extract_route_object(
                &args,
                file,
                instance_name,
                prefix,
                routes,
                handlers,
                handler_defs,
                diagnostics,
            );
            idx = start + instance_name.len() + 1 + "route".len() + args.len() + 2;
            continue;
        }

        if let Some(args) = capture_call_args(tail, "register") {
            analyze_register_call(
                &args,
                file,
                instance_name,
                prefix,
                plugin_defs,
                handler_defs,
                routes,
                handlers,
                diagnostics,
            );
            idx = start + instance_name.len() + 1 + "register".len() + args.len() + 2;
            continue;
        }

        idx = start + 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_shorthand_route(
    args: &str,
    file: &str,
    method: &str,
    instance_name: &str,
    prefix: &str,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    handler_defs: &HashMap<String, HandlerDef>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let parts = split_top_level(args, ',');
    let path = parts.first().and_then(|s| extract_quoted_string(s));
    if path.is_none() {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
            &format!("unsupported dynamic path in {}.{}(...)", instance_name, method),
        ));
        return;
    }
    let path = path.unwrap();

    let handler_ref = parts.get(1).and_then(|s| extract_handler_ref(s));
    if handler_ref.is_none() {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            &format!(
                "unsupported non-reference handler in {}.{}('{}', handler)",
                instance_name, method, path
            ),
        ));
        return;
    }

    let handler_ref = handler_ref.unwrap();
    routes.push(RouteIR {
        method: method.to_ascii_uppercase(),
        path: join_path(prefix, &path),
        handler_ref: handler_ref.clone(),
    });
    upsert_handler(handlers, handler_defs, &handler_ref);
}

#[allow(clippy::too_many_arguments)]
fn extract_route_object(
    args: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    handler_defs: &HashMap<String, HandlerDef>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let Some(obj) = first_object_literal(args) else {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT",
            &format!("unsupported route object method in {}.route({{...}})", instance_name),
        ));
        return;
    };

    let method = extract_object_string_prop(&obj, "method").map(|m| m.to_ascii_uppercase());
    let path = extract_object_string_prop(&obj, "url").or_else(|| extract_object_string_prop(&obj, "path"));
    let handler_ref = extract_object_handler_ref(&obj, "handler");

    let supported = HashSet::from(["GET", "POST", "PUT", "DELETE", "PATCH"]);
    let Some(method) = method else {
        diagnostics.push(diag(file, "ANALYZER_UNSUPPORTED_ROUTE_OBJECT", &format!("unsupported route object method in {}.route({{...}})", instance_name)));
        return;
    };
    if !supported.contains(method.as_str()) {
        diagnostics.push(diag(file, "ANALYZER_UNSUPPORTED_ROUTE_OBJECT", &format!("unsupported route object method in {}.route({{...}})", instance_name)));
        return;
    }

    let Some(path) = path else {
        diagnostics.push(diag(file, "ANALYZER_UNSUPPORTED_DYNAMIC_PATH", &format!("unsupported route object path in {}.route({{...}})", instance_name)));
        return;
    };

    let Some(handler_ref) = handler_ref else {
        diagnostics.push(diag(file, "ANALYZER_UNSUPPORTED_INLINE_HANDLER", &format!("unsupported route object handler in {}.route({{...}})", instance_name)));
        return;
    };

    routes.push(RouteIR { method, path: join_path(prefix, &path), handler_ref: handler_ref.clone() });
    upsert_handler(handlers, handler_defs, &handler_ref);
}

#[allow(clippy::too_many_arguments)]
fn analyze_register_call(
    args: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    plugin_defs: &HashMap<String, PluginDef>,
    handler_defs: &HashMap<String, HandlerDef>,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let parts = split_top_level(args, ',');
    let plugin_expr = parts.first().map(|s| s.trim()).unwrap_or("");
    let options_expr = parts.get(1).map(|s| s.as_str()).unwrap_or("");
    let prefix_from_register = extract_object_string_prop(options_expr, "prefix").unwrap_or_default();
    let next_prefix = join_path(prefix, &prefix_from_register);

    if let Some(inline) = parse_inline_plugin(plugin_expr) {
        analyze_scope(
            &inline.body,
            file,
            &inline.param_name,
            &next_prefix,
            plugin_defs,
            handler_defs,
            routes,
            handlers,
            diagnostics,
        );
        return;
    }

    if let Some(plugin_ref) = extract_handler_ref(plugin_expr) {
        if let Some(plugin) = plugin_defs.get(&plugin_ref) {
            analyze_scope(
                &plugin.body,
                file,
                &plugin.param_name,
                &next_prefix,
                plugin_defs,
                handler_defs,
                routes,
                handlers,
                diagnostics,
            );
            return;
        }

        diagnostics.push(diag(
            file,
            "ANALYZER_UNRESOLVED_PLUGIN",
            &format!("register plugin '{}' could not be resolved in current file", plugin_ref),
        ));
        return;
    }

    diagnostics.push(diag(
        file,
        "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK",
        &format!("unsupported register callback pattern on {}.register(...)", instance_name),
    ));
}

fn collect_plugin_definitions(src: &str) -> HashMap<String, PluginDef> {
    let mut map = HashMap::new();

    let fn_re = Regex::new(r"function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*\{").unwrap();
    for cap in fn_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else { continue };
        let params = cap[2]
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if let Some(first) = params.first() {
            map.insert(
                cap[1].to_string(),
                PluginDef {
                    param_name: (*first).to_string(),
                    body,
                },
            );
        }
    }

    let arrow_re = Regex::new(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?\(([^)]*)\)\s*=>\s*\{").unwrap();
    for cap in arrow_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else { continue };
        let params = cap[2]
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if let Some(first) = params.first() {
            map.insert(
                cap[1].to_string(),
                PluginDef {
                    param_name: (*first).to_string(),
                    body,
                },
            );
        }
    }

    let var_fn_re = Regex::new(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?function\s*\(([^)]*)\)\s*\{").unwrap();
    for cap in var_fn_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else { continue };
        let params = cap[2]
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if let Some(first) = params.first() {
            map.insert(
                cap[1].to_string(),
                PluginDef {
                    param_name: (*first).to_string(),
                    body,
                },
            );
        }
    }

    map
}

fn collect_handler_definitions(src: &str) -> HashMap<String, HandlerDef> {
    let mut map = HashMap::new();

    let fn_re = Regex::new(r"(?s)(async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*\{").unwrap();
    for cap in fn_re.captures_iter(src) {
        map.insert(
            cap[2].to_string(),
            HandlerDef {
                params: split_params(&cap[3]),
                is_async: cap.get(1).is_some(),
            },
        );
    }

    let var_fn_re = Regex::new(r"(?s)(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(async\s+)?(?:function\s*\(([^)]*)\)|\(([^)]*)\)\s*=>)\s*\{").unwrap();
    for cap in var_fn_re.captures_iter(src) {
        let params_src = cap.get(3).or_else(|| cap.get(4)).map(|m| m.as_str()).unwrap_or("");
        map.insert(
            cap[1].to_string(),
            HandlerDef {
                params: split_params(params_src),
                is_async: cap.get(2).is_some(),
            },
        );
    }

    map
}

fn detect_root_instance_name(src: &str) -> Option<String> {
    let re = Regex::new(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*Fastify\s*\(").unwrap();
    re.captures(src).map(|c| c[1].to_string())
}

fn capture_call_args(tail: &str, method: &str) -> Option<String> {
    let needle = format!("{}(", method);
    let mut s = tail.trim_start();
    if !s.starts_with(&needle) {
        return None;
    }
    s = &s[needle.len()..];
    let (inside, _) = capture_balanced(s, '(', ')')?;
    Some(inside)
}

fn capture_balanced(src: &str, open: char, close: char) -> Option<(String, usize)> {
    let mut depth = 1i32;
    let mut i = 0usize;
    let chars = src.chars().collect::<Vec<_>>();
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                i = skip_string(&chars, i);
                continue;
            }
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    let inside = chars[..i].iter().collect::<String>();
                    return Some((inside, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_string(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    chars.len()
}

fn split_top_level(src: &str, delim: char) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut paren = 0i32;
    let mut brace = 0i32;
    let mut bracket = 0i32;
    let chars = src.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                let next = skip_string(&chars, i);
                cur.push_str(&chars[i..next].iter().collect::<String>());
                i = next;
                continue;
            }
            '(' => paren += 1,
            ')' => paren -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            c if c == delim && paren == 0 && brace == 0 && bracket == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                i += 1;
                continue;
            }
            _ => {}
        }
        cur.push(chars[i]);
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn parse_inline_plugin(expr: &str) -> Option<PluginDef> {
    let re_arrow = Regex::new(r"(?s)^(?:async\s+)?\(\s*([A-Za-z_$][\w$]*)\s*\)\s*=>\s*\{(.*)\}$").unwrap();
    if let Some(cap) = re_arrow.captures(expr.trim()) {
        return Some(PluginDef { param_name: cap[1].to_string(), body: cap[2].to_string() });
    }

    let re_fn = Regex::new(r"(?s)^(?:async\s+)?function(?:\s+[A-Za-z_$][\w$]*)?\s*\(\s*([A-Za-z_$][\w$]*)\s*[^)]*\)\s*\{(.*)\}$").unwrap();
    re_fn.captures(expr.trim()).map(|cap| PluginDef { param_name: cap[1].to_string(), body: cap[2].to_string() })
}

fn first_object_literal(src: &str) -> Option<String> {
    let t = src.trim_start();
    if !t.starts_with('{') {
        return None;
    }
    let (obj, _) = capture_balanced(&t[1..], '{', '}')?;
    Some(obj)
}

fn extract_quoted_string(v: &str) -> Option<String> {
    let t = v.trim();
    if t.len() >= 2 && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\''))) {
        return Some(t[1..t.len() - 1].to_string());
    }
    None
}

fn extract_handler_ref(v: &str) -> Option<String> {
    let t = v.trim();
    let re = Regex::new(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$").unwrap();
    if re.is_match(t) {
        Some(t.to_string())
    } else {
        None
    }
}

fn extract_object_string_prop(obj: &str, key: &str) -> Option<String> {
    let pattern = format!(r#"(?s)(?:\b{}\b|"{}")\s*:\s*("[^"]*"|'[^']*')"#, regex::escape(key), regex::escape(key));
    let re = Regex::new(&pattern).unwrap();
    re.captures(obj).and_then(|c| extract_quoted_string(&c[1]))
}

fn extract_object_handler_ref(obj: &str, key: &str) -> Option<String> {
    let pattern = format!(r#"(?s)(?:\b{}\b|"{}")\s*:\s*([A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*)"#, regex::escape(key), regex::escape(key));
    let re = Regex::new(&pattern).unwrap();
    re.captures(obj).map(|c| c[1].to_string())
}

fn upsert_handler(handlers: &mut Vec<HandlerIR>, defs: &HashMap<String, HandlerDef>, handler_ref: &str) {
    if handler_ref.contains('.') {
        return;
    }
    if handlers.iter().any(|h| h.id == handler_ref) {
        return;
    }
    let Some(def) = defs.get(handler_ref) else { return };
    handlers.push(HandlerIR {
        id: handler_ref.to_string(),
        params: def
            .params
            .iter()
            .map(|name| HandlerParamIR {
                name: name.clone(),
                role: infer_param_role(name),
            })
            .collect(),
        r#async: def.is_async,
        semantics: Some(HandlerSemanticsIR {
            response_mode: "unknown".to_string(),
        }),
    });
}

fn split_params(src: &str) -> Vec<String> {
    src.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(ToString::to_string).collect()
}

fn infer_param_role(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "req" | "request" => "request",
        "res" | "reply" | "response" => "response",
        "next" => "next",
        _ => "custom",
    }
    .to_string()
}

fn join_path(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim();
    let path = path.trim();
    if prefix.is_empty() {
        return ensure_slash(path);
    }
    if path.is_empty() {
        return ensure_slash(prefix);
    }
    let left = ensure_slash(prefix).trim_end_matches('/').to_string();
    let right = ensure_slash(path);
    format!("{}{}", left, right)
}

fn ensure_slash(v: &str) -> String {
    if v.starts_with('/') {
        v.to_string()
    } else {
        format!("/{}", v)
    }
}

fn diag(file: &str, code: &str, message: &str) -> DiagnosticIR {
    DiagnosticIR {
        level: "warn".to_string(),
        code: code.to_string(),
        message: message.to_string(),
        source: Some(DiagnosticSourceIR { file: file.to_string() }),
    }
}
