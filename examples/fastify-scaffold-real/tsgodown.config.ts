export default {
  entry: "src/app.ts",
  outDir: "dist-go",
  format: "esm",
  sourcemap: true,
  target: "node20",
  treeshake: true,
  go: {
    package: "main",
    port: 18081,
  },
};
