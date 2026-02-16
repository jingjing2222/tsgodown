use regex::Regex;
use std::collections::HashMap;

use crate::ast::capture_balanced;

#[derive(Clone, Debug)]
pub(crate) struct PluginDef {
    pub(crate) param_name: String,
    pub(crate) body: String,
}

#[derive(Clone, Debug)]
pub(crate) struct HandlerDef {
    pub(crate) params: Vec<String>,
    pub(crate) is_async: bool,
}

pub(crate) fn collect_plugin_definitions(src: &str) -> HashMap<String, PluginDef> {
    let mut map = HashMap::new();

    let fn_re = Regex::new(r"function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*\{").unwrap();
    for cap in fn_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };
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

    let arrow_re = Regex::new(
        r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?\(([^)]*)\)\s*=>\s*\{",
    )
    .unwrap();
    for cap in arrow_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };
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

    let var_fn_re = Regex::new(
        r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?function\s*\(([^)]*)\)\s*\{",
    )
    .unwrap();
    for cap in var_fn_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };
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

pub(crate) fn collect_handler_definitions(src: &str) -> HashMap<String, HandlerDef> {
    let mut map = HashMap::new();

    let fn_re =
        Regex::new(r"(?s)(async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*\{").unwrap();
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
        let params_src = cap
            .get(3)
            .or_else(|| cap.get(4))
            .map(|m| m.as_str())
            .unwrap_or("");
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

pub(crate) fn detect_root_instance_name(src: &str) -> Option<String> {
    let re = Regex::new(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*Fastify\s*\(").unwrap();
    re.captures(src).map(|c| c[1].to_string())
}

fn split_params(src: &str) -> Vec<String> {
    src.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}
