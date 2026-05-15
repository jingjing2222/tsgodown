#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-real");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);

function writeVectors(id, vectors) {
  if (vectors.length !== 100) {
    throw new Error(`${id} generated ${vectors.length} vectors, expected 100`);
  }
  const dir = path.join(corpusRoot, "cases", id);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(
    path.join(dir, "vectors.json"),
    `${JSON.stringify(
      {
        version: "node-corpus-vectors.v1",
        corpus: id,
        cases: vectors.map((vector, index) => ({
          id: `${id}-${String(index + 1).padStart(3, "0")}`,
          ...vector,
        })),
      },
      null,
      2,
    )}\n`,
  );
}

function pick(values, index) {
  return values[index % values.length];
}

function semverVectors() {
  const versions = [
    "0.0.0",
    "0.1.0",
    "1.0.0",
    "1.2.3",
    "1.2.3-beta.2",
    "1.2.3-beta.10",
    "1.10.0",
    "2.0.0",
    "10.4.3",
    "bad",
  ];
  const validVersions = versions.filter((version) => version !== "bad");
  const ranges = ["^1.2.0", "~1.2.0", ">=1.0.0 <2", "*", ">=2"];
  return Array.from({ length: 100 }, (_, index) => {
    const op = pick(["valid", "compare", "satisfies", "sort"], index);
    if (op === "valid") return { op, value: pick(versions, index) };
    if (op === "compare") {
      return {
        op,
        left: pick(validVersions, index + 1),
        right: pick(validVersions, index + 4),
      };
    }
    if (op === "satisfies") {
      return {
        op,
        value: pick(versions, index + 2),
        range: pick(ranges, index),
      };
    }
    return {
      op,
      values: [0, 1, 2, 3].map((offset) => pick(validVersions, index + offset)),
    };
  });
}

function minimatchVectors() {
  const paths = [
    "index.js",
    "index.ts",
    "src/a/b/index.ts",
    ".env",
    "src/app.test.ts",
    "src/app.spec.ts",
    "dist/app.ts",
    "src/app.ts",
    "src\\app.ts",
    "README.md",
  ];
  const patterns = [
    "*.js",
    "*.ts",
    "src/**/*.ts",
    "*",
    "**/*.{test,spec}.ts",
    "!(dist)/**/*.ts",
    "src/**/[ai]*.ts",
    "**/*.md",
  ];
  const optionPool = [
    {},
    { dot: true },
    { nocase: true },
    { windowsPathsNoEscape: true },
    { matchBase: true },
  ];
  return Array.from({ length: 100 }, (_, index) => ({
    op: "match",
    path: pick(paths, index),
    pattern: pick(patterns, index + Math.floor(index / 10)),
    options: pick(optionPool, index),
  }));
}

function qsVectors() {
  const queries = [
    "user[name]=kim&user[roles][]=admin&user[roles][]=ops",
    "tag=a&tag=b&tag=c",
    "space=a%20b&reserved=%5Bvalue%5D",
    "a[b][c]=1&a[b][d]=2",
    "arr[0]=zero&arr[2]=two",
    "empty=&bool=true&num=42",
    "encoded=%ED%95%9C%EA%B8%80",
    "semi=a%3Bb&plus=a%2Bb",
  ];
  const objects = [
    { user: { name: "kim", roles: ["admin", "ops"] } },
    { tag: ["a", "b", "c"] },
    { space: "a b", reserved: "[value]" },
    { a: { b: { c: 1, d: 2 } } },
    { arr: ["zero", "one", "two"] },
    { empty: "", bool: true, num: 42 },
    { encoded: "한글" },
    { semi: "a;b", plus: "a+b" },
  ];
  const stringifyOptions = [
    {},
    { encodeValuesOnly: true },
    { arrayFormat: "repeat" },
    { arrayFormat: "indices" },
  ];
  return Array.from({ length: 100 }, (_, index) =>
    index % 2 === 0
      ? { op: "parse", query: pick(queries, index) }
      : {
          op: "stringify",
          value: pick(objects, index),
          options: pick(stringifyOptions, index),
        },
  );
}

function dotenvVectors() {
  const lines = [
    "PLAIN=value",
    'QUOTED="hello world"',
    "COMMENTED=ok # trailing comment",
    'ESCAPED="line\\nnext"',
    "EMPTY=",
    "NUM=42",
    "SPACED = spaced",
    "EXPORT_ME=exported",
  ];
  return Array.from({ length: 100 }, (_, index) => {
    const envText = [0, 1, 2, 3]
      .map((offset) => pick(lines, index + offset))
      .join("\n");
    return index % 3 === 0
      ? { op: "parse", envText }
      : {
          op: "config",
          envText,
          initialEnv: { PLAIN: index % 2 === 0 ? "existing" : undefined },
          override: index % 4 === 0,
        };
  });
}

