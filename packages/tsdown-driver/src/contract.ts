import { normalizeDiagnostics } from "./diagnostics.js";
import type { ArtifactManifest } from "./types.js";

export type NormalizedRustEngineResponse =
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

export function normalizeRustEngineResponse(
  payload: unknown,
  stdout: string,
): NormalizedRustEngineResponse {
  if (!isRecord(payload)) {
    return contractError(
      "rust-engine-binary-contract",
      "missing or invalid status envelope",
      stdout,
    );
  }

  const status = normalizeStatus(payload);
  if (!status) {
    return contractError(
      "rust-engine-binary-contract",
      "missing or invalid status envelope",
      stdout,
    );
  }

  if (status === "ok") {
    if (!isArtifactManifest(payload.manifest)) {
      return contractError(
        "rust-engine-binary-contract",
        "ok envelope missing valid manifest payload",
        stdout,
      );
    }

    return {
      ok: true,
      manifest: payload.manifest,
      diagnostics: normalizeDiagnostics(payload.diagnostics),
    };
  }

  const errorObj = isRecord(payload.error) ? payload.error : payload;
  return {
    ok: false,
    error: {
      source: normalizeText(errorObj.source, "rust-engine-binary"),
      cause: normalizeText(
        errorObj.cause,
        "rust engine returned error without cause",
      ),
      guidance: normalizeText(
        errorObj.guidance,
        "Inspect rust engine logs and JSON response contract.",
      ),
    },
  };
}

function normalizeStatus(
  payload: Record<string, unknown>,
): "ok" | "error" | undefined {
  if (typeof payload.ok === "boolean") {
    return payload.ok ? "ok" : "error";
  }

  if (typeof payload.status !== "string") return undefined;

  const normalized = payload.status.trim().toLowerCase();
  if (normalized === "ok" || normalized === "success") {
    return "ok";
  }

  if (normalized === "error" || normalized === "failed") {
    return "error";
  }

  return undefined;
}

function isArtifactManifest(value: unknown): value is ArtifactManifest {
  if (!isRecord(value)) return false;

  return (
    typeof value.buildId === "string" &&
    Array.isArray(value.entries) &&
    Array.isArray(value.bundles) &&
    Array.isArray(value.types) &&
    typeof value.tsconfigPath === "string"
  );
}

function contractError(
  source: string,
  reason: string,
  stdout: string,
): NormalizedRustEngineResponse {
  return {
    ok: false,
    error: {
      source,
      cause: `${reason} stdout=${stdout || "<empty>"}`,
      guidance:
        "Ensure rust engine returns deterministic status or ok envelope.",
    },
  };
}

function normalizeText(value: unknown, fallback: string): string {
  if (typeof value !== "string") return fallback;
  const normalized = value.trim();
  return normalized || fallback;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
