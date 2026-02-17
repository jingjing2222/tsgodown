#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLE_DIR="${ROOT_DIR}/examples/fastify-complex"
DIST_GO_DIR="${EXAMPLE_DIR}/dist-go"
BINARY_NAME="tsgodown-fastify-complex"
PORT="${FASTIFY_COMPLEX_PORT:-18081}"
BASE_URL="http://127.0.0.1:${PORT}"
SERVER_LOG="${ROOT_DIR}/.tmp-fastify-complex-server.log"

SERVER_PID=""

red() { printf "\033[31m%s\033[0m\n" "$*"; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
yellow() { printf "\033[33m%s\033[0m\n" "$*"; }
log() { printf "[fastify-complex] %s\n" "$*"; }

fail_with_hint() {
  local cause="$1"
  local fix="$2"
  red "[fastify-complex] FAIL cause: ${cause}"
  yellow "[fastify-complex] FIX hint: ${fix}"
  exit 1
}

teardown() {
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    log "stopping server (pid=${SERVER_PID})"
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
}

on_exit() {
  local exit_code="$?"
  teardown
  if [[ "${exit_code}" -ne 0 ]]; then
    yellow "[fastify-complex] diagnostics"
    printf "  - ROOT_DIR: %s\n" "${ROOT_DIR}"
    printf "  - EXAMPLE_DIR: %s\n" "${EXAMPLE_DIR}"
    printf "  - DIST_GO_DIR: %s\n" "${DIST_GO_DIR}"
    printf "  - TSGODOWN_RUST_ENGINE_BIN: %s\n" "${TSGODOWN_RUST_ENGINE_BIN:-<unset>}"
    printf "  - TSGODOWN_ENGINE_CORE_BIN: %s\n" "${TSGODOWN_ENGINE_CORE_BIN:-<unset>}"
    if [[ -f "${SERVER_LOG}" ]]; then
      yellow "[fastify-complex] server log tail (${SERVER_LOG})"
      tail -n 60 "${SERVER_LOG}" || true
    fi
  fi
  exit "${exit_code}"
}
trap on_exit EXIT

require_cmd() {
  local name="$1"
  command -v "${name}" >/dev/null 2>&1 || fail_with_hint "missing required command '${name}'" "Install '${name}' and re-run ./scripts/devx-fastify-complex.sh"
}

assert_route() {
  local method="$1"
  local path="$2"
  local expected_status="$3"
  local expected_fragment="$4"
  local tmp_body
  tmp_body="$(mktemp /tmp/fastify-complex-route.XXXXXX)"

  local status
  status="$(curl -sS -X "${method}" -o "${tmp_body}" -w "%{http_code}" "${BASE_URL}${path}")"
  local body
  body="$(cat "${tmp_body}")"
  rm -f "${tmp_body}"

  if [[ "${status}" != "${expected_status}" ]]; then
    fail_with_hint "${method} ${path} returned status=${status} (expected ${expected_status})" "Inspect ${SERVER_LOG}, then open generated ${DIST_GO_DIR}/main.go to confirm route wiring"
  fi

  if [[ -n "${expected_fragment}" ]] && [[ "${body}" != *"${expected_fragment}"* ]]; then
    fail_with_hint "${method} ${path} body mismatch (expected fragment '${expected_fragment}')" "Check emitted TODO handler text in ${DIST_GO_DIR}/main.go and ensure route handler names in src/index.ts are stable"
  fi

  log "asserted ${method} ${path} -> ${status}"
}

log "preflight"
require_cmd node
require_cmd pnpm
require_cmd cargo
require_cmd go
require_cmd curl

log "build workspace"
(
  cd "${ROOT_DIR}"
  pnpm run build
)

log "build engine-core"
(
  cd "${ROOT_DIR}"
  cargo build -p engine-core
)

export TSGODOWN_RUST_ENGINE_BIN="${TSGODOWN_RUST_ENGINE_BIN:-${ROOT_DIR}/scripts/rust-engine-launcher.sh}"
export TSGODOWN_ENGINE_CORE_BIN="${TSGODOWN_ENGINE_CORE_BIN:-${ROOT_DIR}/target/debug/engine-core}"

[[ -x "${TSGODOWN_RUST_ENGINE_BIN}" ]] || fail_with_hint "TSGODOWN_RUST_ENGINE_BIN is not executable (${TSGODOWN_RUST_ENGINE_BIN})" "Use scripts/rust-engine-launcher.sh or chmod +x your custom launcher"
[[ -x "${TSGODOWN_ENGINE_CORE_BIN}" ]] || fail_with_hint "TSGODOWN_ENGINE_CORE_BIN is not executable (${TSGODOWN_ENGINE_CORE_BIN})" "Run cargo build -p engine-core or set TSGODOWN_ENGINE_CORE_BIN to the built binary"

log "build fastify-complex -> dist-go"
(
  cd "${EXAMPLE_DIR}"
  rm -rf dist-go
  node --import tsx ../../packages/cli/src/index.ts build
)

[[ -f "${DIST_GO_DIR}/main.go" ]] || fail_with_hint "dist-go/main.go was not generated" "Run with DEBUG=1 and check CLI stderr for rust-engine adapter contract errors"

if [[ ! -f "${DIST_GO_DIR}/go.mod" ]]; then
  log "go.mod missing in dist-go; initializing temporary module"
  (
    cd "${DIST_GO_DIR}"
    go mod init example.com/tsgodown-fastify-complex >/dev/null 2>&1 || true
  )
fi

log "compile go runtime"
(
  cd "${DIST_GO_DIR}"
  go build -o "${BINARY_NAME}" .
)

if command -v lsof >/dev/null 2>&1 && lsof -nP -iTCP:"${PORT}" -sTCP:LISTEN >/dev/null 2>&1; then
  fail_with_hint "port ${PORT} is already in use" "Set FASTIFY_COMPLEX_PORT to a free port, e.g. FASTIFY_COMPLEX_PORT=19081 pnpm run devx:fastify-complex"
fi

log "run go runtime"
: >"${SERVER_LOG}"
(
  cd "${DIST_GO_DIR}"
  PORT="${PORT}" "./${BINARY_NAME}" >"${SERVER_LOG}" 2>&1
) &
SERVER_PID="$!"

ready=0
for _ in {1..50}; do
  code="$(curl -sS -o /dev/null -w "%{http_code}" "${BASE_URL}/health" || true)"
  if [[ "${code}" == "501" ]]; then
    ready=1
    break
  fi
  sleep 0.2
done

[[ "${ready}" == "1" ]] || fail_with_hint "go runtime did not become ready on ${BASE_URL}/health with expected status 501" "Check ${SERVER_LOG} for bind/runtime errors, or retry on a different FASTIFY_COMPLEX_PORT"

assert_route "GET" "/health" "501" "TODO implement handler healthHandler for GET /health"
assert_route "POST" "/users" "501" "TODO implement handler createUserHandler for POST /users"
assert_route "PUT" "/users/:id" "501" "TODO implement handler updateUserHandler for PUT /users/:id"
assert_route "DELETE" "/users/:id" "501" "TODO implement handler deleteUserHandler for DELETE /users/:id"
assert_route "GET" "/users" "405" "Method Not Allowed"
assert_route "GET" "/missing" "404" "404 page not found"

green "[fastify-complex] PASS"