function yargsParserVectors() {
  const argvs = [
    ["--name", "kim", "-abc", "--count", "3", "pos1"],
    ["--tag", "red", "--tag", "green", "--no-cache"],
    ["--", "--literal", "value"],
    ["-x", "1", "-y", "2", "--flag"],
    ["--nested.value", "ok", "--arr", "a", "--arr", "b"],
  ];
  const configs = [
    {
      alias: { name: ["n"] },
      array: ["tag"],
      boolean: ["cache", "a", "b", "c"],
      number: ["count"],
      configuration: { "populate--": true, "strip-aliased": true },
    },
    { array: ["arr", "tag"], boolean: ["flag", "cache"], number: ["x", "y"] },
    { configuration: { "dot-notation": true, "populate--": true } },
  ];
  return Array.from({ length: 100 }, (_, index) => ({
    op: "parse",
    argv: pick(argvs, index),
    options: pick(configs, index),
  }));
}

function yamlVectors() {
  const docs = [
    "name: tsgodown\nitems:\n  - id: 1\n    ok: true",
    "- a\n- b\n- c",
    "nested:\n  value: hello\n  count: 2",
    'quoted: "hello world"\nempty: null',
    "date: 2026-05-15\nnum: 42",
  ];
  const values = [
    { name: "tsgodown", enabled: true, count: 2 },
    ["a", "b", "c"],
    { nested: { value: "hello", count: 2 } },
    { quoted: "hello world", empty: null },
  ];
  return Array.from({ length: 100 }, (_, index) => {
    const op = pick(["load", "dump", "invalid"], index);
    if (op === "load") return { op, source: pick(docs, index) };
    if (op === "dump") return { op, value: pick(values, index) };
    return { op, source: `key: [unterminated-${index}` };
  });
}

function lruCacheVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "sequence",
    options: { max: 2 + (index % 3) },
    steps: [
      { op: "set", key: `k${index % 5}`, value: index },
      { op: "set", key: `k${(index + 1) % 5}`, value: index + 1 },
      { op: "get", key: `k${index % 5}` },
      { op: "set", key: `k${(index + 2) % 5}`, value: index + 2 },
      { op: "has", key: `k${(index + 1) % 5}` },
      { op: "entries" },
    ],
  }));
}

function uuidVectors() {
  const values = [
    "6fa459ea-ee8a-3ca4-894e-db77e160355e",
    "00000000-0000-4000-8000-000000000000",
    "not-a-uuid",
    "6ba7b811-9dad-11d1-80b4-00c04fd430c8",
  ];
  return Array.from({ length: 100 }, (_, index) => {
    const op = pick(
      ["validate", "version", "roundTrip", "v5", "v4Shape"],
      index,
    );
    if (op === "v5") return { op, name: `tsgodown-${index}` };
    if (op === "v4Shape") return { op };
    return { op, value: pick(values, index) };
  });
}

function fsExtraVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "recipe",
    files: [
      {
        path: `source/data-${index}.json`,
        json: { name: "tsgodown", index, tags: [`t${index % 3}`, "go"] },
      },
      {
        path: `source/nested/info-${index % 7}.json`,
        json: { ok: true, value: index * 2 },
      },
    ],
    copyFrom: "source",
    copyTo: "target",
    remove: index % 2 === 0 ? "source" : "target",
  }));
}

function execaVectors() {
  return Array.from({ length: 100 }, (_, index) => {
    const fail = index % 5 === 0;
    return {
      op: "nodeEval",
      code: fail
        ? `console.error("bad-${index}"); process.exit(${1 + (index % 7)})`
        : `console.log((process.env.TSGODOWN_VECTOR || "") + ":" + process.argv[1])`,
      args: fail ? [] : [`argv-${index}`],
      env: { TSGODOWN_VECTOR: `ok-${index}` },
      expectFailure: fail,
    };
  });
}

const generators = {
  dotenv: dotenvVectors,
  execa: execaVectors,
  "fs-extra": fsExtraVectors,
  "js-yaml": yamlVectors,
  "lru-cache": lruCacheVectors,
  minimatch: minimatchVectors,
  qs: qsVectors,
  semver: semverVectors,
  uuid: uuidVectors,
  "yargs-parser": yargsParserVectors,
};

for (const testCase of manifest.cases) {
  const generator = generators[testCase.id];
  if (!generator) {
    throw new Error(`missing vector generator for ${testCase.id}`);
  }
  writeVectors(testCase.id, generator());
}

console.log(
  JSON.stringify({
    version: "node-corpus-vectors-generate.v1",
    generated: manifest.cases.map((testCase) => ({
      id: testCase.id,
      vectors: 100,
      path: path.relative(
        repoRoot,
        path.join(corpusRoot, "cases", testCase.id, "vectors.json"),
      ),
    })),
  }),
);
