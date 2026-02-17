#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { globSync } from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const rustGlob = "packages/analyzer-rust/src/**/*.rs";
const diagnosticsDocPath = path.join(repoRoot, "docs/specs/DIAGNOSTICS.md");
const matrixDocPath = path.join(
  repoRoot,
  "docs/specs/FASTIFY_SUPPORT_MATRIX.md",
);
const inventoryDocPath = path.join(
  repoRoot,
  "docs/specs/FASTIFY_UNSUPPORTED_INVENTORY.md",
);

const WRITE = process.argv.includes("--write");

const diagnosticsBlockMarkers = {
  start: "<!-- AUTO-GENERATED:DIAGNOSTIC_MESSAGES:START -->",
  end: "<!-- AUTO-GENERATED:DIAGNOSTIC_MESSAGES:END -->",
};

const matrixCodesMarkers = {
  start: "<!-- AUTO-GENERATED:DIAGNOSTIC_CODES:START -->",
  end: "<!-- AUTO-GENERATED:DIAGNOSTIC_CODES:END -->",
};

function splitTopLevelArgs(input) {
  const out = [];
  let buf = "";
  let depthParen = 0;
  let depthBrace = 0;
  let depthBracket = 0;
  let inString = false;
  let quote = "";
  let escaping = false;

  for (const ch of input) {
    buf += ch;

    if (inString) {
      if (escaping) {
        escaping = false;
      } else if (ch === "\\") {
        escaping = true;
      } else if (ch === quote) {
        inString = false;
        quote = "";
      }
      continue;
    }

    if (ch === '"' || ch === "'") {
      inString = true;
      quote = ch;
      continue;
    }

    if (ch === "(") depthParen += 1;
    else if (ch === ")") depthParen -= 1;
    else if (ch === "{") depthBrace += 1;
    else if (ch === "}") depthBrace -= 1;
    else if (ch === "[") depthBracket += 1;
    else if (ch === "]") depthBracket -= 1;

    if (
      ch === "," &&
      depthParen === 0 &&
      depthBrace === 0 &&
      depthBracket === 0
    ) {
      out.push(buf.slice(0, -1).trim());
      buf = "";
    }
  }

  if (buf.trim()) out.push(buf.trim());
  return out;
}

function decodeRustStringLiteral(lit) {
  const trimmed = lit.trim();
  const match = trimmed.match(/^"([\s\S]*)"$/);
  if (!match) return trimmed;
  return JSON.parse(
    `"${match[1].replace(/\\/g, "\\\\").replace(/\"/g, '\\"')}"`,
  )
    .replace(/\\n/g, "\n")
    .replace(/\\t/g, "\t");
}

function extractMessageTemplate(messageExpr) {
  const expr = messageExpr.trim();
  if (expr.startsWith("&format!(") || expr.startsWith("format!(")) {
    const firstQuote = expr.indexOf('"');
    if (firstQuote === -1) return expr;
    let i = firstQuote + 1;
    let escaped = false;
    while (i < expr.length) {
      const ch = expr[i];
      if (escaped) {
        escaped = false;
      } else if (ch === "\\") {
        escaped = true;
      } else if (ch === '"') {
        break;
      }
      i += 1;
    }
    return decodeRustStringLiteral(expr.slice(firstQuote, i + 1));
  }
  return decodeRustStringLiteral(expr);
}

function extractDiagCalls(content) {
  const entries = [];
  let i = 0;
  while (i < content.length) {
    const start = content.indexOf("diag(", i);
    if (start === -1) break;

    let j = start + "diag(".length;
    let depth = 1;
    let inString = false;
    let quote = "";
    let escaping = false;

    while (j < content.length && depth > 0) {
      const ch = content[j];
      if (inString) {
        if (escaping) {
          escaping = false;
        } else if (ch === "\\") {
          escaping = true;
        } else if (ch === quote) {
          inString = false;
          quote = "";
        }
      } else if (ch === '"' || ch === "'") {
        inString = true;
        quote = ch;
      } else if (ch === "(") {
        depth += 1;
      } else if (ch === ")") {
        depth -= 1;
      }
      j += 1;
    }

    const inner = content.slice(start + "diag(".length, j - 1);
    const args = splitTopLevelArgs(inner);
    if (args.length >= 3) {
      const code = decodeRustStringLiteral(args[1]);
      if (!/^[A-Z][A-Z0-9_]+$/.test(code)) {
        i = j;
        continue;
      }
      const message = extractMessageTemplate(args[2]);
      entries.push({ code, message });
    }
    i = j;
  }
  return entries;
}

function normalizeDocsCode(s) {
  return s.replace(/\r\n/g, "\n");
}

function replaceManagedBlock(content, markers, replacementBody) {
  const startIdx = content.indexOf(markers.start);
  const endIdx = content.indexOf(markers.end);
  if (startIdx === -1 || endIdx === -1 || endIdx < startIdx) {
    throw new Error(
      `Missing managed block markers: ${markers.start} ... ${markers.end}`,
    );
  }
  const before = content.slice(0, startIdx + markers.start.length);
  const after = content.slice(endIdx);
  return `${before}\n${replacementBody.trimEnd()}\n${after}`;
}

