# tsdown Artifact Contract

This contract defines the `tsdown` output shapes that `tsgodown` must accept.
Completion means every supported artifact shape either lowers to backend-neutral
IR and generated Go parity, or fails closed before codegen with deterministic
diagnostics.

| Key | Artifact Shape | Contract Status | Go Status | Diagnostic | Evidence | Notes |
|---|---|---|---|---|---|---|
| tsdown.esm_bundle | ESM bundle | TODO | TODO | TSDOWN_ESM_BUNDLE_UNSUPPORTED | planned | Static ESM imports/exports and execution order. |
| tsdown.cjs_bundle | CJS bundle | TODO | TODO | TSDOWN_CJS_BUNDLE_UNSUPPORTED | planned | CommonJS wrapper, require, exports/module.exports. |
| tsdown.dual_package | Dual package output | TODO | TODO | TSDOWN_DUAL_PACKAGE_UNSUPPORTED | planned | ESM/CJS entry selection and parity. |
| tsdown.dts | `.d.ts` input | WIP | WIP | TSDOWN_DTS_UNSUPPORTED | existing compiler input contract | Symbol/type surface consumed for diagnostics/lowering. |
| tsdown.declaration_map | Declaration map input | TODO | TODO | TSDOWN_DECLARATION_MAP_UNSUPPORTED | planned | Original type location mapping. |
| tsdown.sourcemap | Source map input | WIP | WIP | TSDOWN_SOURCEMAP_UNSUPPORTED | existing diagnostics subset | Original JS/TS diagnostic locations. |
| tsdown.package_exports | package `exports` metadata | WIP | WIP | TSDOWN_PACKAGE_EXPORTS_UNSUPPORTED | corpus package graph subset | Conditional exports and subpath exports. |
| tsdown.package_imports | package `imports` metadata | TODO | TODO | TSDOWN_PACKAGE_IMPORTS_UNSUPPORTED | planned | Internal import maps. |
| tsdown.package_main_module_type | package `main`/`module`/`type` metadata | WIP | WIP | TSDOWN_PACKAGE_ENTRY_UNSUPPORTED | corpus package graph subset | Entry format selection. |
| tsdown.node_builtins | `node:` builtin imports | WIP | WIP | TSDOWN_NODE_BUILTIN_UNSUPPORTED | corpus subset | Must map to Node LTS ledger capabilities. |
| tsdown.json_modules | JSON modules | TODO | TODO | TSDOWN_JSON_MODULE_UNSUPPORTED | planned | Import attributes and JSON cache semantics. |
| tsdown.import_attributes | Import attributes | TODO | TODO | TSDOWN_IMPORT_ATTRIBUTES_UNSUPPORTED | planned | Attribute validation and resolution. |
| tsdown.dynamic_import | Dynamic import | TODO | TODO | TSDOWN_DYNAMIC_IMPORT_UNSUPPORTED | planned | Async module loading and errors. |
| tsdown.top_level_await | Top-level await | TODO | TODO | TSDOWN_TOP_LEVEL_AWAIT_UNSUPPORTED | planned | Module async evaluation order. |
| tsdown.code_splitting | Code splitting/chunks | TODO | TODO | TSDOWN_CODE_SPLITTING_UNSUPPORTED | planned | Multiple chunks and runtime loading graph. |
| tsdown.externals | External dependencies | TODO | TODO | TSDOWN_EXTERNAL_UNSUPPORTED | planned | External package boundary and fail-closed policy. |
| tsdown.assets | Asset/text imports | TODO | TODO | TSDOWN_ASSET_IMPORT_UNSUPPORTED | planned | Text/binary asset embedding or diagnostics. |
| tsdown.cli_shebang | Shebang/CLI entrypoints | WIP | WIP | TSDOWN_CLI_ENTRY_UNSUPPORTED | corpus CLI subset | argv/execPath and executable entry behavior. |
| tsdown.platform_target | Platform target metadata | TODO | TODO | TSDOWN_PLATFORM_TARGET_UNSUPPORTED | planned | Node/platform target constraints. |
| tsdown.package_manager | Package manager metadata | TODO | TODO | TSDOWN_PACKAGE_MANAGER_UNSUPPORTED | planned | Lockfile/package manager provenance. |
| tsdown.diagnostics_mapping | Sourcemap-mapped fail-closed diagnostics | WIP | WIP | TSDOWN_DIAGNOSTIC_MAPPING_UNSUPPORTED | existing diagnostics subset | Every unsupported row must produce deterministic location. |

## Gate Rules

- Required rows are enforced by `pnpm run gate:tsdown-artifact-contract`.
- `TODO` and `WIP` are allowed while developing.
- Final mode must reject `TODO` and `WIP`:

```bash
node scripts/check-ledger.mjs tsdown-artifact --final
```
