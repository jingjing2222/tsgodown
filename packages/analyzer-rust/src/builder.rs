use std::collections::{BTreeMap, BTreeSet};

use crate::{DiagnosticIR, DiagnosticSourceIR, HandlerIR, ImportIR, ModuleIR, ProgramIR, RouteIR};

const ALLOWED_METHODS: [&str; 5] = ["GET", "POST", "PUT", "DELETE", "PATCH"];

pub fn build_program_ir(file: &str, src: &str) -> ProgramIR {
    let mut diagnostics = Vec::new();

    let imports = collect_imports(src, &mut diagnostics, file);
    let exports = collect_exports(src);
    let handler_defs = collect_handler_defs(src);

    let mut routes = Vec::new();
    let mut referenced_handlers = BTreeSet::new();
    for stmt in collect_statements(src) {
        if let Some(route) = parse_shorthand_route(stmt, &mut diagnostics, file) {
            referenced_handlers.insert(route.handler_ref.clone());
            routes.push(route);
            continue;
        }
        if let Some(route) = parse_route_object(stmt, &mut diagnostics, file) {
            referenced_handlers.insert(route.handler_ref.clone());
            routes.push(route);
        }
    }

    routes.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.method.cmp(&b.method))
            .then_with(|| a.handler_ref.cmp(&b.handler_ref))
    });

    let mut handlers = referenced_handlers
        .into_iter()
        .map(|id| HandlerIR {
            r#async: handler_defs.get(&id).copied().unwrap_or(false),
            id,
            params: vec![],
            semantics: None,
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
    for (idx, ch) in src.char_indices() {
        if ch == ';' {
            out.push(src[start..=idx].trim());
            start = idx + 1;
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

fn collect_handler_defs(src: &str) -> BTreeMap<String, bool> {
    let mut handlers = BTreeMap::new();

    for line in src.lines() {
        let trimmed = line.trim();

        for prefix in ["const ", "let ", "var "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some((name, after_name)) = split_identifier_and_rest(rest) {
                    if after_name.trim_start().starts_with('=') {
                        let is_async =
                            after_name.contains("= async") || after_name.contains("=async");
                        handlers.insert(name.to_string(), is_async);
                    }
                }
            }
        }

        if let Some(rest) = trimmed.strip_prefix("function ") {
            if let Some(name) = take_identifier(rest) {
                handlers.insert(name.to_string(), false);
            }
        }

        if let Some(rest) = trimmed.strip_prefix("async function ") {
            if let Some(name) = take_identifier(rest) {
                handlers.insert(name.to_string(), true);
            }
        }
    }

    handlers
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

    if !arg.starts_with('{') || !arg.ends_with('}') {
        diagnostics.push(diag(
            "error",
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE",
            "fastify.route(...) requires an inline object literal",
            file,
        ));
        return None;
    }

    let method = if let Some(v) = extract_prop_quoted(arg, "method") {
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

    let path = extract_prop_quoted(arg, "url").or_else(|| extract_prop_quoted(arg, "path"));
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

    let handler = if let Some(v) = extract_prop_identifier(arg, "handler") {
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
    for quote in ['\'', '"'] {
        if let Some(start) = raw.find(quote) {
            let rest = &raw[start + 1..];
            if let Some(end) = rest.find(quote) {
                return Some(&rest[..end]);
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
