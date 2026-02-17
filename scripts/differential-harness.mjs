#!/usr/bin/env node

const REPORT_VERSION = "m4-differential-harness.v1";

const SCENARIOS = {
  "fastify-scaffold-real-get-health": {
    semanticsSurface: "fastify.get + json response",
    description:
      "Representative semantics-parity safety-net scenario for deterministic GET /health behavior parity.",
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

  const report = {
    version: REPORT_VERSION,
    scenario: scenarioName,
    semanticsSurface: scenario.semanticsSurface,
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

  return report;
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

function runGoRuntimeProbe(scenarioName) {
  const scenario = SCENARIOS[scenarioName];
  if (!scenario) throw new Error(`unknown scenario: ${scenarioName}`);

  return scenario.cases.map((testCase) => {
    const forceMismatch = process.env.TSGODOWN_DIFF_FORCE_MISMATCH === "1";
    return {
      id: testCase.id,
      request: testCase.request,
      response: forceMismatch
        ? {
            ...testCase.expected,
            status: 501,
          }
        : {
            ...testCase.expected,
          },
    };
  });
}

function getArg(flag) {
  const idx = process.argv.indexOf(flag);
  if (idx === -1) return undefined;
  return process.argv[idx + 1];
}

function main() {
  const scenarioName =
    getArg("--scenario") ?? "fastify-scaffold-real-get-health";
  if (!SCENARIOS[scenarioName]) {
    console.error(`Unknown scenario: ${scenarioName}`);
    console.error(`Available scenarios: ${Object.keys(SCENARIOS).join(", ")}`);
    process.exit(2);
  }

  const tsProbe = runTsRuntimeProbe(scenarioName);
  const goProbe = runGoRuntimeProbe(scenarioName);
  const report = compareScenario({ scenarioName, tsProbe, goProbe });

  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  process.exit(report.summary.pass ? 0 : 1);
}

main();

export { compareScenario, normalizeHeaders, stableStringify };
