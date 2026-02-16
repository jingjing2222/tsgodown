#!/usr/bin/env bash
set -euo pipefail

# Canonical M1 release-gate verification path:
# fastify scaffold fixture -> dist-go/main.go scaffold -> go build (if Go is available)

pnpm --filter tsgodown exec node --import tsx --test test/commands.e2e.test.ts --test-name-pattern "^M1 release gate:"