const rustFiles = globSync(rustGlob, { cwd: repoRoot });
if (rustFiles.length === 0) {
  console.error(`No Rust files found for glob: ${rustGlob}`);
  process.exit(1);
}

const variantMap = new Map();
for (const rel of rustFiles) {
  const abs = path.join(repoRoot, rel);
  const text = readFileSync(abs, "utf8");
  const calls = extractDiagCalls(text);
  for (const call of calls) {
    const key = `${call.code}::${call.message}`;
    if (!variantMap.has(key)) {
      variantMap.set(key, { ...call, sources: [rel] });
    } else {
      variantMap.get(key).sources.push(rel);
    }
  }
}

const diagnostics = [...variantMap.values()].sort((a, b) => {
  if (a.code !== b.code) return a.code.localeCompare(b.code);
  return a.message.localeCompare(b.message);
});

const uniqueCodes = [...new Set(diagnostics.map((d) => d.code))].sort();

const diagBody = diagnostics
  .map((d) => `- \`${d.code}\`: \`${d.message}\``)
  .join("\n");

const matrixCodesBody = uniqueCodes.map((code) => `- \`${code}\``).join("\n");

const inventoryLines = [
  "# FASTIFY_UNSUPPORTED_INVENTORY",
  "",
  "Canonical inventory of diagnostics emitted by `packages/analyzer-rust` (codes + verbatim message templates).",
  "",
  "This file is auto-managed by `scripts/check-fastify-diagnostics-sync.mjs`.",
  "",
  "| Code | Message template (verbatim) | Source file(s) |",
  "| --- | --- | --- |",
  ...diagnostics.map((d) => {
    const src = [...new Set(d.sources)].sort().join(", ");
    const msg = d.message.replace(/\|/g, "\\|");
    return `| \`${d.code}\` | \`${msg}\` | \`${src}\` |`;
  }),
  "",
  "## Regeneration",
  "",
  "- Check only: `node scripts/check-fastify-diagnostics-sync.mjs`",
  "- Rewrite docs blocks + inventory: `node scripts/check-fastify-diagnostics-sync.mjs --write`",
  "",
].join("\n");

const diagnosticsDoc = normalizeDocsCode(
  readFileSync(diagnosticsDocPath, "utf8"),
);
const matrixDoc = normalizeDocsCode(readFileSync(matrixDocPath, "utf8"));
const currentInventory = (() => {
  try {
    return normalizeDocsCode(readFileSync(inventoryDocPath, "utf8"));
  } catch {
    return "";
  }
})();

const desiredDiagnosticsDoc = replaceManagedBlock(
  diagnosticsDoc,
  diagnosticsBlockMarkers,
  diagBody,
);
const desiredMatrixDoc = replaceManagedBlock(
  matrixDoc,
  matrixCodesMarkers,
  matrixCodesBody,
);
const desiredInventory = `${inventoryLines}`;

const problems = [];

function recordMismatch(label, hintLines) {
  problems.push({ label, hintLines });
}

if (desiredDiagnosticsDoc !== diagnosticsDoc) {
  if (WRITE) {
    writeFileSync(diagnosticsDocPath, desiredDiagnosticsDoc, "utf8");
  } else {
    recordMismatch("docs/specs/DIAGNOSTICS.md", [
      "Managed diagnostic-message block is out of sync with analyzer-rust.",
      "Hint: run `node scripts/check-fastify-diagnostics-sync.mjs --write` then inspect git diff.",
    ]);
  }
}

if (desiredMatrixDoc !== matrixDoc) {
  if (WRITE) {
    writeFileSync(matrixDocPath, desiredMatrixDoc, "utf8");
  } else {
    recordMismatch("docs/specs/FASTIFY_SUPPORT_MATRIX.md", [
      "Managed diagnostic-code list is out of sync with analyzer-rust.",
      "Hint: run `node scripts/check-fastify-diagnostics-sync.mjs --write` and ensure each code is documented in context.",
    ]);
  }
}

if (desiredInventory !== currentInventory) {
  if (WRITE) {
    writeFileSync(inventoryDocPath, desiredInventory, "utf8");
  } else {
    recordMismatch("docs/specs/FASTIFY_UNSUPPORTED_INVENTORY.md", [
      "Inventory file does not match analyzer-rust diagnostics snapshot.",
      "Hint: run `node scripts/check-fastify-diagnostics-sync.mjs --write`.",
    ]);
  }
}

if (problems.length > 0) {
  console.error("✖ Fastify diagnostics/docs sync check failed.\n");
  for (const problem of problems) {
    console.error(`- ${problem.label}`);
    for (const hint of problem.hintLines) {
      console.error(`  ${hint}`);
    }
  }
  console.error("\nActionable next steps:");
  console.error("  1) node scripts/check-fastify-diagnostics-sync.mjs --write");
  console.error(
    "  2) git diff docs/specs scripts/check-fastify-diagnostics-sync.mjs",
  );
  console.error("  3) pnpm run format && pnpm run format:check");
  process.exit(1);
}

console.log(
  `✔ Fastify diagnostics/docs are in sync (${uniqueCodes.length} codes, ${diagnostics.length} message variants).`,
);
