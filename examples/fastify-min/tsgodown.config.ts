import { defineConfig } from "@tsgodown/config";

export default defineConfig({
  entry: "src/index.ts",
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
  onSuccess() {
    console.log("[example] tsgodown done");
  },
});
