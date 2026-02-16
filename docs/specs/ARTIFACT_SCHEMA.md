# Artifact Schema (Draft)

## Required Inputs per build
- `bundle/*.js`
- `bundle/*.js.map`
- `types/**/*.d.ts`
- `manifest.json`

> `manifest.json`은 Rust build core 계약의 결과물로 취급한다.
> TS 계층은 오케스트레이션/표시만 수행하며, 분석 fallback 입력으로 TS analyzer를 사용하지 않는다.

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
