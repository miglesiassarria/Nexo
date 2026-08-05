import { defineConfig } from "vite";

export default defineConfig({
  root: "website",
  base: "./",
  publicDir: "public",
  build: {
    outDir: "../site-dist",
    emptyOutDir: true,
    target: "es2020",
    sourcemap: false,
  },
});
