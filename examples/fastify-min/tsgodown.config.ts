export default {
  entry: "src/app.ts",
  outDir: "dist-go",
  treeshake: true,
  fastify: {
    detectPlugins: true,
    routeMode: "direct",
  },
  go: {
    package: "main",
    port: 18081,
  },
};
