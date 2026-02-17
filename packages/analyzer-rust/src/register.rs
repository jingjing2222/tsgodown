use std::collections::{HashMap, HashSet};

use crate::ast::{
    extract_handler_ref, extract_identifier, extract_object_string_prop, parse_inline_plugin,
    split_top_level, unwrap_single_call_arg,
};
use crate::defs::{HandlerDef, PluginDef};
use crate::diagnostics::diag;
use crate::ir::{DiagnosticIR, HandlerIR, RouteIR};
use crate::routes::join_path;
use crate::traversal::analyze_scope;

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_register_call(
    args: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    plugin_defs: &HashMap<String, PluginDef>,
    plugin_aliases: &HashMap<String, String>,
    plugin_call_aliases: &HashMap<String, String>,
    handler_defs: &HashMap<String, HandlerDef>,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let parts = split_top_level(args, ',');
    let plugin_expr = parts.first().map(|s| s.trim()).unwrap_or("");
    let options_expr = parts.get(1).map(|s| s.as_str()).unwrap_or("");
    let prefix_from_register =
        extract_object_string_prop(options_expr, "prefix").unwrap_or_default();
    let next_prefix = join_path(prefix, &prefix_from_register);

    let mut candidates = vec![];
    let mut stack = vec![plugin_expr.to_string()];
    let mut seen = HashSet::new();

    while let Some(candidate) = stack.pop() {
        let normalized = candidate.trim().to_string();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }

        if let Some(inner) = unwrap_single_call_arg(&normalized) {
            stack.push(inner);
        }

        if let Some(identifier) = extract_identifier(&normalized) {
            if let Some(next) = plugin_aliases.get(&identifier) {
                stack.push(next.clone());
            }
            if let Some(call_expr) = plugin_call_aliases.get(&identifier) {
                stack.push(call_expr.clone());
            }
        }

        candidates.push(normalized);
    }

    let mut unresolved_plugin_ref: Option<String> = None;

    for candidate in &candidates {
        if let Some(inline) = parse_inline_plugin(candidate) {
            analyze_scope(
                &inline.body,
                file,
                &inline.param_name,
                &next_prefix,
                plugin_defs,
                plugin_aliases,
                plugin_call_aliases,
                handler_defs,
                routes,
                handlers,
                diagnostics,
            );
            return;
        }

        if let Some(plugin_ref) = extract_handler_ref(candidate) {
            if let Some(plugin) = plugin_defs.get(&plugin_ref) {
                analyze_scope(
                    &plugin.body,
                    file,
                    &plugin.param_name,
                    &next_prefix,
                    plugin_defs,
                    plugin_aliases,
                    plugin_call_aliases,
                    handler_defs,
                    routes,
                    handlers,
                    diagnostics,
                );
                return;
            }

            let mut resolved = plugin_ref.clone();
            let mut depth = 0usize;
            while let Some(next) = plugin_aliases.get(&resolved) {
                if next == &resolved || depth > 8 {
                    break;
                }
                resolved = next.clone();
                depth += 1;
                if let Some(plugin) = plugin_defs.get(&resolved) {
                    analyze_scope(
                        &plugin.body,
                        file,
                        &plugin.param_name,
                        &next_prefix,
                        plugin_defs,
                        plugin_aliases,
                        plugin_call_aliases,
                        handler_defs,
                        routes,
                        handlers,
                        diagnostics,
                    );
                    return;
                }
            }

            if unresolved_plugin_ref.is_none() {
                unresolved_plugin_ref = Some(plugin_ref);
            }
        }
    }

    if let Some(plugin_ref) = unresolved_plugin_ref {
        diagnostics.push(diag(
            file,
            "ANALYZER_UNRESOLVED_PLUGIN",
            &format!(
                "register plugin '{}' could not be resolved in current file. Ensure plugin is declared in the same file or use an inline callback.",
                plugin_ref
            ),
        ));
        return;
    }

    diagnostics.push(diag(
        file,
        "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK",
        &format!(
            "unsupported register callback pattern on {}.register(...). Use inline function(plugin) {{ ... }} or named local plugin reference.",
            instance_name
        ),
    ));
}
