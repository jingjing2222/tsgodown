import fs from "node:fs";
import path from "node:path";

import type {
  DiagnosticIR,
  HandlerIR,
  ModuleIR,
  ProgramIR,
} from "@tsgodown/ir-core";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

function tryReadFile(
  baseDir: string,
  relativePath?: string,
): string | undefined {
  if (!relativePath) {
    return undefined;
  }

  const absolutePath = path.join(baseDir, relativePath);
  if (!fs.existsSync(absolutePath)) {
    return undefined;
  }

  return fs.readFileSync(absolutePath, "utf8");
}

function parseSourcePathFromMap(bundleFile: string, mapRaw?: string): string {
  if (!mapRaw) {
    return bundleFile;
  }

  try {
    const map = JSON.parse(mapRaw) as { sources?: string[] };
    const source = map.sources?.[0];
    if (!source) {
      return bundleFile;
    }

    const normalized = path
      .normalize(path.join(path.dirname(bundleFile), source))
      .replace(/\\/g, "/");

    return normalized.startsWith("../")
      ? normalized.slice(3)
      : normalized.replace(/^\.\//, "");
  } catch {
    return bundleFile;
  }
}

function parseExports(jsRaw?: string): string[] {
  if (!jsRaw) {
    return [];
  }

  const names = new Set<string>();
  for (const match of jsRaw.matchAll(
    /export\s+(?:async\s+)?function\s+([A-Za-z0-9_$]+)/g,
  )) {
    const name = match[1];
    if (name) {
      names.add(name);
    }
  }
  return [...names];
}

function parseImports(
  jsRaw?: string,
  bundleFile?: string,
): ModuleIR["imports"] {
  if (!jsRaw) {
    return [];
  }

  const imports: ModuleIR["imports"] = [];
  for (const match of jsRaw.matchAll(
    /import\s+[^"']+\s+from\s+["']([^"']+)["']/g,
  )) {
    const spec = match[1];
    if (!spec) {
      continue;
    }
    const resolved =
      spec.startsWith(".") && bundleFile
        ? path
            .normalize(path.join(path.dirname(bundleFile), spec))
            .replace(/\\/g, "/")
        : undefined;
    imports.push({ spec, kind: "esm", resolved });
  }

  return imports;
}

function parseReturnLiteralObject(
  jsRaw: string,
  exportName: string,
): string | undefined {
  const returnMatch = jsRaw.match(
    new RegExp(
      `export\\s+(?:async\\s+)?function\\s+${exportName}\\s*\\([^)]*\\)[\\s\\S]*?return\\s+(\\{[\\s\\S]*?\\})\\s*;`,
    ),
  );
  if (!returnMatch?.[1]) {
    return undefined;
  }

  const normalized = returnMatch[1]
    .replace(/^\{/, "")
    .replace(/\}$/, "")
    .replace(/([A-Za-z_$][A-Za-z0-9_$]*)\s*:/g, '"$1":')
    .replace(/'([^']*)'/g, '"$1"')
    .replace(/,\s*}/g, "}")
    .trim();

  try {
    const parsed = JSON.parse(`{${normalized}}`) as Record<string, unknown>;
    return JSON.stringify(parsed);
  } catch {
    return undefined;
  }
}

function parseHandlerParamsFromTypes(
  typeRaw: string | undefined,
  exportName: string,
): HandlerIR["params"] {
  if (!typeRaw) {
    return [];
  }

  const declaration = typeRaw.match(
    new RegExp(`function\\s+${exportName}\\s*\\(([^)]*)\\)`),
  );
  const rawParams = declaration?.[1]?.trim();
  if (!rawParams) {
    return [];
  }

  return rawParams
    .split(",")
    .map((segment) => segment.trim())
    .filter(Boolean)
    .map((segment) => {
      const [nameRaw, typeRawName] = segment
        .split(":")
        .map((part) => part.trim());
      const name = nameRaw.replace(/\?$/, "");
      const typeName = typeRawName ?? "";
      const lowered = typeName.toLowerCase();
      const role: HandlerIR["params"][number]["role"] = lowered.includes(
        "request",
      )
        ? "request"
        : lowered.includes("response")
          ? "response"
          : lowered.includes("next")
            ? "next"
            : "custom";
      return { name, role };
    });
}

export function buildProgramIrFromArtifacts(
  buildResult: RunBuildResult,
  entry: string,
): ProgramIR {
  const manifestEntries =
    buildResult.manifest.entries.length > 0
      ? buildResult.manifest.entries
      : [entry];

  const manifestDir = path.dirname(buildResult.manifestPath);
  const workspaceRoot = path.resolve(manifestDir, "..", "..");
  const bundle = buildResult.manifest.bundles[0];
  const bundleRaw = tryReadFile(workspaceRoot, bundle?.file);
  const mapRaw = tryReadFile(workspaceRoot, bundle?.map);
  const typeRaw = tryReadFile(workspaceRoot, buildResult.manifest.types?.[0]);

  const exports = parseExports(bundleRaw);
  const sourcePath = mapRaw
    ? parseSourcePathFromMap(
        bundle?.file ?? manifestEntries[0] ?? entry,
        mapRaw,
      )
    : (manifestEntries[0] ?? entry);

  const modules: ModuleIR[] = [
    {
      id: "module_0",
      sourcePath,
      exports,
      imports: parseImports(bundleRaw, bundle?.file),
    },
  ];

  const primaryHandlerId = exports[0] ?? "health";
  const handlerBody = bundleRaw
    ? parseReturnLiteralObject(bundleRaw, primaryHandlerId)
    : undefined;

  const handlers: HandlerIR[] = [
    {
      id: primaryHandlerId,
      params: parseHandlerParamsFromTypes(typeRaw, primaryHandlerId),
      async: Boolean(
        bundleRaw?.match(
          new RegExp(`export\\s+async\\s+function\\s+${primaryHandlerId}\\b`),
        ),
      ),
      bodyRef: handlerBody ?? entry,
      semantics: {
        responseMode: handlerBody ? "return" : "unknown",
        usesStatus: false,
        usesBody: false,
        usesHeaders: false,
        usesJson: Boolean(handlerBody),
      },
    },
  ];

  const diagnostics: DiagnosticIR[] = [
    {
      level: "info",
      code: "pipeline.artifacts.consumed",
      message: `consumed artifacts: bundle=${bundle?.file ?? "<none>"}, map=${bundle?.map ?? "<none>"}, types=${buildResult.manifest.types?.[0] ?? "<none>"}`,
    },
  ];

  return {
    modules,
    routes: [
      {
        method: "GET",
        path: "/health",
        handlerRef: primaryHandlerId,
      },
    ],
    handlers,
    diagnostics,
  };
}
