use std::collections::{HashMap, HashSet};

use crate::ast::{
    extract_handler_ref, extract_object_handler_ref, extract_object_string_prop,
    extract_quoted_string, first_object_literal, split_top_level,
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

    let handler_ref = parts.get(1).and_then(|s| extract_handler_ref(s));
    if handler_ref.is_none() {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            &format!(
                "unsupported non-reference handler in {}.{}('{}', handler). Extract handler to a named function and pass its identifier.",
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
pub(crate) fn extract_route_object(
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
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE",
            &format!(
                "unsupported route object pattern in {}.route(...). Provide an inline object literal (e.g. {{ method: 'GET', url: '/users', handler: listUsers }}).",
                instance_name
            ),
        ));
        return;
    };

    let raw_method = extract_object_string_prop(&obj, "method");
    let method = raw_method.as_deref().map(|m| m.to_ascii_uppercase());
    let path = extract_object_string_prop(&obj, "url")
        .or_else(|| extract_object_string_prop(&obj, "path"));
    let handler_ref = extract_object_handler_ref(&obj, "handler");

    let supported = HashSet::from(["GET", "POST", "PUT", "DELETE", "PATCH"]);
    let Some(method) = method else {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
            &format!(
                "unsupported route object method in {}.route({{...}}): missing string 'method'. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                instance_name
            ),
        ));
        return;
    };
    if !supported.contains(method.as_str()) {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD",
            &format!(
                "unsupported route object method in {}.route({{...}}): '{}'. Supported methods: GET|POST|PUT|DELETE|PATCH.",
                instance_name,
                raw_method.unwrap_or_else(|| "<unknown>".to_string())
            ),
        ));
        return;
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

    let Some(handler_ref) = handler_ref else {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
            &format!(
                "unsupported route object handler in {}.route({{...}}). Provide named handler reference in 'handler' field.",
                instance_name
            ),
        ));
        return;
    };

    routes.push(RouteIR {
        method,
        path: join_path(prefix, &path),
        handler_ref: handler_ref.clone(),
    });
    upsert_handler(handlers, handler_defs, &handler_ref);
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
