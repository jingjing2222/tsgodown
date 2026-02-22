use analyzer_rust::analyze_compiler_entry;

#[test]
fn returns_module_envelope_for_compiler_mode_core_builder() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        "export const health = () => ({ ok: true });",
    );

    assert_eq!(ir.modules.len(), 1);
    assert_eq!(ir.modules[0].id, "src/index.ts");
    assert_eq!(ir.modules[0].exports, vec!["health".to_string()]);
    assert!(ir.routes.is_empty());
    assert!(ir.handlers.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn collects_exported_class_symbols_in_module_exports() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export class HealthController {
  handle() {
    return { ok: true };
  }
}
"#,
    );

    assert_eq!(ir.modules.len(), 1);
    assert_eq!(
        ir.modules[0].exports,
        vec!["HealthController".to_string()]
    );
    assert!(ir.routes.is_empty());
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_deterministic_diagnostic_for_class_private_elements() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
class Counter {
  #count = 0;
}
"#,
    );

    assert_eq!(ir.routes.len(), 0);
    assert_eq!(ir.diagnostics.len(), 1);
    assert_eq!(
        ir.diagnostics[0].code,
        "ANALYZER_UNSUPPORTED_CLASS_PRIVATE_ELEMENTS"
    );
    assert_eq!(
        ir.diagnostics[0].message,
        "class private elements are currently unsupported in compiler mode"
    );
}

#[test]
fn collects_default_exported_class_with_constructor_symbol() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export default class HealthController {
  constructor(service) {
    this.service = service;
  }
}
"#,
    );

    assert_eq!(ir.modules.len(), 1);
    assert_eq!(
        ir.modules[0].exports,
        vec!["HealthController".to_string()]
    );
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn supports_simple_class_extends_clause() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export class ApiController extends BaseController {
  constructor() {}
}
"#,
    );

    assert_eq!(ir.modules.len(), 1);
    assert_eq!(ir.modules[0].exports, vec!["ApiController".to_string()]);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_diagnostic_for_non_identifier_extends_target() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export class ApiController extends buildBase() {}
"#,
    );

    assert_eq!(ir.diagnostics.len(), 1);
    assert_eq!(
        ir.diagnostics[0].code,
        "ANALYZER_UNSUPPORTED_CLASS_EXTENDS_EXPRESSION"
    );
    assert_eq!(
        ir.diagnostics[0].message,
        "class extends target must be a simple identifier in compiler mode"
    );
}

#[test]
fn supports_simple_public_class_fields() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export class ProfileController {
  retries = 3;
  label = "ok";
}
"#,
    );

    assert_eq!(ir.modules[0].exports, vec!["ProfileController".to_string()]);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_diagnostic_for_computed_public_class_fields() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export class ProfileController {
  [dynamicName] = 1;
}
"#,
    );

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
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export class Cache {
  static VERSION = 1;
  static reset() {}
}
"#,
    );

    assert_eq!(ir.modules[0].exports, vec!["Cache".to_string()]);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn emits_diagnostic_for_computed_static_class_members() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        r#"
export class Cache {
  static [nameExpr] = 1;
}
"#,
    );

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
