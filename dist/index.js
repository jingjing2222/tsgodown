import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import path from "node:path";
const KNOWN_OUTPUT_DIRS = ["bundle", "dist", "build", "out"];
export async function runBuild(cwd, configPath) {
    const diagnostics = [
        "[tsdown-driver] TODO: real tsdown integration is not implemented; using fallback adapter scan mode.",
    ];
    const manifest = await collectArtifacts(cwd, configPath);
    const manifestPath = await writeManifest(cwd, manifest);
    for (const diagnostic of diagnostics) {
        console.warn(diagnostic);
    }
    return {
        mode: "fallback-adapter",
        manifestPath,
        manifest,
        diagnostics,
    };
}
export async function collectArtifacts(cwd, configPath) {
    const dirs = await existingOutputDirs(cwd);
    const bundleCandidates = [];
    const typeCandidates = [];
    for (const dir of dirs) {
        const files = await walkFiles(path.join(cwd, dir));
        for (const absFile of files) {
            const relFile = toPosix(path.relative(cwd, absFile));
            if (relFile.endsWith(".d.ts")) {
                typeCandidates.push(relFile);
            }
            if (isBundleFile(relFile)) {
                bundleCandidates.push(relFile);
            }
        }
    }
    const bundles = await toBundles(cwd, uniqueSorted(bundleCandidates));
    const types = uniqueSorted(typeCandidates);
    const entries = await detectEntries(cwd);
    const manifestBase = {
        entries,
        bundles,
        types,
        tsconfigPath: toPosix(configPath ?? "tsconfig.json"),
    };
    const buildId = createBuildId(manifestBase);
    return {
        buildId,
        ...manifestBase,
    };
}
export async function writeManifest(cwd, manifest) {
    const outDir = path.join(cwd, "artifacts", "manifests");
    await fs.mkdir(outDir, { recursive: true });
    const manifestPath = path.join(outDir, "manifest.json");
    await fs.writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
    return manifestPath;
}
function createBuildId(input) {
    const normalized = JSON.stringify(input);
    return createHash("sha256").update(normalized).digest("hex").slice(0, 16);
}
function isBundleFile(relFile) {
    if (relFile.endsWith(".js.map"))
        return false;
    return (relFile.endsWith(".js") ||
        relFile.endsWith(".mjs") ||
        relFile.endsWith(".cjs"));
}
async function toBundles(cwd, files) {
    const bundles = [];
    for (const file of files) {
        const absMap = path.join(cwd, `${file}.map`);
        const hasMap = await fileExists(absMap);
        bundles.push({
            file,
            map: hasMap ? toPosix(`${file}.map`) : undefined,
            format: file.endsWith(".cjs") ? "cjs" : "esm",
            exports: [],
        });
    }
    return bundles;
}
async function existingOutputDirs(cwd) {
    const dirs = [];
    for (const dir of KNOWN_OUTPUT_DIRS) {
        const absDir = path.join(cwd, dir);
        if (await directoryExists(absDir))
            dirs.push(dir);
    }
    return dirs;
}
async function detectEntries(cwd) {
    const preferred = path.join(cwd, "src", "index.ts");
    if (await fileExists(preferred))
        return ["src/index.ts"];
    const srcDir = path.join(cwd, "src");
    if (!(await directoryExists(srcDir)))
        return [];
    const files = await walkFiles(srcDir);
    return uniqueSorted(files
        .map((f) => toPosix(path.relative(cwd, f)))
        .filter((f) => f.endsWith(".ts") && !f.endsWith(".d.ts")));
}
async function walkFiles(dir) {
    const out = [];
    const entries = await fs.readdir(dir, { withFileTypes: true });
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
        const abs = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            out.push(...(await walkFiles(abs)));
        }
        else if (entry.isFile()) {
            out.push(abs);
        }
    }
    return out;
}
async function fileExists(p) {
    try {
        const stat = await fs.stat(p);
        return stat.isFile();
    }
    catch {
        return false;
    }
}
async function directoryExists(p) {
    try {
        const stat = await fs.stat(p);
        return stat.isDirectory();
    }
    catch {
        return false;
    }
}
function uniqueSorted(values) {
    return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}
function toPosix(p) {
    return p.split(path.sep).join("/");
}
