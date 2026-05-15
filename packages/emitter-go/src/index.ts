import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { ProgramIR } from "@tsgodown/ir-core";

interface EmitGoResponse {
  files?: Array<{
    path?: string;
    contents?: string;
  }>;
}

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);
const defaultEngineCoreBin = path.join(
  repoRoot,
  "target",
  "debug",
  "engine-core",
);

export function emitGoProject(ir: ProgramIR, outDir: string) {
  fs.mkdirSync(outDir, { recursive: true });
  const entry = ir.modules[0]?.sourcePath ?? "src/index.js";
  const cwd = path.dirname(outDir);
  const response = emitGoWithEngineCore({
    analyze: {
      manifest: {
        entry,
      },
      cwd,
      config: {},
    },
    packageName: "main",
    modulePath: "example.com/tsgodown-generated",
    outputKind: "main",
  });

  for (const file of response.files ?? []) {
    if (typeof file.path !== "string" || typeof file.contents !== "string") {
      continue;
    }
    const outputPath = path.join(outDir, file.path);
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, file.contents);
  }
}

function emitGoWithEngineCore(request: unknown): EmitGoResponse {
  const engineCoreBin =
    process.env.TSGODOWN_ENGINE_CORE_BIN ?? defaultEngineCoreBin;

  if (!fs.existsSync(engineCoreBin)) {
    const build = spawnSync("cargo", ["build", "-p", "engine-core"], {
      cwd: repoRoot,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    });
    if (build.status !== 0) {
      throw new Error(build.stderr || build.stdout || "cargo build failed");
    }
  }

  const emit = spawnSync(engineCoreBin, ["emit-go"], {
    cwd: repoRoot,
    input: JSON.stringify(request),
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });

  if (emit.status !== 0) {
    throw new Error(emit.stderr || emit.stdout || "engine-core emit-go failed");
  }

  return JSON.parse(emit.stdout) as EmitGoResponse;
}
