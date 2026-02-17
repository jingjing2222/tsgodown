use std::collections::{HashMap, HashSet};

use crate::ast::{
    extract_handler_ref, extract_identifier, extract_object_handler_ref, extract_object_prop_expr,
    extract_object_string_array_prop, extract_object_string_prop, extract_quoted_string,
    first_object_literal, parse_inline_handler_signature, resolve_bound_object_literal,
    resolve_bound_static_string, split_top_level,
};
use crate::defs::HandlerDef;
use crate::diagnostics::diag;
use crate::ir::{DiagnosticIR, HandlerIR, HandlerParamIR, HandlerSemanticsIR, RouteIR};

#[allow(clippy::too_many_arguments)]
pub(crate) fn extract_shorthand_route(
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
            &format!(
                "unsupported dynamic path in {}.{}(...). Use string literal path (e.g. '/users/:id') for IR extraction.",
                instance_name, method
            ),
        ));
        return;
    }
    let path = path.unwrap();

    let method_upper = method.to_ascii_uppercase();
    let joined_path = join_path(prefix, &path);

    let handler_expr = parts.get(1).map(|s| s.trim()).unwrap_or("");
    if let Some(handler_ref) = extract_handler_ref(handler_expr) {
        routes.push(RouteIR {
            method: method_upper,
            path: joined_path,
            handler_ref: handler_ref.clone(),
        });
        upsert_handler(handlers, handler_defs, &handler_ref);
        return;
    }

    if let Some((params, is_async)) = parse_inline_handler_signature(handler_expr) {
        let synthesized = synthesize_inline_handler_ref(&method_upper, &joined_path, handler_expr);
        routes.push(RouteIR {
            method: method_upper,
            path: joined_path,
            handler_ref: synthesized.clone(),
        });
        upsert_inline_handler(handlers, &synthesized, params, is_async);
        return;
    }

    diagnostics.push(diag(
        file,
        "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
        &format!(
            "unsupported non-reference handler in {}.{}('{}', handler). Extract handler to a named function and pass its identifier.",
            instance_name, method, path
        ),
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn extract_route_object(
    args: &str,
    scope_src: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    handler_defs: &HashMap<String, HandlerDef>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let obj = first_object_literal(args).or_else(|| {
        extract_identifier(args).and_then(|name| resolve_bound_object_literal(scope_src, &name))
    });

    let Some(obj) = obj else {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE",
            &format!(
                "unsupported route object pattern in {}.route(...). Provide an inline object literal or local const object reference (e.g. {{ method: 'GET', url: '/users', handler: listUsers }}).",
                instance_name
            ),
        ));
        return;
    };

    let raw_method = extract_object_string_prop(&obj, "method").or_else(|| {
        extract_object_prop_expr(&obj, "method")
            .and_then(|expr| extract_identifier(&expr))
            .and_then(|name| resolve_bound_static_string(scope_src, &name))
    });
    let raw_method_array = extract_object_string_array_prop(&obj, "method");
    let path = extract_object_string_prop(&obj, "url")
        .or_else(|| {
            extract_object_prop_expr(&obj, "url")
                .and_then(|expr| extract_identifier(&expr))
                .and_then(|name| resolve_bound_static_string(scope_src, &name))
        })
        .or_else(|| extract_object_string_prop(&obj, "path"))
        .or_else(|| {
            extract_object_prop_expr(&obj, "path")
                .and_then(|expr| extract_identifier(&expr))
                .and_then(|name| resolve_bound_static_string(scope_src, &name))
        });
    let handler_ref = extract_object_handler_ref(&obj, "handler");
    let handler_expr = extract_object_prop_expr(&obj, "handler");

    let supported = HashSet::from(["GET", "POST", "PUT", "DELETE", "PATCH"]);

    let methods = if let Some(method) = raw_method {
        vec![method]
    } else if !raw_method_array.is_empty() {
        raw_method_array
    } else {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
            &format!(
                "unsupported route object method in {}.route({{...}}): missing string 'method' or non-empty string array. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                instance_name
            ),
        ));
        return;
    };

    let methods = methods
        .iter()
        .map(|m| {
            let upper = m.to_ascii_uppercase();
            let normalized = if upper == "DEL" {
                "DELETE".to_string()
            } else {
                upper
            };
            (m.clone(), normalized)
        })
        .collect::<Vec<_>>();

    for (raw, upper) in &methods {
        if !supported.contains(upper.as_str()) {
            diagnostics.push(diag(
                file,
                "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
                &format!(
                    "unsupported route object method in {}.route({{...}}): '{}'. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                    instance_name, raw
                ),
            ));
            return;
        }
    }

    let Some(path) = path else {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
            &format!(
                "unsupported route object path in {}.route({{...}}). Provide string literal 'url' or 'path' (e.g. '/users/:id').",
                instance_name
            ),
        ));
        return;
    };

    if let Some(handler_ref) = handler_ref {
        for (_, method) in methods {
            routes.push(RouteIR {
                method,
                path: join_path(prefix, &path),
                handler_ref: handler_ref.clone(),
            });
        }
        upsert_handler(handlers, handler_defs, &handler_ref);
        return;
    }

    if let Some(expr) = handler_expr {
        if let Some((params, is_async)) = parse_inline_handler_signature(&expr) {
            let synthesized =
                synthesize_inline_handler_ref("ROUTE", &join_path(prefix, &path), &expr);
            for (_, method) in methods {
                routes.push(RouteIR {
                    method,
                    path: join_path(prefix, &path),
                    handler_ref: synthesized.clone(),
                });
            }
            upsert_inline_handler(handlers, &synthesized, params, is_async);
            return;
        }
    }

    diagnostics.push(diag(
        file,
        "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
        &format!(
            "unsupported route object handler in {}.route({{...}}). Provide named handler reference in 'handler' field.",
            instance_name
        ),
    ));
}

