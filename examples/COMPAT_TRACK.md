# Examples Compatibility Track

The examples workspace is split into:

- default track: baseline compiler/gate references used in regular checks
- compat track: framework-shaped reference examples kept as optional compatibility samples

Current compat-track examples:

- `examples/fastify-scaffold-real`
- `examples/hono-scaffold-real`

Install-first gate behavior:

- default run (`pnpm run examples:install-first:check`) excludes compat examples
- include compat examples by setting `TSGODOWN_INCLUDE_COMPAT_EXAMPLES=1`
