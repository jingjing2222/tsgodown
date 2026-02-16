# Artifact Schema (Draft)

## Required Inputs per build
- `bundle/*.js`
- `bundle/*.js.map`
- `types/**/*.d.ts`
- `manifest.json`

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
