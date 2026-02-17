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
