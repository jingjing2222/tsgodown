import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import fsExtra from "../../packages/fs-extra/lib/index.js";

const root = mkdtempSync(join(tmpdir(), "tsgodown-fs-extra-"));
const sourceDir = join(root, "source");
const targetDir = join(root, "target");
const sourceJson = join(sourceDir, "data.json");
const copiedJson = join(targetDir, "data.json");

await fsExtra.ensureDir(sourceDir);
await fsExtra.writeJson(sourceJson, { name: "tsgodown", count: 2 });
const readBack = await fsExtra.readJson(sourceJson);
await fsExtra.copy(sourceDir, targetDir);
const copied = await fsExtra.readJson(copiedJson);
await fsExtra.remove(sourceDir);

const report = {
  package: "fs-extra",
  probes: {
    readBack,
    copied,
    sourceExistsAfterRemove: await fsExtra.pathExists(sourceDir),
    targetExists: await fsExtra.pathExists(copiedJson),
  },
};

rmSync(root, { recursive: true, force: true });

console.log(JSON.stringify(report, null, 2));
