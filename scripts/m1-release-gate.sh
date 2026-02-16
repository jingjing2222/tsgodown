#!/usr/bin/env bash
set -euo pipefail

# Canonical M1 release-gate verification path:
# fastify scaffold fixture -> dist-go/main.go scaffold -> go build (if Go is available)

pnpm --filter tsgodown run build
(
  cd packages/cli
  node --import tsx --test-name-pattern "^M1 release gate:" --test test/commands.e2e.test.ts
)