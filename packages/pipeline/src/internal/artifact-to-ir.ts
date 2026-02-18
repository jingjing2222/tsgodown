import type { ProgramIR } from "@tsgodown/ir-core";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

export function buildProgramIrFromArtifacts(
  buildResult: RunBuildResult,
  entry: string,
): ProgramIR {
  const manifestEntries =
    buildResult.manifest.entries.length > 0
      ? buildResult.manifest.entries
      : [entry];

  const modules = manifestEntries.map((manifestEntry, index) => ({
    id: `module_${index}`,
    sourcePath: manifestEntry,
    exports: [],
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
    diagnostics: [],
  };
}
