import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import dotenv from "../../packages/dotenv/lib/main.js";

const envText = [
  "PLAIN=value",
  'QUOTED="hello world"',
  "COMMENTED=ok # trailing comment",
  'ESCAPED="line\\nnext"',
  "EMPTY=",
].join("\n");

const parsed = dotenv.parse(envText);
const dir = mkdtempSync(join(tmpdir(), "tsgodown-dotenv-"));
const envPath = join(dir, ".env");
writeFileSync(envPath, envText);

const before = process.env.PLAIN;
process.env.PLAIN = "existing";
Reflect.deleteProperty(process.env, "QUOTED");
const configNoOverride = dotenv.config({
  path: envPath,
  override: false,
  quiet: true,
});
const afterNoOverride = {
  PLAIN: process.env.PLAIN,
  QUOTED: process.env.QUOTED,
};
const configOverride = dotenv.config({
  path: envPath,
  override: true,
  quiet: true,
});
const afterOverride = {
  PLAIN: process.env.PLAIN,
  QUOTED: process.env.QUOTED,
};

if (before === undefined) {
  Reflect.deleteProperty(process.env, "PLAIN");
} else {
  process.env.PLAIN = before;
}
Reflect.deleteProperty(process.env, "QUOTED");
rmSync(dir, { recursive: true, force: true });

const report = {
  package: "dotenv",
  probes: {
    parsed,
    noOverrideError: configNoOverride.error?.name ?? null,
    overrideError: configOverride.error?.name ?? null,
    afterNoOverride,
    afterOverride,
  },
};

console.log(JSON.stringify(report, null, 2));
