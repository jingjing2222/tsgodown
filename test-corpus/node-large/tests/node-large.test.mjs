import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { runVectorCase } from "./vector-runner.mjs";

const corpusRoot = path.resolve(import.meta.dirname, "..");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);

describe("large Node corpus manifest", () => {
  it("declares the 20 large corpus entries and 100-vector contract", () => {
    expect(manifest.version).toBe("node-large-corpus.v1");
    expect(manifest.nodeLts).toBe("24.15.0");
    expect(manifest.policy.vectorsPerEntry).toBe(100);
    expect(manifest.policy.status).toBe("vectors-node-ready");
    expect(manifest.entries).toHaveLength(20);
  });

  it.each(manifest.entries)(
    "$id records package metadata and pending vectors",
    (entry) => {
      expect(entry.id).toMatch(/^[a-z0-9-]+$/);
      expect(entry.package).toBeTypeOf("string");
      expect(entry.version).toMatch(/^\d+\.\d+\.\d+/);
      expect(entry.license).toBeTypeOf("string");
      expect(entry.source).toBeTypeOf("string");
      expect(entry.sourceLanguage).toMatch(/^(javascript|typescript)$/);
      expect(entry.moduleFormat).toMatch(/^(cjs|esm|esm-cjs)$/);
      expect(entry.declarationSource).toBeTypeOf("string");
      expect(entry.packageManager).toEqual({
        name: "npm",
        lockfile: "package-lock.json",
        installedWith: "npm ci --ignore-scripts",
      });
      expect(entry.packagePath).toMatch(/^packages\//);
      expect(entry.packageMetadataPath).toBe(
        `${entry.packagePath}/package.json`,
      );
      expect(entry.entry).toMatch(/^packages\//);
      expect(entry.entry.startsWith(entry.packagePath)).toBe(true);
      expect(
        fs.existsSync(path.join(corpusRoot, entry.packageMetadataPath)),
      ).toBe(true);
      expect(fs.existsSync(path.join(corpusRoot, entry.entry))).toBe(true);
      expect(entry.vendored).toMatchObject({
        source: "npm",
        package: entry.package,
        version: entry.version,
      });
      expect(entry.vendored.files).toBeGreaterThan(0);
      expect(entry.nativeOrExternalDependencyStatus).toBeTypeOf("string");
      expect(entry.probeCommand).toMatch(/^npm run --silent probe:/);
      expect(entry.comparator).toBeTypeOf("string");
      expect(entry.vectorFocus).toBeTypeOf("string");
      expect(entry.parityDimensions.length).toBeGreaterThan(0);
      expect(entry.vectors).toEqual({
        expected: 100,
        status: "node-ready",
      });
    },
  );
});

for (const entry of manifest.entries) {
  const vectors = JSON.parse(
    fs.readFileSync(
      path.join(corpusRoot, "cases", entry.id, "vectors.json"),
      "utf8",
    ),
  );

  describe(`${entry.id} Node vectors`, () => {
    it("declares exactly 100 vectors", () => {
      expect(vectors.version).toBe("node-large-corpus-vectors.v1");
      expect(vectors.corpus).toBe(entry.id);
      expect(vectors.cases).toHaveLength(100);
    });

    it.each(vectors.cases)(
      "$id matches Node observable behavior",
      async (vector) => {
        const result = await runVectorCase(entry.id, vector);
        expect(result.ok, JSON.stringify(result.error ?? null)).toBe(true);
        expect(result.value).not.toBeUndefined();
      },
    );
  });
}
