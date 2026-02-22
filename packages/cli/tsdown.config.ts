export default {
  entry: {
    index: "src/index.ts",
    config: "src/config.ts",
    "perf-baseline": "src/perf-baseline.ts",
  },
  outDir: "dist",
  format: ["esm"],
  dts: true,
  sourcemap: true,
  clean: true,
  fixedExtension: false,
  outExtensions: ({ format }) => ({
    js: format === "es" ? ".js" : ".cjs",
    dts: ".d.ts",
  }),
};
