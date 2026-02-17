use std::collections::HashMap;

use crate::ast::{capture_call_args, find_if_block_ranges};
use crate::defs::{HandlerDef, PluginDef};
use crate::diagnostics::diag;
use crate::ir::{DiagnosticIR, HandlerIR, RouteIR};
use crate::register::analyze_register_call;
use crate::routes::{extract_route_object, extract_shorthand_route};

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_scope(
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
    let conditional_ranges = find_if_block_ranges(body);
    let mut idx = 0usize;
    while let Some((start, dot_idx)) = find_instance_dot(body, idx, instance_name) {
        let tail = &body[dot_idx + 1..];

        if conditional_ranges
            .iter()
            .any(|(from, to)| dot_idx >= *from && dot_idx < *to)
        {
            if let Some(method) = first_supported_method(tail) {
                diagnostics.push(diag(
                    file,
                    "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE",
                    &format!(
                        "conditional route registration in if-block is unsupported for deterministic extraction ({}.{}(...)). Move route declaration to top-level plugin scope.",
                        instance_name, method
                    ),
                ));
            }
            idx = start + 1;
            continue;
        }

        if let Some(consumed) = analyze_call_chain(
            tail,
            file,
            instance_name,
            prefix,
            plugin_defs,
            handler_defs,
            routes,
            handlers,
            diagnostics,
        ) {
            idx = dot_idx + 1 + consumed;
            continue;
        }

        idx = start + 1;
    }
}

fn find_instance_dot(body: &str, from: usize, instance_name: &str) -> Option<(usize, usize)> {
    let mut search_from = from;
    while let Some(rel) = body[search_from..].find(instance_name) {
        let start = search_from + rel;
        let end = start + instance_name.len();

        let prev_ok = if start == 0 {
            true
        } else {
            !is_ident_char(body.as_bytes()[start - 1])
        };
        if !prev_ok {
            search_from = start + 1;
            continue;
        }

        let mut i = end;
        while i < body.len() && body.as_bytes()[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < body.len() && body.as_bytes()[i] == b'.' {
            return Some((start, i));
        }

        search_from = start + 1;
    }
    None
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn first_supported_method(chain: &str) -> Option<&'static str> {
    ["get", "post", "put", "delete", "patch", "route", "register"]
        .iter()
        .find_map(|method| capture_call_args(chain, method).map(|_| *method))
}

#[allow(clippy::too_many_arguments)]
fn analyze_call_chain(
    mut chain: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    plugin_defs: &HashMap<String, PluginDef>,
    handler_defs: &HashMap<String, HandlerDef>,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    diagnostics: &mut Vec<DiagnosticIR>,
) -> Option<usize> {
    let original_len = chain.len();
    let mut matched_any = false;

    loop {
        let mut matched_here = false;

        for method in ["get", "post", "put", "delete", "patch"] {
            if let Some(args) = capture_call_args(chain, method) {
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
                let consumed = method.len() + args.len() + 2;
                chain = &chain[consumed..];
                matched_any = true;
                matched_here = true;
                break;
            }
        }

        if !matched_here {
            if let Some(args) = capture_call_args(chain, "route") {
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
                let consumed = "route".len() + args.len() + 2;
                chain = &chain[consumed..];
                matched_any = true;
                matched_here = true;
            }
        }

        if !matched_here {
            if let Some(args) = capture_call_args(chain, "register") {
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
                let consumed = "register".len() + args.len() + 2;
                chain = &chain[consumed..];
                matched_any = true;
                matched_here = true;
            }
        }

        if !matched_here {
            break;
        }

        let trimmed = chain.trim_start();
        if !trimmed.starts_with('.') {
            chain = trimmed;
            break;
        }
        chain = &trimmed[1..];
    }

    if matched_any {
        Some(original_len - chain.len())
    } else {
        None
    }
}
