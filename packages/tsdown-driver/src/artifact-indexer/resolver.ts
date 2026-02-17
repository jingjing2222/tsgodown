import { readFileSync } from "node:fs";
import path from "node:path";

export interface ResolverModuleRecord {
  from: string;
  spec: string;
  resolved: string;
}

export interface ResolverSymbolRecord {
  from: string;
  spec: string;
  imported: string;
  local: string;
  kind: "value" | "type";
}

export interface ResolverDiagnostic {
  code:
    | "UNRESOLVED_MODULE"
    | "UNSUPPORTED_DYNAMIC_IMPORT"
    | "UNSUPPORTED_REQUIRE_CALL"
    | "UNSUPPORTED_EXPORT_ALL"
    | "UNSUPPORTED_NAMESPACE_IMPORT"
    | "UNSUPPORTED_IMPORT_CLAUSE";
  message: string;
  file: string;
  line: number;
}

export interface ResolverSubsetResult {
  modules: ResolverModuleRecord[];
  symbols: ResolverSymbolRecord[];
  unresolved: ResolverDiagnostic[];
}

const IMPORT_FROM_RE =
  /^\s*import\s+(type\s+)?(.+?)\s+from\s+(["'])([^"']+)\3\s*;?\s*$/;
const EXPORT_FROM_RE =
  /^\s*export\s+\{([^}]+)\}\s+from\s+(["'])([^"']+)\2\s*;?\s*$/;

export function resolveSubsetFromEntries(
  cwd: string,
  entries: string[],
): ResolverSubsetResult {
  const modules: ResolverModuleRecord[] = [];
  const symbols: ResolverSymbolRecord[] = [];
  const unresolved: ResolverDiagnostic[] = [];

  for (const entry of entries) {
    const abs = path.resolve(cwd, entry);
    const rel = toPosix(path.relative(cwd, abs));
    const source = readFileSync(abs, "utf8");
    const lines = source.split(/\r?\n/);

    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index] ?? "";
      const lineNo = index + 1;

      if (line.includes("import(")) {
        unresolved.push({
          code: "UNSUPPORTED_DYNAMIC_IMPORT",
          message: "dynamic import(...) is unsupported in resolver subset",
          file: rel,
          line: lineNo,
        });
      }

      if (line.includes("require(")) {
        unresolved.push({
          code: "UNSUPPORTED_REQUIRE_CALL",
          message: "require(...) is unsupported in resolver subset",
          file: rel,
          line: lineNo,
        });
      }

      if (/^\s*export\s+\*\s+from\s+/.test(line)) {
        unresolved.push({
          code: "UNSUPPORTED_EXPORT_ALL",
          message: "export * from is unsupported in resolver subset",
          file: rel,
          line: lineNo,
        });
        continue;
      }

      const importMatch = line.match(IMPORT_FROM_RE);
      if (importMatch) {
        const isTypeOnlyImport = Boolean(importMatch[1]);
        const clause = importMatch[2]?.trim() ?? "";
        const spec = importMatch[4] ?? "";
        const resolved = resolveModule(cwd, abs, spec);
        if (!resolved) {
          unresolved.push({
            code: "UNRESOLVED_MODULE",
            message: `cannot resolve module specifier ${JSON.stringify(spec)}`,
            file: rel,
            line: lineNo,
          });
          continue;
        }

        modules.push({ from: rel, spec, resolved });

        if (clause.startsWith("* as ")) {
          unresolved.push({
            code: "UNSUPPORTED_NAMESPACE_IMPORT",
            message: "namespace import is unsupported in resolver subset",
            file: rel,
            line: lineNo,
          });
          continue;
        }

        if (clause.startsWith("{")) {
          const named = parseNamedBindings(clause, isTypeOnlyImport ? "type" : "value");
          if (!named) {
            unresolved.push({
              code: "UNSUPPORTED_IMPORT_CLAUSE",
              message: "import clause is unsupported in resolver subset",
              file: rel,
              line: lineNo,
            });
            continue;
          }
          symbols.push(...named.map((record) => ({ ...record, from: rel, spec })));
          continue;
        }

        if (/^[A-Za-z_$][\w$]*$/.test(clause)) {
          symbols.push({
            from: rel,
            spec,
            imported: "default",
            local: clause,
            kind: isTypeOnlyImport ? "type" : "value",
          });
          continue;
        }

        if (/^[A-Za-z_$][\w$]*\s*,\s*\{/.test(clause)) {
          const firstComma = clause.indexOf(",");
          const defaultPart = firstComma >= 0 ? clause.slice(0, firstComma).trim() : undefined;
          const namedPart = firstComma >= 0 ? clause.slice(firstComma + 1).trim() : undefined;
          if (!defaultPart || !namedPart) {
            unresolved.push({
              code: "UNSUPPORTED_IMPORT_CLAUSE",
              message: "import clause is unsupported in resolver subset",
              file: rel,
              line: lineNo,
            });
            continue;
          }

          symbols.push({
            from: rel,
            spec,
            imported: "default",
            local: defaultPart,
            kind: isTypeOnlyImport ? "type" : "value",
          });

          const named = parseNamedBindings(namedPart, isTypeOnlyImport ? "type" : "value");
          if (!named) {
            unresolved.push({
              code: "UNSUPPORTED_IMPORT_CLAUSE",
              message: "import clause is unsupported in resolver subset",
              file: rel,
              line: lineNo,
            });
            continue;
          }

          symbols.push(...named.map((record) => ({ ...record, from: rel, spec })));
          continue;
        }

        unresolved.push({
          code: "UNSUPPORTED_IMPORT_CLAUSE",
          message: "import clause is unsupported in resolver subset",
          file: rel,
          line: lineNo,
        });
        continue;
      }

      const exportMatch = line.match(EXPORT_FROM_RE);
      if (exportMatch) {
        const spec = exportMatch[3] ?? "";
        const resolved = resolveModule(cwd, abs, spec);
        if (!resolved) {
          unresolved.push({
            code: "UNRESOLVED_MODULE",
            message: `cannot resolve module specifier ${JSON.stringify(spec)}`,
            file: rel,
            line: lineNo,
          });
          continue;
        }

        modules.push({ from: rel, spec, resolved });
        continue;
      }

      if (/^\s*import\s+/.test(line)) {
        unresolved.push({
          code: "UNSUPPORTED_IMPORT_CLAUSE",
          message: "import clause is unsupported in resolver subset",
          file: rel,
          line: lineNo,
        });
      }
    }
  }

  return {
    modules: uniqueByKey(modules, (item) => `${item.from}\u0000${item.spec}\u0000${item.resolved}`),
    symbols: uniqueByKey(
      symbols,
      (item) => `${item.from}\u0000${item.spec}\u0000${item.imported}\u0000${item.local}\u0000${item.kind}`,
    ),
    unresolved: unresolved
      .slice()
      .sort(
        (a, b) =>
          a.file.localeCompare(b.file) ||
          a.line - b.line ||
          a.code.localeCompare(b.code) ||
          a.message.localeCompare(b.message),
      ),
  };
}

function parseNamedBindings(
  clause: string,
  kind: "value" | "type",
): Array<Pick<ResolverSymbolRecord, "imported" | "local" | "kind">> | undefined {
  const trimmed = clause.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return undefined;

  const body = trimmed.slice(1, -1).trim();
  if (!body) return [];

  const out: Array<Pick<ResolverSymbolRecord, "imported" | "local" | "kind">> = [];
  for (const raw of body.split(",")) {
    const part = raw.trim();
    if (!part) continue;

    const localType = part.startsWith("type ") ? "type" : kind;
    const item = part.replace(/^type\s+/, "");
    const [left, right] = item.split(/\s+as\s+/);
    const imported = (left ?? "").trim();
    const local = (right ?? left ?? "").trim();

    if (!/^[A-Za-z_$][\w$]*$/.test(imported) || !/^[A-Za-z_$][\w$]*$/.test(local)) {
      return undefined;
    }

    out.push({ imported, local, kind: localType });
  }

  return out;
}

function resolveModule(
  cwd: string,
  fromAbsPath: string,
  spec: string,
): string | undefined {
  if (!spec.startsWith("./") && !spec.startsWith("../")) {
    return undefined;
  }

  const fromDir = path.dirname(fromAbsPath);
  const base = path.resolve(fromDir, spec);
  const candidates = [
    base,
    `${base}.ts`,
    `${base}.tsx`,
    `${base}.js`,
    `${base}.mjs`,
    `${base}.cjs`,
    `${base}.d.ts`,
    path.join(base, "index.ts"),
    path.join(base, "index.tsx"),
    path.join(base, "index.js"),
    path.join(base, "index.mjs"),
    path.join(base, "index.cjs"),
    path.join(base, "index.d.ts"),
  ];

  for (const candidate of candidates) {
    try {
      const stat = readFileSync(candidate);
      if (stat.length >= 0) {
        return toPosix(path.relative(cwd, candidate));
      }
    } catch {
      // continue
    }
  }

  return undefined;
}

function uniqueByKey<T>(values: T[], keyOf: (value: T) => string): T[] {
  const seen = new Set<string>();
  const out: T[] = [];

  for (const value of values) {
    const key = keyOf(value);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(value);
  }

  return out.sort((a, b) => keyOf(a).localeCompare(keyOf(b)));
}

function toPosix(p: string): string {
  return p.split(path.sep).join("/");
}
