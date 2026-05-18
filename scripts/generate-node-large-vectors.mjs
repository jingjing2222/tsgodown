#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-large");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);

const generators = {
  "express-app": httpVectors("express-app"),
  "nestjs-app": nestVectors,
  "fastify-app": httpVectors("fastify-app"),
  "koa-app": httpVectors("koa-app"),
  "hapi-app": httpVectors("hapi-app"),
  "vite-build": viteVectors,
  "rollup-build": rollupVectors,
  "webpack-build": webpackVectors,
  "next-app": nextVectors,
  "nuxt-app": nuxtVectors,
  "astro-app": astroVectors,
  "remix-app": remixVectors,
  "eslint-engine": eslintVectors,
  "prettier-engine": prettierVectors,
  "babel-core": babelVectors,
  "typescript-compiler": typescriptVectors,
  "graphql-engine": graphqlVectors,
  "apollo-server-app": apolloVectors,
  "socketio-app": socketIoVectors,
  "typeorm-app": typeormVectors,
};

for (const entry of manifest.entries) {
  const makeVectors = generators[entry.id];
  if (!makeVectors) {
    throw new Error(`missing vector generator for ${entry.id}`);
  }
  writeVectors(entry.id, makeVectors());
}

fs.writeFileSync(
  path.join(corpusRoot, "manifest.json"),
  `${JSON.stringify(
    {
      ...manifest,
      policy: { ...manifest.policy, status: "vectors-node-ready" },
      entries: manifest.entries.map((entry) => ({
        ...entry,
        vectors: { expected: 100, status: "node-ready" },
      })),
    },
    null,
    2,
  )}\n`,
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
        version: "node-large-corpus-vectors.v1",
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

function httpVectors(corpus) {
  return () => {
    const methods = ["GET", "POST", "PUT", "PATCH", "DELETE"];
    const modes = ["json", "headers", "query", "params", "error"];
    return Array.from({ length: 100 }, (_, index) => ({
      op: "http",
      corpus,
      method: pick(methods, index),
      mode: pick(modes, index + Math.floor(index / 10)),
      pathId: `item-${index}`,
      query: {
        q: `value ${index}`,
        tag: pick(["red", "green", "blue"], index),
      },
      payload: { index, nested: { ok: index % 2 === 0 } },
      headerValue: `trace-${index}`,
      status: 200 + (index % 5),
    }));
  };
}

function nestVectors() {
  const modes = ["factory", "asyncFactory", "override", "missing"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(modes, index),
    base: index,
    delta: (index % 7) + 1,
    label: `nest-${index}`,
  }));
}

function viteVectors() {
  const ops = ["normalizePath", "mergeConfig", "createFilter", "defineConfig"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    inputPath: pick(
      [
        "src\\entry.ts",
        "src/entry.ts",
        "/repo/pkg/src/app.ts",
        "C:\\repo\\pkg\\src\\app.ts",
      ],
      index,
    ),
    include: pick(["**/*.ts", "src/**", "**/*.{js,ts}"], index),
    exclude: pick(["**/*.test.ts", "dist/**", "node_modules/**"], index),
    mode: pick(["development", "production", "test"], index),
    defineValue: `__VALUE_${index}__`,
  }));
}

function rollupVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "bundle",
    inputId: `entry-${index}.js`,
    exportName: `value${index}`,
    source: `export const value${index} = ${index}; export default value${index} + 1;`,
    format: pick(["es", "cjs"], index),
  }));
}

function webpackVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "bundle",
    value: index,
    expression: `${index} + ${index % 11}`,
    libraryName: `bundle${index}`,
    devtool: pick([false, "source-map"], index),
  }));
}

function nextVectors() {
  const ops = ["normalizeConfig", "defaultConfig", "runtime", "postponed"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    distDir: `.next-${index}`,
    output: pick(["standalone", "export", undefined], index),
    runtime: pick(["nodejs", "edge", undefined], index),
    size: pick(["64", "128k", "2mb", "bad"], index),
  }));
}

function nuxtVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "defineConfig",
    ssr: index % 2 === 0,
    routeRules: {
      [`/item-${index}`]: { prerender: index % 3 === 0, swr: index % 5 },
    },
    runtimeConfig: {
      public: { value: `nuxt-${index}` },
    },
  }));
}

function astroVectors() {
  const ops = ["defineConfig", "mergeConfig", "envField"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    site: `https://example${index}.test`,
    base: `/base-${index % 5}`,
    output: pick(["static", "server", "hybrid"], index),
    envName: `PUBLIC_VALUE_${index}`,
  }));
}

function remixVectors() {
  const ops = ["generatePath", "matchRoutes", "createPath", "redirect"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    pattern: "/users/:id/files/:name",
    params: { id: String(index), name: `file-${index}.txt` },
    pathname: `/users/${index}/files/file-${index}.txt`,
    search: `?q=${index}`,
    hash: `#section-${index % 4}`,
  }));
}

function eslintVectors() {
  const ops = ["semi", "noUndef", "noUnused", "clean"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    code: pick(
      [
        "const a = 1\nconsole.log(a)\n",
        "missingReference()\n",
        "const unused = 1;\n",
        "const ok = 1;\nconsole.log(ok);\n",
      ],
      index,
    ),
  }));
}

function prettierVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "format",
    parser: pick(["babel", "typescript", "json", "markdown"], index),
    source: pick(
      [
        "const x={a:1,b:[2,3]}\n",
        "type User={id:number;name:string}\nconst user:User={id:1,name:'kim'}\n",
        '{"b":2,"a":[1,2]}',
        "# Title\n\n- a\n- b\n",
      ],
      index,
    ),
  }));
}

function babelVectors() {
  const ops = ["transform", "parseError", "plugin"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    code:
      index % 3 === 1
        ? "const = ;"
        : `const input${index} = () => ${index}; input${index}();`,
    renameFrom: `input${index}`,
    renameTo: `output${index}`,
  }));
}

function typescriptVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "transpile",
    module: pick(["CommonJS", "ESNext"], index),
    target: pick(["ES2019", "ES2022"], index),
    source: `export const value${index}: number = ${index};\nexport type Box${index} = { value: number };\n`,
  }));
}

function graphqlVectors() {
  const ops = ["execute", "validate", "parseError"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    field: `value${index}`,
    value: index,
    query:
      index % 3 === 2
        ? "query {"
        : `query($input:Int!){ value${index}(input:$input) }`,
  }));
}

function apolloVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "executeOperation",
    field: `value${index}`,
    value: index,
    variable: index + 1,
  }));
}

function socketIoVectors() {
  return Array.from({ length: 100 }, (_, index) => ({
    op: "ack",
    room: `room-${index % 5}`,
    event: `event-${index}`,
    payload: { index, ok: index % 2 === 0 },
  }));
}

function typeormVectors() {
  const ops = ["entitySchema", "dataSourceOptions", "metadataArgs"];
  return Array.from({ length: 100 }, (_, index) => ({
    op: pick(ops, index),
    entityName: `User${index}`,
    tableName: `users_${index}`,
    columnName: pick(["name", "email", "age"], index),
    nullable: index % 2 === 0,
  }));
}
