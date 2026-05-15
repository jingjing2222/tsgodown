import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { runVectorCase } from "./vector-runner.mjs";

const corpusRoot = path.resolve(import.meta.dirname, "..");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);

function readVectors(testCase) {
  const vectorPath = path.join(
    corpusRoot,
    "cases",
    testCase.id,
    "vectors.json",
  );
  const vectors = JSON.parse(fs.readFileSync(vectorPath, "utf8"));
  expect(vectors.corpus).toBe(testCase.id);
  expect(vectors.cases).toHaveLength(100);
  return vectors.cases;
}

for (const testCase of manifest.cases) {
  describe(`node corpus vectors: ${testCase.id}`, () => {
    const vectors = readVectors(testCase);

    it.each(vectors)("$id", async (vector) => {
      const result = await runVectorCase(testCase.id, vector);
      expect(result.ok).toBe(true);
      expect(() => JSON.stringify(result.value)).not.toThrow();
    });
  });
}
