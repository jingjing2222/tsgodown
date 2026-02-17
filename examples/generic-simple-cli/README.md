# generic-simple-cli

Minimal framework-agnostic TypeScript CLI workspace used to validate
`tsdown bundle + d.ts + sourcemap -> tsgodown -> Go` compile flow.

## Build Go output with tsgodown

```bash
pnpm install
pnpm run build:go
```

Output is emitted to `dist-go/main.go`.
