use analyzer_rust::analyze_fastify_entry;

#[test]
fn returns_empty_program_ir_in_compiler_mode_stub() {
    let ir = analyze_fastify_entry("src/index.ts", "fastify.get('/health', health)");

    assert!(ir.modules.is_empty());
    assert!(ir.routes.is_empty());
    assert!(ir.handlers.is_empty());
    assert!(ir.diagnostics.is_empty());
}
