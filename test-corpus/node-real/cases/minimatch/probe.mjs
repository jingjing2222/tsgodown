import { minimatch } from "minimatch";

const matrix = [
  ["index.js", "*.js", {}],
  ["index.ts", "*.js", {}],
  ["src/a/b/index.ts", "src/**/*.ts", {}],
  [".env", "*", {}],
  [".env", "*", { dot: true }],
  ["src/app.test.ts", "src/**/*.{test,spec}.ts", {}],
  ["src/app.ts", "!(dist)/**/*.ts", {}],
  ["dist/app.ts", "!(dist)/**/*.ts", {}],
  ["src\\app.ts", "src/**/*.ts", { windowsPathsNoEscape: true }],
];

const report = {
  package: "minimatch",
  probes: matrix.map(([path, pattern, options]) => ({
    path,
    pattern,
    options,
    match: minimatch(path, pattern, options),
  })),
};

console.log(JSON.stringify(report, null, 2));
