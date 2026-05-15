import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const moduleCache = new Map();

export async function runVectorCase(corpus, vector) {
  try {
    return {
      ok: true,
      value: await runVectorCaseUnsafe(corpus, vector),
    };
  } catch (error) {
    return {
      ok: false,
      error: normalizeError(error),
    };
  }
}

async function runVectorCaseUnsafe(corpus, vector) {
  switch (corpus) {
    case "express-app":
      return runExpress(vector);
    case "nestjs-app":
      return runNest(vector);
    case "fastify-app":
      return runFastify(vector);
    case "koa-app":
      return runKoa(vector);
    case "hapi-app":
      return runHapi(vector);
    case "vite-build":
      return runVite(vector);
    case "rollup-build":
      return runRollup(vector);
    case "webpack-build":
      return runWebpack(vector);
    case "next-app":
      return runNext(vector);
    case "nuxt-app":
      return runNuxt(vector);
    case "astro-app":
      return runAstro(vector);
    case "remix-app":
      return runRemix(vector);
    case "eslint-engine":
      return runEslint(vector);
    case "prettier-engine":
      return runPrettier(vector);
    case "babel-core":
      return runBabel(vector);
    case "typescript-compiler":
      return runTypescript(vector);
    case "graphql-engine":
      return runGraphql(vector);
    case "apollo-server-app":
      return runApollo(vector);
    case "socketio-app":
      return runSocketIo(vector);
    case "typeorm-app":
      return runTypeorm(vector);
    default:
      throw new Error(`unknown large corpus ${corpus}`);
  }
}

async function runExpress(vector) {
  const express = require("../packages/express");
  const app = express();
  const events = [];
  app.use(express.json());
  app.use((req, res, next) => {
    events.push(`mw:${req.method}`);
    res.setHeader("x-vector", vector.headerValue);
    next();
  });
  app.all("/items/:id", (req, res, next) => {
    if (vector.mode === "error") {
      next(new Error(`express-${vector.pathId}`));
      return;
    }
    res.status(vector.status).json({
      id: req.params.id,
      query: req.query,
      body: req.body ?? null,
      events,
    });
  });
  app.use((error, _req, res, _next) => {
    res.status(500).json({ name: error.name, message: error.message, events });
  });
  return requestHttp(app, vector);
}

async function runFastify(vector) {
  const fastify = require("../packages/fastify");
  const app = fastify({ logger: false });
  const events = [];
  app.addHook("onRequest", async (request, reply) => {
    events.push(`hook:${request.method}`);
    reply.header("x-vector", vector.headerValue);
  });
  app.route({
    method: vector.method,
    url: "/items/:id",
    async handler(request, reply) {
      if (vector.mode === "error") {
        throw new Error(`fastify-${vector.pathId}`);
      }
      reply.code(vector.status);
      return {
        id: request.params.id,
        query: request.query,
        body: request.body ?? null,
        events,
      };
    },
  });
  app.setErrorHandler((error, _request, reply) => {
    reply.code(500).send({ name: error.name, message: error.message, events });
  });
  try {
    const result = await app.inject({
      method: vector.method,
      url: `/items/${vector.pathId}?${new URLSearchParams(vector.query)}`,
      headers: { "content-type": "application/json" },
      payload: JSON.stringify(vector.payload),
    });
    return normalizeHttpResult({
      status: result.statusCode,
      headers: result.headers,
      body: result.body,
    });
  } finally {
    await app.close();
  }
}

async function runKoa(vector) {
  const Koa = require("../packages/koa");
  const app = new Koa();
  const events = [];
  app.use(async (ctx, next) => {
    events.push(`mw:${ctx.method}`);
    ctx.set("x-vector", vector.headerValue);
    await next();
  });
  app.use(async (ctx) => {
    const id = ctx.path.split("/").at(-1);
    if (vector.mode === "error") {
      ctx.status = 500;
      ctx.body = { name: "Error", message: `koa-${vector.pathId}`, events };
      return;
    }
    ctx.status = vector.status;
    ctx.body = { id, query: ctx.query, events };
  });
  return requestHttp(app.callback(), vector);
}

