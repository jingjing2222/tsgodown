import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  CAPABILITY_KEYS,
  CAPABILITY_MATRIX,
  type CapabilityStatus,
} from "../src/capability.ts";

interface MatrixRow {
  key: string;
  scope: string;
  status: string;
  strategy: string;
  goStatus: string;
  goStrategy: string;
}

function parseCapabilityRows(markdown: string): MatrixRow[] {
  const rows = markdown
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("|"))
    .map((line) =>
      line
        .split("|")
        .slice(1, -1)
        .map((cell) => cell.trim()),
    )
    .filter((cells) => cells.length === 6)
    .filter((cells) => cells[0] !== "Capability Key")
    .filter((cells) => !cells.every((cell) => /^-+$/.test(cell)));

  return rows.map(([key, scope, status, strategy, goStatus, goStrategy]) => ({
    key,
    scope,
    status,
    strategy,
    goStatus,
    goStrategy,
  }));
}

test("CAPABILITY_MATRIX keys are unique and internally consistent", () => {
  const matrixEntries = Object.entries(CAPABILITY_MATRIX);
  const matrixKeys = matrixEntries.map(([key]) => key);

  assert.deepEqual(matrixKeys, CAPABILITY_KEYS);
  assert.equal(new Set(matrixKeys).size, matrixKeys.length);

  for (const [key, rule] of matrixEntries) {
    assert.equal(rule.key, key);
  }
});

test("docs/specs/CAPABILITY_MATRIX.md stays 1:1 with implemented capability keys", () => {
  const docPath = new URL(
    "../../../docs/specs/CAPABILITY_MATRIX.md",
    import.meta.url,
  );
  const markdown = readFileSync(docPath, "utf8");
  const rows = parseCapabilityRows(markdown);

  const docKeys = rows.map((row) => row.key);
  assert.equal(new Set(docKeys).size, docKeys.length);
  assert.deepEqual(docKeys, CAPABILITY_KEYS);

  for (const row of rows) {
    const rule = CAPABILITY_MATRIX[row.key as keyof typeof CAPABILITY_MATRIX];
    assert.ok(rule, `Missing code rule for ${row.key}`);
    assert.equal(rule.scope, row.scope);
    assert.equal(rule.status, row.status as CapabilityStatus);
    assert.equal(rule.strategy, row.strategy);
    assert.equal(rule.backends.go.status, row.goStatus as CapabilityStatus);
    assert.equal(rule.backends.go.strategy, row.goStrategy);
  }
});
