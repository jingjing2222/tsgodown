#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLE_DIR="${ROOT_DIR}/examples/generic-simple-cli"
DIST_GO_DIR="${EXAMPLE_DIR}/dist-go"
RUN_OUTPUT="${ROOT_DIR}/.tmp-smoke-m1-run.out"
RUN_ERROR="${ROOT_DIR}/.tmp-smoke-m1-run.err"

red() { printf "\033[31m%s\033[0m\n" "$*"; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
yellow() { printf "\033[33m%s\033[0m\n" "$*"; }
log() { printf "[smoke-m1] %s\n" "$*"; }

teardown() {
  rm -f "${RUN_OUTPUT}" "${RUN_ERROR}"
}

failure_diagnostics() {
  red "[smoke-m1] FAILURE"
  yellow "[smoke-m1] diagnostics:"
  printf "  - ROOT_DIR: %s\n" "${ROOT_DIR}"
  printf "  - EXAMPLE_DIR: %s\n" "${EXAMPLE_DIR}"
  printf "  - DIST_GO_DIR: %s\n" "${DIST_GO_DIR}"
  printf "  - TSGODOWN_RUST_ENGINE_BIN: %s\n" "${TSGODOWN_RUST_ENGINE_BIN:-<unset>}"

  if [[ -f "${RUN_OUTPUT}" ]]; then
    yellow "[smoke-m1] binary stdout (${RUN_OUTPUT}):"
    cat "${RUN_OUTPUT}" || true
  fi

  if [[ -f "${RUN_ERROR}" ]]; then
    yellow "[smoke-m1] binary stderr (${RUN_ERROR}):"
    cat "${RUN_ERROR}" || true
  fi

  if [[ -f "${DIST_GO_DIR}/main.go" ]]; then
    yellow "[smoke-m1] dist-go/main.go head:"
    sed -n '1,120p' "${DIST_GO_DIR}/main.go" || true
  fi
}

on_exit() {
  local exit_code="$?"
  teardown
  if [[ "${exit_code}" -ne 0 ]]; then
    failure_diagnostics
  fi
  exit "${exit_code}"
}
trap on_exit EXIT

require_cmd() {
  local name="$1"
  if ! command -v "${name}" >/dev/null 2>&1; then
    red "[smoke-m1] missing required command: ${name}"
    exit 1
  fi
}

log "preflight: command checks"
require_cmd node
require_cmd pnpm
require_cmd cargo
require_cmd go

log "preflight: versions"
log "node=$(node -v)"
log "pnpm=$(pnpm -v)"
log "cargo=$(cargo --version)"
log "go=$(go version)"

log "build: TypeScript packages"
(
  cd "${ROOT_DIR}"
  pnpm run build
)

log "build: Rust engine-core"
(
  cd "${ROOT_DIR}"
  cargo build -p engine-core
)

# Required env for current adapter contract:
# TSGODOWN_RUST_ENGINE_BIN must accept JSON on stdin and print JSON on stdout.
if [[ -z "${TSGODOWN_RUST_ENGINE_BIN:-}" ]]; then
  export TSGODOWN_RUST_ENGINE_BIN="${ROOT_DIR}/scripts/rust-engine-launcher.sh"
  yellow "[smoke-m1] TSGODOWN_RUST_ENGINE_BIN was unset; using local launcher: ${TSGODOWN_RUST_ENGINE_BIN}"
fi

if [[ ! -x "${TSGODOWN_RUST_ENGINE_BIN}" ]]; then
  red "[smoke-m1] TSGODOWN_RUST_ENGINE_BIN is not executable: ${TSGODOWN_RUST_ENGINE_BIN}"
  exit 1
fi

log "build: generic-simple-cli -> dist-go"
(
  cd "${EXAMPLE_DIR}"
  rm -rf dist-go
  node --import tsx ../../packages/cli/src/index.ts build
)

if [[ ! -f "${DIST_GO_DIR}/main.go" ]]; then
  red "[smoke-m1] build did not produce ${DIST_GO_DIR}/main.go"
  exit 1
fi

if [[ ! -f "${DIST_GO_DIR}/tsgodownrt/runtime.go" ]]; then
  red "[smoke-m1] build did not produce ${DIST_GO_DIR}/tsgodownrt/runtime.go"
  exit 1
fi

log "build: go binary"
(
  cd "${DIST_GO_DIR}"
  go build -o tsgodown-local .
)

log "run: generated binary fails closed deterministically"
set +e
(
  cd "${DIST_GO_DIR}"
  ./tsgodown-local >"${RUN_OUTPUT}" 2>"${RUN_ERROR}"
)
run_status="$?"
set -e

if [[ "${run_status}" -ne 1 ]]; then
  red "[smoke-m1] expected generated binary exit 1 while fail-closed; got ${run_status}"
  exit 1
fi

if ! grep -q '"unsupported":true' "${RUN_OUTPUT}"; then
  red "[smoke-m1] fail-closed output missing unsupported=true"
  exit 1
fi

if ! grep -q 'EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED' "${RUN_OUTPUT}"; then
  red "[smoke-m1] fail-closed output missing executable JS codegen diagnostic"
  exit 1
fi

green "[smoke-m1] PASS"