async function runHapi(vector) {
  const Hapi = require("../packages/hapi-hapi");
  const server = Hapi.server();
  const events = [];
  server.ext("onRequest", (request, h) => {
    events.push(`ext:${request.method.toUpperCase()}`);
    return h.continue;
  });
  server.route({
    method: vector.method,
    path: "/items/{id}",
    handler(request, h) {
      const response =
        vector.mode === "error"
          ? h
              .response({
                name: "Error",
                message: `hapi-${vector.pathId}`,
                events,
              })
              .code(500)
          : h
              .response({
                id: request.params.id,
                query: request.query,
                payload: request.payload ?? null,
                events,
              })
              .code(vector.status);
      return response.header("x-vector", vector.headerValue);
    },
  });
  const result = await server.inject({
    method: vector.method,
    url: `/items/${vector.pathId}?${new URLSearchParams(vector.query)}`,
    headers: { "content-type": "application/json" },
    payload: vector.payload,
  });
  return normalizeHttpResult({
    status: result.statusCode,
    headers: result.headers,
    body: result.payload,
  });
}

async function runNest(vector) {
  require("reflect-metadata");
  const { NestFactory } = require("@nestjs/core");
  const { Module } = require("@nestjs/common");
  class AppModule {}
  const providers = [
    { provide: "BASE", useValue: vector.base },
    {
      provide: "CALC",
      useFactory(base) {
        return {
          run() {
            if (vector.op === "missing") {
              throw new Error(`nest-${vector.label}`);
            }
            return {
              mode: vector.op,
              label: vector.label,
              value: base + vector.delta,
            };
          },
        };
      },
      inject: ["BASE"],
    },
  ];
  Module({ providers })(AppModule);
  const app = await NestFactory.createApplicationContext(AppModule, {
    logger: false,
  });
  try {
    return app.get("CALC").run();
  } catch (error) {
    return { error: normalizeError(error) };
  } finally {
    await app.close();
  }
}

async function runVite(vector) {
  const vite = require("../packages/vite/dist/node/index.js");
  if (vector.op === "normalizePath") {
    return vite.normalizePath(vector.inputPath);
  }
  if (vector.op === "mergeConfig") {
    return vite.mergeConfig(
      { mode: vector.mode, define: { __BASE__: "base" } },
      { define: { __VALUE__: vector.defineValue } },
    );
  }
  if (vector.op === "createFilter") {
    const filter = vite.createFilter(vector.include, vector.exclude);
    return {
      src: filter("src/app.ts"),
      test: filter("src/app.test.ts"),
      dist: filter("dist/app.ts"),
    };
  }
  return vite.defineConfig({
    mode: vector.mode,
    define: { __VALUE__: vector.defineValue },
  });
}

async function runRollup(vector) {
  const { rollup } = require("../packages/rollup/dist/rollup.js");
  const bundle = await rollup({
    input: vector.inputId,
    onwarn() {},
    plugins: [
      {
        name: "virtual",
        resolveId(id) {
          return id === vector.inputId ? id : null;
        },
        load(id) {
          return id === vector.inputId ? vector.source : null;
        },
      },
    ],
  });
  try {
    const generated = await bundle.generate({
      format: vector.format,
      exports: "named",
    });
    return generated.output.map((chunk) => ({
      type: chunk.type,
      exports: chunk.exports ?? [],
      codeIncludesExport: chunk.code.includes(vector.exportName),
    }));
  } finally {
    await bundle.close();
  }
}

