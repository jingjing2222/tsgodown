export interface CommandErrorDetails {
  message: string;
  source?: string;
  stage?: string;
  cause?: string;
  guidance?: string;
}

export function extractCommandErrorDetails(
  error: unknown,
): CommandErrorDetails {
  const message = error instanceof Error ? error.message : String(error);

  const details: CommandErrorDetails = { message };
  const fields = parseDelimitedKeyValuePairs(message);

  details.source = fields.source;
  details.stage = fields.stage;
  details.cause = fields.cause;
  details.guidance = fields.guidance;

  return details;
}

export function printHumanError(details: CommandErrorDetails): void {
  console.error(`[tsgodown] command failed: ${details.message}`);
  if (details.source) {
    console.error(`source: ${details.source}`);
  }
  if (details.stage) {
    console.error(`stage: ${details.stage}`);
  }
  if (details.cause) {
    console.error(`cause: ${details.cause}`);
  }
  if (details.guidance) {
    console.error(`guidance: ${details.guidance}`);
  }
}

function parseDelimitedKeyValuePairs(message: string): Record<string, string> {
  const parsed: Record<string, string> = {};
  for (const token of message.split(";")) {
    const item = token.trim();
    const idx = item.indexOf(":");
    if (idx <= 0) {
      continue;
    }

    const key = item.slice(0, idx).trim().toLowerCase();
    const value = item.slice(idx + 1).trim();
    if (!key || !value) {
      continue;
    }
    parsed[key] = value;
  }
  return parsed;
}
