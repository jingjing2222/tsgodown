import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import dotenv from "../packages/dotenv/lib/main.js";
import { execa } from "../packages/execa/index.js";
import fsExtra from "../packages/fs-extra/lib/index.js";
import yaml from "../packages/js-yaml/dist/js-yaml.mjs";
import { LRUCache } from "../packages/lru-cache/dist/esm/index.js";
import { minimatch } from "../packages/minimatch/dist/esm/index.js";
import qs from "../packages/qs/lib/index.js";
import semver from "../packages/semver/index.js";
import {
  parse as uuidParse,
  stringify as uuidStringify,
  v4 as uuidV4,
  v5 as uuidV5,
  validate as uuidValidate,
  version as uuidVersion,
} from "../packages/uuid/dist-node/index.js";
import parser from "../packages/yargs-parser/build/lib/index.js";

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
    case "semver":
      return runSemver(vector);
    case "minimatch":
      return minimatch(vector.path, vector.pattern, vector.options);
    case "qs":
      return vector.op === "parse"
        ? qs.parse(vector.query)
        : qs.stringify(vector.value, vector.options);
    case "dotenv":
      return runDotenv(vector);
    case "yargs-parser":
      return parser(vector.argv, vector.options);
    case "js-yaml":
      return runYaml(vector);
    case "lru-cache":
      return runLru(vector);
    case "uuid":
      return runUuid(vector);
    case "fs-extra":
      return runFsExtra(vector);
    case "execa":
      return runExeca(vector);
    default:
      throw new Error(`unknown corpus ${corpus}`);
  }
}

function runSemver(vector) {
  if (vector.op === "valid") return semver.valid(vector.value);
  if (vector.op === "compare") return semver.compare(vector.left, vector.right);
  if (vector.op === "satisfies")
    return semver.satisfies(vector.value, vector.range);
  if (vector.op === "sort") return semver.sort([...vector.values]);
  throw new Error(`unknown semver op ${vector.op}`);
}

function runDotenv(vector) {
  if (vector.op === "parse") return dotenv.parse(vector.envText);
  const dir = mkdtempSync(join(tmpdir(), "tsgodown-dotenv-vector-"));
  const envPath = join(dir, ".env");
  const previous = new Map();
  try {
    writeFileSync(envPath, vector.envText);
    for (const [key, value] of Object.entries(vector.initialEnv ?? {})) {
      previous.set(key, process.env[key]);
      if (value === undefined) {
        Reflect.deleteProperty(process.env, key);
      } else {
        process.env[key] = value;
      }
    }
    const result = dotenv.config({
      path: envPath,
      override: vector.override,
      quiet: true,
    });
    const observedEnv = {};
    for (const key of Object.keys(dotenv.parse(vector.envText))) {
      observedEnv[key] = process.env[key] ?? null;
    }
    return {
      parsed: result.parsed ?? null,
      error: result.error?.name ?? null,
      observedEnv,
    };
  } finally {
    for (const [key, value] of previous) {
      if (value === undefined) {
        Reflect.deleteProperty(process.env, key);
      } else {
        process.env[key] = value;
      }
    }
    rmSync(dir, { recursive: true, force: true });
  }
}

function runYaml(vector) {
  if (vector.op === "load") return yaml.load(vector.source);
  if (vector.op === "dump") return yaml.dump(vector.value);
  if (vector.op === "invalid") {
    try {
      yaml.load(vector.source);
      return { threw: false };
    } catch (error) {
      return {
        threw: true,
        name: error.name,
        reason: error.reason ?? null,
        messagePrefix: String(error.message).split("\n")[0],
      };
    }
  }
  throw new Error(`unknown yaml op ${vector.op}`);
}

function runLru(vector) {
  const cache = new LRUCache(vector.options);
  const results = [];
  for (const step of vector.steps) {
    if (step.op === "set") {
      cache.set(step.key, step.value);
      results.push(["set", step.key, true]);
    } else if (step.op === "get") {
      results.push(["get", step.key, cache.get(step.key) ?? null]);
    } else if (step.op === "has") {
      results.push(["has", step.key, cache.has(step.key)]);
    } else if (step.op === "entries") {
      results.push(["entries", [...cache.entries()]]);
    } else {
      throw new Error(`unknown lru op ${step.op}`);
    }
  }
  return results;
}

function runUuid(vector) {
  if (vector.op === "validate") return uuidValidate(vector.value);
  if (vector.op === "version") {
    try {
      return uuidVersion(vector.value);
    } catch (error) {
      return { error: normalizeError(error) };
    }
  }
  if (vector.op === "roundTrip") {
    try {
      return uuidStringify(uuidParse(vector.value));
    } catch (error) {
      return { error: normalizeError(error) };
    }
  }
  if (vector.op === "v5") return uuidV5(vector.name, uuidV5.URL);
  if (vector.op === "v4Shape") {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      uuidV4(),
    );
  }
  throw new Error(`unknown uuid op ${vector.op}`);
}

async function runFsExtra(vector) {
  const root = mkdtempSync(join(tmpdir(), "tsgodown-fs-extra-vector-"));
  try {
    for (const file of vector.files) {
      const target = join(root, file.path);
      await fsExtra.ensureDir(dirname(target));
      await fsExtra.writeJson(target, file.json);
    }
    const before = [];
    for (const file of vector.files) {
      before.push(await fsExtra.readJson(join(root, file.path)));
    }
    await fsExtra.copy(join(root, vector.copyFrom), join(root, vector.copyTo));
    await fsExtra.remove(join(root, vector.remove));
    return {
      before,
      removedExists: await fsExtra.pathExists(join(root, vector.remove)),
      copiedExists: await fsExtra.pathExists(join(root, vector.copyTo)),
      rootExists: await fsExtra.pathExists(root),
    };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

async function runExeca(vector) {
  try {
    const result = await execa(
      process.execPath,
      ["-e", vector.code, ...vector.args],
      {
        env: vector.env,
      },
    );
    return {
      failed: false,
      exitCode: result.exitCode,
      stdout: result.stdout,
      stderr: result.stderr,
    };
  } catch (error) {
    return {
      failed: true,
      exitCode: error.exitCode,
      stdout: error.stdout,
      stderr: error.stderr,
      shortMessagePrefix: String(error.shortMessage).split("\n")[0],
    };
  }
}

function normalizeError(error) {
  return {
    name: error?.name ?? "Error",
    message: String(error?.message ?? error),
  };
}