async function runWebpack(vector) {
  const webpack = require("webpack");
  const root = mkdtempSync(join(tmpdir(), "tsgodown-webpack-vector-"));
  try {
    const entry = join(root, "entry.js");
    const outDir = join(root, "dist");
    writeFileSync(
      entry,
      `const value = ${vector.expression}; module.exports = { value };`,
    );
    const stats = await new Promise((resolve, reject) => {
      webpack(
        {
          mode: "none",
          entry,
          devtool: vector.devtool,
          output: {
            path: outDir,
            filename: "bundle.js",
            library: { name: vector.libraryName, type: "umd" },
          },
        },
        (error, result) => (error ? reject(error) : resolve(result)),
      );
    });
    const json = stats.toJson({ all: false, assets: true, errors: true });
    return {
      hasErrors: stats.hasErrors(),
      assets: (json.assets ?? []).map((asset) => asset.name),
      errors: (json.errors ?? []).map((error) => error.message),
    };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

async function runNext(vector) {
  const nextConfig = require("../packages/next/dist/server/config-shared.js");
  if (vector.op === "defaultConfig") {
    return {
      distDir: nextConfig.defaultConfig.distDir,
      trailingSlash: nextConfig.defaultConfig.trailingSlash,
    };
  }
  if (vector.op === "runtime") {
    return nextConfig.getNextConfigRuntime({
      experimental: {},
      serverRuntimeConfig: { value: vector.runtime },
      publicRuntimeConfig: { distDir: vector.distDir },
    });
  }
  if (vector.op === "postponed") {
    return {
      value: nextConfig.parseMaxPostponedStateSize(vector.size) ?? null,
    };
  }
  return nextConfig.normalizeConfig("phase-production-build", {
    distDir: vector.distDir,
    output: vector.output,
  });
}

async function runNuxt(vector) {
  const { defineNuxtConfig } = await importModule("../packages/nuxt/config.js");
  return defineNuxtConfig({
    ssr: vector.ssr,
    routeRules: vector.routeRules,
    runtimeConfig: vector.runtimeConfig,
  });
}

async function runAstro(vector) {
  const astro = await importModule(
    "../packages/astro/dist/config/entrypoint.js",
  );
  if (vector.op === "mergeConfig") {
    return astro.mergeConfig(
      { site: vector.site },
      { base: vector.base, output: vector.output },
    );
  }
  if (vector.op === "envField") {
    return astro.envField.string({
      context: "client",
      access: "public",
      default: vector.envName,
    });
  }
  return astro.defineConfig({
    site: vector.site,
    base: vector.base,
    output: vector.output,
  });
}

async function runRemix(vector) {
  const remix = require("../packages/remix-run-router/dist/router.cjs.js");
  if (vector.op === "generatePath") {
    return remix.generatePath(vector.pattern, vector.params);
  }
  if (vector.op === "matchRoutes") {
    return remix
      .matchRoutes(
        [{ path: "/users/:id/files/:name", id: "file" }],
        vector.pathname,
      )
      ?.map((match) => ({ id: match.route.id, params: match.params }));
  }
  if (vector.op === "createPath") {
    return remix.createPath({
      pathname: vector.pathname,
      search: vector.search,
      hash: vector.hash,
    });
  }
  const response = remix.redirect(vector.pathname, 302);
  return {
    status: response.status,
    location: response.headers.get("Location"),
  };
}

async function runEslint(vector) {
  const { ESLint } = require("eslint");
  const eslint = new ESLint({
    overrideConfigFile: true,
    overrideConfig: [
      {
        languageOptions: {
          ecmaVersion: 2022,
          sourceType: "module",
          globals: { console: "readonly" },
        },
        rules: {
          semi: vector.op === "semi" ? ["error", "always"] : "off",
          "no-undef": vector.op === "noUndef" ? "error" : "off",
          "no-unused-vars": vector.op === "noUnused" ? "error" : "off",
        },
      },
    ],
  });
  const [result] = await eslint.lintText(vector.code);
  return {
    errorCount: result.errorCount,
    messages: result.messages.map((message) => ({
      ruleId: message.ruleId,
      message: message.message,
    })),
  };
}

async function runPrettier(vector) {
  const prettier = require("../packages/prettier");
  try {
    return {
      formatted: await prettier.format(vector.source, {
        parser: vector.parser,
      }),
    };
  } catch (error) {
    return { error: normalizeError(error) };
  }
}

async function runBabel(vector) {
  const babel = require("../packages/babel-core");
  try {
    const result = babel.transformSync(vector.code, {
      ast: false,
      babelrc: false,
      configFile: false,
      plugins:
        vector.op === "plugin"
          ? [
              function renamePlugin() {
                return {
                  visitor: {
                    Identifier(path) {
                      if (path.node.name === vector.renameFrom) {
                        path.node.name = vector.renameTo;
                      }
                    },
                  },
                };
              },
            ]
          : [],
    });
    return { code: result.code };
  } catch (error) {
    return { error: normalizeError(error) };
  }
}

async function runTypescript(vector) {
  const ts = require("../packages/typescript");
  const result = ts.transpileModule(vector.source, {
    compilerOptions: {
      module: ts.ModuleKind[vector.module],
      target: ts.ScriptTarget[vector.target],
      declaration: true,
    },
    reportDiagnostics: true,
  });
  return {
    outputText: result.outputText,
    diagnostics: (result.diagnostics ?? []).map(
      (diagnostic) => diagnostic.code,
    ),
  };
}

async function runGraphql(vector) {
  const graphql = require("../packages/graphql");
  if (vector.op === "parseError") {
    try {
      graphql.parse(vector.query);
      return { threw: false };
    } catch (error) {
      return { threw: true, error: normalizeError(error) };
    }
  }
  const schema = graphql.buildSchema(
    `type Query { ${vector.field}(input: Int!): Int! }`,
  );
  if (vector.op === "validate") {
    return graphql
      .validate(schema, graphql.parse(vector.query))
      .map((error) => error.message);
  }
  return graphql.graphql({
    schema,
    source: vector.query,
    rootValue: { [vector.field]: ({ input }) => input + vector.value },
    variableValues: { input: vector.value },
  });
}

async function runApollo(vector) {
  const { ApolloServer } = await import("@apollo/server");
  const server = new ApolloServer({
    typeDefs: `type Query { ${vector.field}(input: Int!): Int! }`,
    resolvers: {
      Query: {
        [vector.field]: (_parent, args) => args.input + vector.value,
      },
    },
  });
  try {
    const result = await server.executeOperation({
      query: `query($input:Int!){ ${vector.field}(input:$input) }`,
      variables: { input: vector.variable },
    });
    return result.body.singleResult;
  } finally {
    await server.stop();
  }
}

async function runSocketIo(vector) {
  const { Server } = require("../packages/socket.io");
  const { io: clientIo } = require("socket.io-client");
  const httpServer = createServer();
  const io = new Server(httpServer);
  const events = [];
  io.on("connection", (socket) => {
    socket.join(vector.room);
    socket.on(vector.event, (payload, ack) => {
      events.push({ event: vector.event, payload });
      ack({ room: vector.room, payload, events });
    });
  });
  await listen(httpServer);
  const port = httpServer.address().port;
  const client = clientIo(`http://127.0.0.1:${port}`, {
    transports: ["websocket"],
    reconnection: false,
  });
  try {
    await once(client, "connect");
    return await new Promise((resolve, reject) => {
      client
        .timeout(1000)
        .emit(vector.event, vector.payload, (error, response) => {
          if (error) reject(error);
          else resolve(response);
        });
    });
  } finally {
    client.close();
    await new Promise((resolve) => io.close(resolve));
    await new Promise((resolve) => httpServer.close(resolve));
  }
}

async function runTypeorm(vector) {
  const typeorm = require("../packages/typeorm");
  if (vector.op === "entitySchema") {
    const schema = new typeorm.EntitySchema({
      name: vector.entityName,
      tableName: vector.tableName,
      columns: {
        id: { type: Number, primary: true },
        [vector.columnName]: { type: String, nullable: vector.nullable },
      },
    });
    return {
      name: schema.options.name,
      tableName: schema.options.tableName,
      columns: Object.keys(schema.options.columns),
    };
  }
  if (vector.op === "dataSourceOptions") {
    const dataSourceOptions = {
      type: "sqlite",
      database: ":memory:",
      entities: [],
    };
    return {
      type: dataSourceOptions.type,
      database: dataSourceOptions.database,
      entityCount: dataSourceOptions.entities.length,
    };
  }
  typeorm.getMetadataArgsStorage().tables.push({
    target: vector.entityName,
    name: vector.tableName,
    type: "regular",
  });
  return {
    tables: typeorm
      .getMetadataArgsStorage()
      .tables.filter((table) => table.name === vector.tableName).length,
  };
}

async function requestHttp(appOrHandler, vector) {
  const server =
    typeof appOrHandler.listen === "function"
      ? await listenExpress(appOrHandler)
      : createServer(appOrHandler);
  if (typeof appOrHandler.listen !== "function") {
    await listen(server);
  }
  try {
    const port = server.address().port;
    const response = await fetch(
      `http://127.0.0.1:${port}/items/${vector.pathId}?${new URLSearchParams(
        vector.query,
      )}`,
      {
        method: vector.method,
        headers: { "content-type": "application/json" },
        body: ["GET", "HEAD"].includes(vector.method)
          ? undefined
          : JSON.stringify(vector.payload),
      },
    );
    return normalizeHttpResult({
      status: response.status,
      headers: Object.fromEntries(response.headers),
      body: await response.text(),
    });
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

function listenExpress(app) {
  return new Promise((resolve, reject) => {
    const server = app.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve(server);
    });
    server.once("error", reject);
  });
}

function normalizeHttpResult(result) {
  let parsed = null;
  try {
    parsed = result.body ? JSON.parse(result.body) : null;
  } catch {
    parsed = result.body;
  }
  return {
    status: result.status,
    header: result.headers["x-vector"] ?? null,
    body: parsed,
  };
}

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
}

function once(emitter, event) {
  return new Promise((resolve, reject) => {
    emitter.once(event, resolve);
    emitter.once("connect_error", reject);
    emitter.once("error", reject);
  });
}

async function importModule(specifier) {
  if (!moduleCache.has(specifier)) {
    moduleCache.set(
      specifier,
      import(pathToFileURL(new URL(specifier, import.meta.url).pathname)),
    );
  }
  return moduleCache.get(specifier);
}

function normalizeError(error) {
  return {
    name: error?.name ?? "Error",
    message: String(error?.message ?? error).split("\n")[0],
  };
}
