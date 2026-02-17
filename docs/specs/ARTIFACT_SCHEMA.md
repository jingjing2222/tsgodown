# Artifact Schema (Draft)

## Required Inputs per build
- `bundle/*.js`
- `bundle/*.js.map`
- `types/**/*.d.ts`
- `manifest.json`

> `manifest.json` is treated as an artifact from the Rust build-core contract.
> The TS layer performs orchestration/presentation only, and does not use the TS analyzer as fallback input.

## manifest.json (draft)
```json
{
  "buildId": "string",
  "entries": ["src/index.ts"],
  "bundles": [
    {
      "file": "dist/index.js",
      "map": "dist/index.js.map",
      "format": "esm|cjs",
      "exports": ["start", "handler"]
    }
  ],
  "types": ["dist/index.d.ts"],
  "tsconfigPath": "tsconfig.json"
}
```

## Indexer Output (draft)
```json
{
  "symbols": [],
  "sourceMapLinks": [],
  "unresolved": [],
  "diagnostics": []
}
```
