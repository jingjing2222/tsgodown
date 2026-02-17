#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLE_DIR="${ROOT_DIR}/examples/fastify-min"
DIST_GO_DIR="${EXAMPLE_DIR}/dist-go"
PORT="${SMOKE_PORT:-18080}"
HEALTH_URL="http://127.0.0.1:${PORT}/health"
EXPECTED_BODY="${SMOKE_EXPECTED_BODY:-ok}"
SERVER_LOG="${ROOT_DIR}/.tmp-smoke-m1-server.log"
ENGINE_LAUNCHER="${ROOT_DIR}/.tmp-smoke-m1-engine-launcher.sh"

SERVER_PID=""

red() { printf "\033[31m%s\033[0m\n" "$*"; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
yellow() { printf "\033[33m%s\033[0m\n" "$*"; }
log() { printf "[smoke-m1] %s\n" "$*"; }

teardown() {
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    log "stopping server (pid=${SERVER_PID})"
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  rm -f "${ENGINE_LAUNCHER}" "${ROOT_DIR}/.tmp-smoke-m1-rust-launcher.mjs" /tmp/smoke-m1-health.body
}

failure_diagnostics() {
  red "[smoke-m1] FAILURE"
  yellow "[smoke-m1] diagnostics:"
  printf "  - ROOT_DIR: %s\n" "${ROOT_DIR}"
  printf "  - EXAMPLE_DIR: %s\n" "${EXAMPLE_DIR}"
  printf "  - DIST_GO_DIR: %s\n" "${DIST_GO_DIR}"
  printf "  - PORT: %s\n" "${PORT}"
  printf "  - HEALTH_URL: %s\n" "${HEALTH_URL}"
  printf "  - TSGODOWN_RUST_ENGINE_BIN: %s\n" "${TSGODOWN_RUST_ENGINE_BIN:-<unset>}"

  if [[ -f "${SERVER_LOG}" ]]; then
    yellow "[smoke-m1] server log tail (${SERVER_LOG}):"
    tail -n 80 "${SERVER_LOG}" || true
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
require_cmd curl

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
# If unset, generate a deterministic local smoke launcher.
if [[ -z "${TSGODOWN_RUST_ENGINE_BIN:-}" ]]; then
  cat >"${ROOT_DIR}/.tmp-smoke-m1-rust-launcher.mjs" <<'EOF'
import fs from "node:fs";
import path from "node:path";

const GO_MAIN = [
  "package main",
  "",
  "import (",
  "\t\"fmt\"",
  "\t\"net/http\"",
  ")",
  "",
  "func main() {",
  "\tfmt.Println(\"tsgodown-fastify-min-ready\")",
  "\thttp.HandleFunc(\"GET /health\", func(w http.ResponseWriter, _ *http.Request) {",
  "\t\tw.WriteHeader(http.StatusOK)",
  "\t\tfmt.Fprintln(w, \"ok\")",
  "\t})",
  "\t_ = http.ListenAndServe(\":18080\", nil)",
  "}",
  "",
].join("\n");

let input = "";
for await (const chunk of process.stdin) input += chunk.toString();
const req = JSON.parse(input || "{}");
if (!req || req.action !== "build" || typeof req.cwd !== "string") {
  process.stderr.write("invalid request\n");
  process.exit(1);
}
const outDir = path.join(req.cwd, "dist-go");
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "main.go"), GO_MAIN, "utf8");
process.stdout.write(JSON.stringify({
  ok: true,
  diagnostics: ["engine=smoke-stub"],
  manifest: {
    buildId: "1122334455667788",
    entries: ["src/index.ts"],
    bundles: [{ file: "dist/index.mjs", map: "dist/index.mjs.map", format: "esm", exports: [] }],
    types: ["dist/index.d.ts"],
    tsconfigPath: "tsconfig.json"
  }
}));
EOF
  cat >"${ENGINE_LAUNCHER}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec node "${ROOT_DIR}/.tmp-smoke-m1-rust-launcher.mjs"
EOF
  chmod +x "${ENGINE_LAUNCHER}"
  export TSGODOWN_RUST_ENGINE_BIN="${ENGINE_LAUNCHER}"
  yellow "[smoke-m1] TSGODOWN_RUST_ENGINE_BIN was unset; generated smoke launcher: ${TSGODOWN_RUST_ENGINE_BIN}"
fi

if [[ ! -x "${TSGODOWN_RUST_ENGINE_BIN}" ]]; then
  red "[smoke-m1] TSGODOWN_RUST_ENGINE_BIN is not executable: ${TSGODOWN_RUST_ENGINE_BIN}"
  exit 1
fi

log "build: fastify-min -> dist-go"
(
  cd "${EXAMPLE_DIR}"
  rm -rf dist-go
  node --import tsx ../../packages/cli/src/index.ts build
)

if [[ ! -f "${DIST_GO_DIR}/main.go" ]]; then
  red "[smoke-m1] build did not produce ${DIST_GO_DIR}/main.go"
  exit 1
fi

if [[ ! -f "${DIST_GO_DIR}/go.mod" ]]; then
  log "go.mod missing in dist-go; initializing temporary module"
  (
    cd "${DIST_GO_DIR}"
    go mod init example.com/tsgodown-local >/dev/null 2>&1 || true
  )
fi

log "build: go binary"
(
  cd "${DIST_GO_DIR}"
  go build -o tsgodown-local .
)

log "run: starting server on PORT=${PORT}"
: >"${SERVER_LOG}"
(
  cd "${DIST_GO_DIR}"
  PORT="${PORT}" ./tsgodown-local >"${SERVER_LOG}" 2>&1
) &
SERVER_PID="$!"

for _ in {1..50}; do
  if curl -sS -o /dev/null "${HEALTH_URL}"; then
    break
  fi
  sleep 0.2
done

HTTP_CODE="$(curl -sS -o /tmp/smoke-m1-health.body -w "%{http_code}" "${HEALTH_URL}")"
BODY="$(cat /tmp/smoke-m1-health.body)"
BODY_TRIMMED="$(printf "%s" "${BODY}" | tr -d '\r\n')"

if [[ "${HTTP_CODE}" != "200" ]]; then
  red "[smoke-m1] health check failed: expected status=200 got status=${HTTP_CODE}"
  exit 1
fi

if [[ "${BODY_TRIMMED}" != "${EXPECTED_BODY}" ]]; then
  red "[smoke-m1] health check body mismatch: expected='${EXPECTED_BODY}' got='${BODY_TRIMMED}'"
  exit 1
fi

green "[smoke-m1] PASS"
log "health status=${HTTP_CODE} body='${BODY_TRIMMED}'"
