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

const KNOWN_OUTPUT_DIRS = ["bundle", "dist", "build", "out"];

export interface RunBuildResult {
  mode: "fallback-adapter";
  manifestPath: string;
  manifest: ArtifactManifest;
  diagnostics: string[];
}

export async function runBuild(
  cwd: string,
  configPath?: string,
): Promise<RunBuildResult> {
  const diagnostics = [
    "[tsdown-driver] TODO: real tsdown integration is not implemented; using fallback adapter scan mode.",
  ];

  const manifest = await collectArtifacts(cwd, configPath);
  const manifestPath = await writeManifest(cwd, manifest);

  for (const diagnostic of diagnostics) {
    console.warn(diagnostic);
  }

  return {
    mode: "fallback-adapter",
    manifestPath,
    manifest,
    diagnostics,
  };
}

export async function collectArtifacts(
  cwd: string,
  configPath?: string,
): Promise<ArtifactManifest> {
  const dirs = await existingOutputDirs(cwd);

  const bundleCandidates: string[] = [];
  const typeCandidates: string[] = [];

  for (const dir of dirs) {
    const files = await walkFiles(path.join(cwd, dir));
    for (const absFile of files) {
      const relFile = toPosix(path.relative(cwd, absFile));
      if (relFile.endsWith(".d.ts")) {
        typeCandidates.push(relFile);
      }

      if (isBundleFile(relFile)) {
        bundleCandidates.push(relFile);
      }
    }
  }

  const bundles = await toBundles(cwd, uniqueSorted(bundleCandidates));
  const types = uniqueSorted(typeCandidates);
  const entries = await detectEntries(cwd);

  const manifestBase = {
    entries,
    bundles,
    types,
    tsconfigPath: toPosix(configPath ?? "tsconfig.json"),
  };

  const buildId = createBuildId(manifestBase);

  return {
    buildId,
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

async function toBundles(
  cwd: string,
  files: string[],
): Promise<ArtifactBundle[]> {
  const bundles: ArtifactBundle[] = [];

  for (const file of files) {
    const absMap = path.join(cwd, `${file}.map`);
    const hasMap = await fileExists(absMap);
    bundles.push({
      file,
      map: hasMap ? toPosix(`${file}.map`) : undefined,
      format: file.endsWith(".cjs") ? "cjs" : "esm",
      exports: [],
    });
  }

  return bundles;
}

async function existingOutputDirs(cwd: string): Promise<string[]> {
  const dirs: string[] = [];

  for (const dir of KNOWN_OUTPUT_DIRS) {
    const absDir = path.join(cwd, dir);
    if (await directoryExists(absDir)) dirs.push(dir);
  }

  return dirs;
}

async function detectEntries(cwd: string): Promise<string[]> {
  const preferred = path.join(cwd, "src", "index.ts");
  if (await fileExists(preferred)) return ["src/index.ts"];

  const srcDir = path.join(cwd, "src");
  if (!(await directoryExists(srcDir))) return [];

  const files = await walkFiles(srcDir);
  return uniqueSorted(
    files
      .map((f) => toPosix(path.relative(cwd, f)))
      .filter((f) => f.endsWith(".ts") && !f.endsWith(".d.ts")),
  );
}

async function walkFiles(dir: string): Promise<string[]> {
  const out: string[] = [];
  const entries = await fs.readdir(dir, { withFileTypes: true });

  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...(await walkFiles(abs)));
    } else if (entry.isFile()) {
      out.push(abs);
    }
  }

  return out;
}

async function fileExists(p: string): Promise<boolean> {
  try {
    const stat = await fs.stat(p);
    return stat.isFile();
  } catch {
    return false;
  }
}

async function directoryExists(p: string): Promise<boolean> {
  try {
    const stat = await fs.stat(p);
    return stat.isDirectory();
  } catch {
    return false;
  }
}

function uniqueSorted(values: string[]): string[] {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

function toPosix(p: string): string {
  return p.split(path.sep).join("/");
}
