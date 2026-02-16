import type { TsdownBundleLike } from "../types.js";
import { normalizeChunkFile, uniqueSorted } from "./validation.js";

export interface IndexedArtifacts {
  chunkFiles: string[];
  entries: string[];
}

export function indexArtifacts(
  cwd: string,
  bundles: TsdownBundleLike[],
): IndexedArtifacts {
  const chunkFiles = uniqueSorted(
    bundles
      .flatMap((bundle) => bundle.chunks ?? [])
      .map((chunk) => normalizeChunkFile(cwd, chunk.fileName))
      .filter((file): file is string => Boolean(file)),
  );

  const entries = uniqueSorted(
    bundles.flatMap((bundle) => {
      const entryMap = bundle.config?.entry;
      if (!entryMap) return [];
      return Object.values(entryMap)
        .map((entryPath) => normalizeChunkFile(cwd, entryPath))
        .filter((entry): entry is string => Boolean(entry));
    }),
  );

  return {
    chunkFiles,
    entries,
  };
}
