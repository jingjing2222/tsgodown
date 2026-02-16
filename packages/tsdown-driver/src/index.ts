import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import path from "node:path";

export type BundleFormat = "esm" | "cjs";

export interface ArtifactBundle {
  file: string;
  map?: string;
  format: BundleFormat;
  exports: string[];
}

export interface ArtifactManifest {
  buildId: string;
  entries: string[];
  bundles: ArtifactBundle[];
  types: string[];
  tsconfigPath: string;
}

export interface RunBuildResult {
  mode: "rust-engine-adapter";
  manifestPath: string;
  manifest: ArtifactManifest;
  diagnostics: string[];
}

interface BuildChunkLike {
  fileName?: string;
}

export interface TsdownBundleLike {
  chunks: BuildChunkLike[];
  config?: {
    entry?: Record<string, string>;
    tsconfig?: false | string;
  };
}

export interface RustEngineRequest {
  action: "build";
  cwd: string;
  configPath?: string;
}

export type RustEngineResponse =
  | {
      ok: true;
      manifest: ArtifactManifest;
      diagnostics?: string[];
    }
  | {
      ok: false;
      error: {
        source: string;
        cause: string;
        guidance: string;
      };
    };

interface RunBuildOptions {
  executeRustEngine?: (
    request: RustEngineRequest,
  ) => Promise<RustEngineResponse>;
}

export async function runBuild(
  cwd: string,
  configPath?: string,
  options: RunBuildOptions = {},
): Promise<RunBuildResult> {
  const request: RustEngineRequest = {
    action: "build",
    cwd,
    ...(configPath ? { configPath } : {}),
  };

  const executeRustEngine =
    options.executeRustEngine ?? ((req) => invokeRustEngine(req));
  const response = await executeRustEngine(request);

  if (!response.ok) {
    throw new Error(
      `[tsdown-driver] rust engine failed source=${response.error.source} cause=${response.error.cause} guidance=${response.error.guidance}`,
    );
  }

  const manifestPath = await writeManifest(cwd, response.manifest);
  return {
    mode: "rust-engine-adapter",
    manifestPath,
    manifest: response.manifest,
    diagnostics: response.diagnostics ?? [],
  };
}

async function invokeRustEngine(
  request: RustEngineRequest,
): Promise<RustEngineResponse> {
  const rustBin = process.env.TSGODOWN_RUST_ENGINE_BIN;
  if (!rustBin) {
    return {
      ok: false,
      error: {
        source: "rust-engine-bin-env",
        cause: "TSGODOWN_RUST_ENGINE_BIN is not set",
        guidance:
          "Set TSGODOWN_RUST_ENGINE_BIN to the Rust engine executable path.",
      },
    };
  }

  return invokeRustBinary(rustBin, request);
}

async function invokeRustBinary(
  commandPath: string,
  request: RustEngineRequest,
): Promise<RustEngineResponse> {
  return new Promise((resolve) => {
    const child = spawn(commandPath, [], {
      stdio: ["pipe", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });

    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });

    child.on("error", (error) => {
      resolve({
        ok: false,
        error: {
          source: "rust-engine-binary-spawn",
          cause: formatErrorWithCause(error),
          guidance:
            "Check TSGODOWN_RUST_ENGINE_BIN points to an executable binary.",
        },
      });
    });

    child.on("close", (code) => {
      const out = stdout.trim();
      if (code !== 0) {
        resolve({
          ok: false,
          error: {
            source: "rust-engine-binary",
            cause: `exit=${code ?? "null"} stderr=${stderr.trim() || "n/a"}`,
            guidance: "Inspect rust engine logs and JSON response contract.",
          },
        });
        return;
      }

      try {
        const parsed = JSON.parse(out) as RustEngineResponse;
        resolve(parsed);
      } catch (error) {
        resolve({
          ok: false,
          error: {
            source: "rust-engine-binary-json",
            cause: `${formatErrorWithCause(error)} stdout=${out || "<empty>"}`,
            guidance:
              "Ensure rust engine prints a valid JSON object to stdout.",
          },
        });
      }
    });

    child.stdin.write(`${JSON.stringify(request)}\n`);
    child.stdin.end();
  });
}

