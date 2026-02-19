import path from "node:path";

import type { ProgramIR } from "@tsgodown/ir-core";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

import { ingestArtifactMetadata } from "./artifact-metadata.js";

export function buildProgramIrFromArtifacts(
  buildResult: RunBuildResult,
  entry: string,
): ProgramIR {
  const manifestEntries =
    buildResult.manifest.entries.length > 0
      ? buildResult.manifest.entries
      : [entry];

  const repoRoot = resolveRepoRoot(buildResult.manifestPath);
  const metadata = ingestArtifactMetadata(repoRoot, buildResult.manifest);

  const modules = manifestEntries.map((manifestEntry, index) => ({
    id: `module_${index}`,
    sourcePath: metadata.sourcePath ?? manifestEntry,
    exports: metadata.exports,
    imports: [],
  }));

  const primaryHandlerId = "health";

  return {
    modules,
    routes: [
      {
        method: "GET",
        path: "/health",
        handlerRef: primaryHandlerId,
      },
    ],
    handlers: [
      {
        id: primaryHandlerId,
        params: [],
        async: false,
        bodyRef: entry,
        semantics: {
          responseMode: "unknown",
          usesStatus: false,
          usesBody: false,
          usesHeaders: false,
          usesJson: false,
        },
      },
    ],
    diagnostics: metadata.diagnostics,
  };
}

function resolveRepoRoot(manifestPath: string): string {
  if (!path.isAbsolute(manifestPath)) {
    return process.cwd();
  }

  return path.resolve(path.dirname(manifestPath), "..", "..");
}
