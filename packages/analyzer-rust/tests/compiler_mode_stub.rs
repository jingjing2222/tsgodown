use analyzer_rust::analyze_compiler_entry;

#[test]
fn returns_empty_program_ir_for_compiler_mode_stub() {
    let ir = analyze_compiler_entry(
        "src/index.ts",
        "export const health = () => ({ ok: true });",
    );

    assert!(ir.modules.is_empty());
    assert!(ir.routes.is_empty());
    assert!(ir.handlers.is_empty());
    assert!(ir.diagnostics.is_empty());
}
