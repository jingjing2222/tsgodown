import fs from "node:fs";
export function analyzeFastifyEntry(entryFile) {
    const src = fs.readFileSync(entryFile, "utf-8");
    const routeRe = /fastify\.(get|post|put|delete|patch)\(\s*['\"]([^'\"]+)['\"]\s*,\s*([A-Za-z_][A-Za-z0-9_]*)/g;
    const routes = [];
    for (const m of src.matchAll(routeRe)) {
        routes.push({
            method: m[1].toUpperCase(),
            path: m[2],
            handlerRef: m[3],
        });
    }
    const diagnostics = [];
    if (src.includes("import(")) {
        diagnostics.push({
            level: "warn",
            code: "DYNAMIC_IMPORT_DETECTED",
            message: "dynamic import detected",
            source: { file: entryFile },
        });
    }
    return {
        modules: [],
        routes,
        handlers: [],
        diagnostics,
    };
}