export function buildManifestFromBundles(
  cwd: string,
  bundles: TsdownBundleLike[],
  configPath?: string,
): ArtifactManifest {
  const chunkFiles = uniqueSorted(
    bundles
      .flatMap((bundle) => bundle.chunks ?? [])
      .map((chunk) => normalizeChunkFile(cwd, chunk.fileName))
      .filter((file): file is string => Boolean(file)),
  );

  const chunkSet = new Set(chunkFiles);
  const bundleFiles = chunkFiles.filter(isBundleFile);
  const bundlesOut = bundleFiles.map((file) => ({
    file,
    map: chunkSet.has(`${file}.map`) ? `${file}.map` : undefined,
    format: file.endsWith(".cjs") ? ("cjs" as const) : ("esm" as const),
    exports: [],
  }));

  const typeFiles = chunkFiles.filter(isTypeFile);

  const entries = uniqueSorted(
    bundles.flatMap((bundle) => {
      const entryMap = bundle.config?.entry;
      if (!entryMap) return [];
      return Object.values(entryMap)
        .map((entryPath) => normalizeChunkFile(cwd, entryPath))
        .filter((entry): entry is string => Boolean(entry));
    }),
  );

  const tsconfigPath = resolveTsconfigPath(cwd, bundles, configPath);

  const manifestBase = {
    entries,
    bundles: bundlesOut,
    types: typeFiles,
    tsconfigPath,
  };

  return {
    buildId: createBuildId(manifestBase),
    ...manifestBase,
  };
}

export async function writeManifest(
  cwd: string,
  manifest: ArtifactManifest,
): Promise<string> {
  const outDir = path.join(cwd, "artifacts", "manifests");
  await fs.mkdir(outDir, { recursive: true });

  const manifestPath = path.join(outDir, "manifest.json");
  await fs.writeFile(
    manifestPath,
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  );
  return manifestPath;
}

function resolveTsconfigPath(
  cwd: string,
  bundles: TsdownBundleLike[],
  configPath?: string,
): string {
  if (configPath) return toPosix(configPath);

  for (const bundle of bundles) {
    const tsconfig = bundle.config?.tsconfig;
    if (typeof tsconfig === "string") {
      return normalizeChunkFile(cwd, tsconfig) ?? toPosix(tsconfig);
    }
  }

  return "tsconfig.json";
}

function createBuildId(input: Omit<ArtifactManifest, "buildId">): string {
  const normalized = JSON.stringify(input);
  return createHash("sha256").update(normalized).digest("hex").slice(0, 16);
}

function isBundleFile(relFile: string): boolean {
  if (relFile.endsWith(".js.map")) return false;
  return (
    relFile.endsWith(".js") ||
    relFile.endsWith(".mjs") ||
    relFile.endsWith(".cjs")
  );
}

function isTypeFile(relFile: string): boolean {
  return (
    relFile.endsWith(".d.ts") ||
    relFile.endsWith(".d.mts") ||
    relFile.endsWith(".d.cts")
  );
}

function normalizeChunkFile(
  cwd: string,
  fileName?: string,
): string | undefined {
  if (!fileName) return undefined;
  const normalized = path.isAbsolute(fileName)
    ? path.relative(cwd, fileName)
    : fileName;
  return toPosix(normalized);
}

function uniqueSorted(values: string[]): string[] {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

function toPosix(p: string): string {
  return p.split(path.sep).join("/");
}

function formatErrorWithCause(error: unknown): string {
  const messages: string[] = [];
  let current: unknown = error;

  while (current) {
    if (current instanceof Error) {
      messages.push(`${current.name}: ${current.message}`);
      current = current.cause;
      continue;
    }

    messages.push(String(current));
    break;
  }

  return messages.join(" <- cause: ");
}
