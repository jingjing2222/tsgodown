import fs from "node:fs";
import path from "node:path";
export function emitGoProject(ir, outDir) {
    fs.mkdirSync(outDir, { recursive: true });
    const lines = [];
    lines.push("package main\n");
    lines.push('import (\n\t"fmt"\n\t"net/http"\n)\n');
    lines.push("func main() {");
    for (const [idx, r] of ir.routes.entries()) {
        lines.push(`\thttp.HandleFunc("${r.path}", route${idx})`);
    }
    lines.push('\tfmt.Println("tsgodown scaffold :18081")');
    lines.push('\t_ = http.ListenAndServe(":18081", nil)');
    lines.push("}\n");
    for (const [idx, r] of ir.routes.entries()) {
        lines.push(`func route${idx}(w http.ResponseWriter, req *http.Request) {`);
        lines.push(`\tfmt.Fprintln(w, "TODO ${r.method} ${r.path} -> ${r.handlerRef}")`);
        lines.push("}\n");
    }
    fs.writeFileSync(path.join(outDir, "main.go"), lines.join("\n"));
}
