#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const REPORT_VERSION = "differential-fuzz-parity.v1";
const repoRoot = path.resolve(import.meta.dirname, "..");
const generatedRoot =
  process.env.TSGODOWN_DIFFERENTIAL_FUZZ_GO_ROOT ??
  path.join(repoRoot, "test-corpus", "differential-fuzz", "generated-go");

const cases = [...ecmascriptCases(), ...nodeApiCases(), ...holdoutCases()].map(
  (testCase, index) => ({
    id: `${testCase.group}-${String(index + 1).padStart(3, "0")}`,
    ...testCase,
  }),
);

const reports = cases.map((testCase) => {
  const node = runNode(testCase);
  const go = runGo(testCase);
  const parity =
    node.status === "passed" &&
    go.status === "passed" &&
    stableStringify(node.observed) === stableStringify(go.observed);
  return {
    id: testCase.id,
    group: testCase.group,
    capability: testCase.capability,
    node: stripObserved(node),
    go: stripObserved(go),
    parity: parity ? { status: "passed" } : { status: "blocked" },
  };
});

const summary = {
  total: reports.length,
  nodePassed: reports.filter((report) => report.node.status === "passed")
    .length,
  goPassed: reports.filter((report) => report.go.status === "passed").length,
  parityPassed: reports.filter((report) => report.parity.status === "passed")
    .length,
  groups: Object.fromEntries(
    [...new Set(reports.map((report) => report.group))]
      .sort()
      .map((group) => [
        group,
        reports.filter((report) => report.group === group).length,
      ]),
  ),
};

const report = {
  version: REPORT_VERSION,
  status: summary.parityPassed === summary.total ? "passed" : "blocked",
  nodeLts: "24.15.0",
  policy: {
    deterministicSeed: "tsgodown-differential-fuzz-v1",
    noPrecomputedExpected: true,
    noNodeFallbackForGo: true,
  },
  summary,
  cases: reports,
};

console.log(JSON.stringify(report, null, 2));
if (report.status !== "passed") {
  process.exit(1);
}

function runNode(testCase) {
  const result = spawnSync(
    process.execPath,
    ["--input-type=module", "-e", testCase.source],
    {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 16 * 1024 * 1024,
    },
  );
  const base = {
    status: result.status === 0 ? "passed" : "failed",
    exitCode: result.status,
    stderr: result.stderr,
  };
  if (result.status !== 0) return { ...base, stdout: result.stdout };
  try {
    const observed = JSON.parse(result.stdout);
    return { ...base, digest: digest(observed), observed };
  } catch (error) {
    return {
      ...base,
      status: "failed",
      stdout: result.stdout,
      parseError: error instanceof Error ? error.message : String(error),
    };
  }
}

function runGo(testCase) {
  const goDir = path.join(generatedRoot, testCase.id);
  if (!fs.existsSync(path.join(goDir, "go.mod"))) {
    return {
      status: "blocked",
      reason: "generated Go differential fuzz case missing",
      expectedPath: path.relative(repoRoot, goDir),
    };
  }
  return {
    status: "blocked",
    reason: "generated Go differential fuzz runner not wired yet",
  };
}

function ecmascriptCases() {
  return Array.from({ length: 60 }, (_, index) => {
    const value = index + 1;
    return {
      group: "ecmascript",
      capability: pick(
        [
          "scope-closure",
          "destructuring-spread",
          "class-prototype",
          "coercion-equality",
          "try-finally",
          "promise-order",
        ],
        index,
      ),
      source: printResult(`
        const log = [];
        const base = { value: ${value}, nested: { flag: ${value % 2 === 0} } };
        const clone = { ...base, extra: [${value}, ${value + 1}] };
        class Box {
          constructor(input) { this.input = input; }
          get doubled() { return this.input * 2; }
        }
        function closure(seed) {
          let state = seed;
          return (delta) => { state += delta; return state; };
        }
        const next = closure(${value});
        try {
          log.push(["try", next(1)]);
        } finally {
          log.push(["finally", new Box(${value}).doubled]);
        }
        await Promise.resolve().then(() => log.push(["microtask", clone.extra.at(-1)]));
        return {
          value: clone.value,
          flag: clone.nested.flag,
          eq: ${value} == "${value}",
          strict: ${value} === "${value}",
          log
        };
      `),
    };
  });
}

function nodeApiCases() {
  return Array.from({ length: 60 }, (_, index) => {
    const value = index + 1;
    return {
      group: "node-api",
      capability: pick(
        ["path-url-buffer-crypto", "events", "querystring", "fs-temp"],
        index,
      ),
      source: printResult(`
        const path = (await import("node:path")).default;
        const { URL } = await import("node:url");
        const { Buffer } = await import("node:buffer");
        const crypto = (await import("node:crypto")).default;
        const { EventEmitter } = await import("node:events");
        const querystring = (await import("node:querystring")).default;
        const fs = (await import("node:fs")).default;
        const os = (await import("node:os")).default;
        const url = new URL("/items/${value}?q=a+b", "https://example.test/base/");
        const emitter = new EventEmitter();
        const events = [];
        emitter.on("data", (payload) => events.push(payload));
        emitter.emit("data", { value: ${value} });
        const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-fuzz-"));
        const file = path.join(dir, "value.json");
        fs.writeFileSync(file, JSON.stringify({ value: ${value} }));
        const read = JSON.parse(fs.readFileSync(file, "utf8"));
        fs.rmSync(dir, { recursive: true, force: true });
        return {
          joined: path.posix.join("a", "b", "..", "c"),
          url: { pathname: url.pathname, q: url.searchParams.get("q") },
          buffer: Buffer.from("value-${value}").toString("base64"),
          hash: crypto.createHash("sha256").update("value-${value}").digest("hex").slice(0, 12),
          query: querystring.parse("a=1&a=2&b=${value}"),
          events,
          read
        };
      `),
    };
  });
}

function holdoutCases() {
  return Array.from({ length: 30 }, (_, index) => {
    const value = index + 1;
    return {
      group: "holdout",
      capability: pick(
        ["module-cycle-shape", "async-cli-shape", "object-library-shape"],
        index,
      ),
      source: printResult(`
        const registry = new Map();
        function define(name, factory) {
          registry.set(name, { factory, exports: {}, initialized: false });
        }
        function requireLocal(name) {
          const record = registry.get(name);
          if (!record.initialized) {
            record.initialized = true;
            record.factory(record.exports, requireLocal);
          }
          return record.exports;
        }
        define("a", (exports, require) => {
          exports.name = "a-${value}";
          exports.peer = () => require("b").name;
        });
        define("b", (exports, require) => {
          exports.name = "b-${value}";
          exports.peer = () => require("a").name;
        });
        const a = requireLocal("a");
        const b = requireLocal("b");
        return {
          argvShape: process.argv.slice(0, 1).length,
          cwdType: typeof process.cwd(),
          cycle: [a.name, a.peer(), b.peer()],
          objectKeys: Object.keys({ z: 1, a: 2, [String(${value})]: 3 })
        };
      `),
    };
  });
}

function printResult(bodySource) {
  return `
    const value = await (async () => {
      ${bodySource}
    })();
    console.log(JSON.stringify(value));
  `;
}

function pick(values, index) {
  return values[index % values.length];
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(value) {
  return crypto
    .createHash("sha256")
    .update(stableStringify(value))
    .digest("hex");
}

function stripObserved(result) {
  if (!result || !("observed" in result)) return result;
  const { observed: _observed, ...rest } = result;
  return rest;
}
