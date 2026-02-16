import { spawn } from "node:child_process";

import { normalizeRustEngineResponse } from "./contract.js";
import type { RustEngineRequest, RustEngineResponse } from "./types.js";

export async function invokeRustEngine(
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
        const parsed = JSON.parse(out) as unknown;
        resolve(normalizeRustEngineResponse(parsed, out));
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
