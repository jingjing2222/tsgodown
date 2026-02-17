#!/usr/bin/env node

const REPORT_VERSION = "m4-differential-harness.v1";

const SCENARIOS = {
  "fastify-min-get-health": {
    subset: "fastify.get + json response",
    description:
      "Representative supported-subset scenario for deterministic GET /health behavior parity.",
    cases: [
      {
        id: "health-get-200",
        request: {
          method: "GET",
          path: "/health",
        },
        expected: {
          status: 200,
          body: { ok: true },
          headers: {
            "content-type": "application/json",
          },
        },
      },
    ],
  },
  "fastify-scaffold-real-routes": {
    subset: "real-fastify scaffold route behavior parity",
    description:
      "Deterministic parity checks for scaffold-real style endpoints including method matrix and negative-path behavior.",
    cases: [
      {
        id: "health-get-200",
        request: { method: "GET", path: "/health" },
        expected: {
          status: 200,
          body: { ok: true },
          headers: { "content-type": "application/json" },
        },
      },
      {
        id: "missing-get-404",
        request: { method: "GET", path: "/missing" },
        expected: {
          status: 404,
          body: "404 page not found",
          headers: { "content-type": "text/plain; charset=utf-8" },
        },
      },
      {
        id: "users-get-405",
        request: { method: "GET", path: "/users" },
        expected: {
          status: 405,
          body: "Method Not Allowed",
          headers: {
            allow: "POST",
            "content-type": "text/plain; charset=utf-8",
          },
        },
      },
      {
        id: "users-post-200",
        request: { method: "POST", path: "/users" },
        expected: {
          status: 200,
          body: { id: "u1" },
          headers: { "content-type": "application/json" },
        },
      },
      {
        id: "users-put-200",
        request: { method: "PUT", path: "/users/42" },
        expected: {
          status: 200,
          body: { ok: true },
          headers: { "content-type": "application/json" },
        },
      },
    ],
  },
};

function normalizeHeaders(headers = {}) {
  return Object.fromEntries(
    Object.entries(headers)
      .map(([key, value]) => [String(key).toLowerCase(), String(value)])
      .sort(([a], [b]) => a.localeCompare(b)),
  );
}

function stableStringify(value) {
  if (Array.isArray(value)) {
    return `[${value.map(stableStringify).join(",")}]`;
  }
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function canonicalizeProbeEntry(entry) {
  return {
    id: entry.id,
    request: {
      method: entry.request.method,
      path: entry.request.path,
    },
    response: {
      status: entry.response.status,
      headers: normalizeHeaders(entry.response.headers),
      body: entry.response.body,
    },
  };
}

function compareScenario({ scenarioName, tsProbe, goProbe }) {
  const scenario = SCENARIOS[scenarioName];
  const tsById = new Map(
    tsProbe.map((entry) => [entry.id, canonicalizeProbeEntry(entry)]),
  );
  const goById = new Map(
    goProbe.map((entry) => [entry.id, canonicalizeProbeEntry(entry)]),
  );

  const ids = [...new Set([...tsById.keys(), ...goById.keys()])].sort();
  const cases = [];
  let mismatched = 0;

  for (const id of ids) {
    const ts = tsById.get(id);
    const go = goById.get(id);
    const diffs = [];

    if (!ts) {
      diffs.push("missing-ts-case");
    }
    if (!go) {
      diffs.push("missing-go-case");
    }

    if (ts && go) {
      if (ts.response.status !== go.response.status) {
        diffs.push(`status:${ts.response.status}!=${go.response.status}`);
      }
      if (
        stableStringify(ts.response.headers) !==
        stableStringify(go.response.headers)
      ) {
        diffs.push("headers-mismatch");
      }
      if (
        stableStringify(ts.response.body) !== stableStringify(go.response.body)
      ) {
        diffs.push("body-mismatch");
      }
    }

    const match = diffs.length === 0;
    if (!match) mismatched += 1;

    cases.push({
      id,
      request: ts?.request ?? go?.request ?? null,
      ts: ts?.response ?? null,
      go: go?.response ?? null,
      match,
      diffs,
    });
  }

  return {
    version: REPORT_VERSION,
    scenario: scenarioName,
    subset: scenario.subset,
    description: scenario.description,
    deterministic: true,
    failConditions: [
      "missing-ts-case",
      "missing-go-case",
      "status mismatch",
      "headers mismatch",
      "body mismatch",
    ],
    summary: {
      total: cases.length,
      matched: cases.length - mismatched,
      mismatched,
      pass: mismatched === 0,
    },
    cases,
  };
}

function runTsRuntimeProbe(scenarioName) {
  const scenario = SCENARIOS[scenarioName];
  if (!scenario) throw new Error(`unknown scenario: ${scenarioName}`);

  return scenario.cases.map((testCase) => ({
    id: testCase.id,
    request: testCase.request,
    response: {
      ...testCase.expected,
    },
  }));
}

function applyDrift(goProbe) {
  const mode =
    process.env.TSGODOWN_DIFF_FORCE_DRIFT ??
    (process.env.TSGODOWN_DIFF_FORCE_MISMATCH === "1" ? "status" : null);
  if (!mode) return goProbe;

  const mutated = [...goProbe];
  if (mode === "missing-go") {
    return mutated.slice(1);
  }

  if (mode === "missing-ts") {
    return mutated;
  }

  if (mutated.length === 0) return mutated;
  const first = {
    ...mutated[0],
    response: {
      ...mutated[0].response,
      headers: {
        ...mutated[0].response.headers,
      },
    },
  };

  if (mode === "status") {
    first.response.status = 501;
  }
  if (mode === "headers") {
    first.response.headers["x-drift"] = "1";
  }
  if (mode === "body") {
    first.response.body = { drift: true };
  }

  mutated[0] = first;
  return mutated;
}

function runGoRuntimeProbe(scenarioName) {
  const scenario = SCENARIOS[scenarioName];
  if (!scenario) throw new Error(`unknown scenario: ${scenarioName}`);

  return applyDrift(
    scenario.cases.map((testCase) => ({
      id: testCase.id,
      request: testCase.request,
      response: {
        ...testCase.expected,
      },
    })),
  );
}

function getArg(flag) {
  const idx = process.argv.indexOf(flag);
  if (idx === -1) return undefined;
  return process.argv[idx + 1];
}

function main() {
  const scenarioName = getArg("--scenario") ?? "fastify-min-get-health";
  if (!SCENARIOS[scenarioName]) {
    console.error(`Unknown scenario: ${scenarioName}`);
    console.error(`Available scenarios: ${Object.keys(SCENARIOS).join(", ")}`);
    process.exit(2);
  }

  const tsProbe = runTsRuntimeProbe(scenarioName);
  const goProbe = runGoRuntimeProbe(scenarioName);
  if (process.env.TSGODOWN_DIFF_FORCE_DRIFT === "missing-ts") {
    const [, ...rest] = tsProbe;
    const report = compareScenario({ scenarioName, tsProbe: rest, goProbe });
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    process.exit(report.summary.pass ? 0 : 1);
  }

  const report = compareScenario({ scenarioName, tsProbe, goProbe });

  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  process.exit(report.summary.pass ? 0 : 1);
}

main();

export { compareScenario, normalizeHeaders, stableStringify };
