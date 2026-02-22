use analyzer_rust::analyze_compiler_entry;
#[path = "support/tsdown_fixture.rs"]
mod tsdown_fixture;

#[test]
fn returns_module_envelope_for_compiler_mode_core_builder() {
    let bundled = tsdown_fixture::build_inline_source("export const health = () => ({ ok: true });");
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.modules.len(), 1);
    assert_eq!(ir.modules[0].id, "src/index.ts");
    assert!(ir.modules[0].exports.is_empty());
    assert!(ir.routes.is_empty());
    assert!(ir.handlers.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn collects_exported_class_symbols_in_module_exports() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class HealthController {
  handle() {
    return { ok: true };
  }
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.modules.len(), 1);
    assert!(ir.modules[0].exports.is_empty());
    assert!(ir.routes.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_deterministic_diagnostic_for_class_private_elements() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
class Counter {
  #count = 0;
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.routes.len(), 0);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn collects_default_exported_class_with_constructor_symbol() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export default class HealthController {
  constructor(service) {
    this.service = service;
  }
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.modules.len(), 1);
    assert!(ir.modules[0].exports.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn supports_simple_class_extends_clause() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class ApiController extends BaseController {
  constructor() {}
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.modules.len(), 1);
    assert!(ir.modules[0].exports.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_diagnostic_for_non_identifier_extends_target() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class ApiController extends buildBase() {}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.diagnostics.len(), 1);
    assert_eq!(
        ir.diagnostics[0].code,
        "ANALYZER_UNSUPPORTED_CLASS_PRIVATE_ELEMENTS"
    );
}

#[test]
fn supports_simple_public_class_fields() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class ProfileController {
  retries = 3;
  label = "ok";
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert!(ir.modules[0].exports.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_diagnostic_for_computed_public_class_fields() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class ProfileController {
  [dynamicName] = 1;
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.diagnostics.len(), 1);
    assert_eq!(
        ir.diagnostics[0].code,
        "ANALYZER_UNSUPPORTED_COMPUTED_CLASS_FIELD"
    );
    assert_eq!(
        ir.diagnostics[0].message,
        "computed class field names are unsupported in compiler mode"
    );
}

#[test]
fn supports_simple_static_class_members() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class Cache {
  static VERSION = 1;
  static reset() {}
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert!(ir.modules[0].exports.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_diagnostic_for_computed_static_class_members() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class Cache {
  static [nameExpr] = 1;
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert_eq!(ir.diagnostics.len(), 1);
    assert_eq!(
        ir.diagnostics[0].code,
        "ANALYZER_UNSUPPORTED_STATIC_CLASS_MEMBER"
    );
    assert_eq!(
        ir.diagnostics[0].message,
        "static class member name must be a simple identifier in compiler mode"
    );
}

#[test]
fn supports_static_initialization_blocks() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class Cache {
  static {
    const seed = 1;
    void seed;
  }
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert!(ir.modules[0].exports.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn ignores_static_block_body_for_public_field_computed_diagnostic() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
export class Cache {
  static {
    [dynamicName] = 1;
  }
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_deprecated_and_obsolete_feature_diagnostics() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
const text = escape("a b");
const raw = unescape(text);
obj.__defineGetter__("x", () => 1);
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    let codes = ir
        .diagnostics
        .iter()
        .map(|diag| diag.code.as_str())
        .collect::<Vec<_>>();

    assert!(codes.contains(&"ANALYZER_DEPRECATED_ESCAPE_API"));
    assert!(codes.contains(&"ANALYZER_DEPRECATED_UNESCAPE_API"));
    assert!(codes.contains(&"ANALYZER_DEPRECATED_LEGACY_ACCESSOR_API"));
}

#[test]
fn emits_generator_reentry_warning_for_already_executing_generator_pattern() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
function* task() {
  task.next();
  yield 1;
}
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_duplicate_pragma_warning_for_already_has_pragma_pattern() {
    let bundled = tsdown_fixture::build_inline_source(
        r#"
'use strict';
'use strict';
const answer = 42;
"#,
    );
    let ir = analyze_compiler_entry("src/index.ts", &bundled);

    assert!(ir
        .diagnostics
        .iter()
        .all(|diag| diag.code != "ANALYZER_DUPLICATE_PRAGMA"));
}
