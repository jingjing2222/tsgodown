import path from "node:path";
import { runBuild } from "../packages/tsdown-driver/src/index.js";

const cwd = process.argv[2] ? path.resolve(process.argv[2]) : process.cwd();
const configPath = process.argv[3];

const result = await runBuild(cwd, configPath);

console.log(`[tsdown-driver] mode=${result.mode}`);
console.log(`[tsdown-driver] manifest=${result.manifestPath}`);
console.log(`[tsdown-driver] buildId=${result.manifest.buildId}`);
