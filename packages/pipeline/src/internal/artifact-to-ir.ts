import type { ProgramIR } from "@tsgodown/ir-core";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

export function buildProgramIrFromArtifacts(
  buildResult: RunBuildResult,
  entry: string,
): ProgramIR {
  const modules = buildResult.manifest.entries.map((manifestEntry, index) => ({
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
        },
      },
    ],
    diagnostics: [],
  };
}
