# Fastify unsupported diagnostic fixture matrix

This fixture set provides deterministic **bad/fixed pairs** for unsupported Fastify diagnostics.

## Naming scheme

- `fastify-unsupported-<topic>.bad.fixture.txt`
- `fastify-unsupported-<topic>.fixed.fixture.txt`

Where:
- `<topic>` maps to one unsupported boundary (or a route-object variant of the same code).
- `.bad.` fixture must emit one deterministic unsupported diagnostic.
- `.fixed.` fixture must emit no diagnostics and should extract stable routes.

## Pair mapping

- `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE`
  - `fastify-unsupported-conditional-route.bad.fixture.txt`
  - `fastify-unsupported-conditional-route.fixed.fixture.txt`
- `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`
  - `fastify-unsupported-register-callback.bad.fixture.txt`
  - `fastify-unsupported-register-callback.fixed.fixture.txt`
- `ANALYZER_UNSUPPORTED_DYNAMIC_PATH` (shorthand)
  - `fastify-unsupported-dynamic-path.bad.fixture.txt`
  - `fastify-unsupported-dynamic-path.fixed.fixture.txt`
- `ANALYZER_UNSUPPORTED_INLINE_HANDLER` (shorthand)
  - `fastify-unsupported-inline-handler.bad.fixture.txt`
  - `fastify-unsupported-inline-handler.fixed.fixture.txt`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`
  - `fastify-unsupported-route-object-shape.bad.fixture.txt`
  - `fastify-unsupported-route-object-shape.fixed.fixture.txt`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`
  - `fastify-unsupported-route-object-method.bad.fixture.txt`
  - `fastify-unsupported-route-object-method.fixed.fixture.txt`
- `ANALYZER_UNSUPPORTED_DYNAMIC_PATH` (route object)
  - `fastify-unsupported-route-object-path.bad.fixture.txt`
  - `fastify-unsupported-route-object-path.fixed.fixture.txt`
- `ANALYZER_UNSUPPORTED_INLINE_HANDLER` (route object)
  - `fastify-unsupported-route-object-handler.bad.fixture.txt`
  - `fastify-unsupported-route-object-handler.fixed.fixture.txt`

The matrix is consumed by:
- `packages/analyzer-rust/tests/fastify_ast_analyzer.rs`
  - `fixture_matrix_for_fastify_unsupported_diagnostics_bad_and_fixed_pairs`