fn upsert_inline_handler(
    handlers: &mut Vec<HandlerIR>,
    handler_ref: &str,
    params: Vec<String>,
    is_async: bool,
) {
    if handlers.iter().any(|h| h.id == handler_ref) {
        return;
    }
    handlers.push(HandlerIR {
        id: handler_ref.to_string(),
        params: params
            .iter()
            .map(|name| normalize_inline_param(name))
            .map(|name| HandlerParamIR {
                role: infer_param_role(&name),
                name,
            })
            .collect(),
        r#async: is_async,
        semantics: Some(HandlerSemanticsIR {
            response_mode: "unknown".to_string(),
        }),
    });
}

fn normalize_inline_param(param: &str) -> String {
    let without_default = param.split('=').next().unwrap_or(param).trim();
    let without_type = without_default
        .split(':')
        .next()
        .unwrap_or(without_default)
        .trim();
    without_type.trim_start_matches("...").trim().to_string()
}

fn synthesize_inline_handler_ref(method: &str, path: &str, expr: &str) -> String {
    let mut hash: u64 = 1469598103934665603;
    for b in format!("{}|{}|{}", method, path, expr.trim()).as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("__inline__{}__{:016x}", method.to_ascii_lowercase(), hash)
}

pub(crate) fn upsert_handler(
    handlers: &mut Vec<HandlerIR>,
    defs: &HashMap<String, HandlerDef>,
    handler_ref: &str,
) {
    if handlers.iter().any(|h| h.id == handler_ref) {
        return;
    }
    let Some(def) = defs.get(handler_ref) else {
        return;
    };
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

fn infer_param_role(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "req" | "request" => "request",
        "res" | "reply" | "response" => "response",
        "next" => "next",
        _ => "custom",
    }
    .to_string()
}

pub(crate) fn join_path(prefix: &str, path: &str) -> String {
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
