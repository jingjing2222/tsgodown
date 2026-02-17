# hono-scaffold-real

Official-style Hono workspace fixture to validate
`tsdown bundle + d.ts + sourcemap -> tsgodown -> Go` compile flow.

## Build Go output with tsgodown

```bash
pnpm install
pnpm run build:go
```

Output is emitted to `dist-go/main.go`.
