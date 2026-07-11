import { defineConfig } from "vite";
import preact from "@preact/preset-vite";
import { resolve } from "node:path";

// webxdc: file:// style load inside a sandbox — no absolute paths, no crossorigin modules.
export default defineConfig({
  base: "./",
  root: ".",
  publicDir: "public",
  plugins: [
    preact(),
    {
      name: "webxdc-html-fix",
      transformIndexHtml(html) {
        return html
          .replace(/\s+crossorigin(="[^"]*")?/g, "")
          .replace(
            /<script type="module"([^>]*) src="([^"]+)"><\/script>/g,
            '<script defer src="$2"></script>',
          );
      },
    },
  ],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    assetsDir: "assets",
    target: "es2018",
    minify: "esbuild",
    cssCodeSplit: false,
    modulePreload: false,
    rollupOptions: {
      input: resolve(__dirname, "index.html"),
      output: {
        // Classic single bundle — more reliable in webxdc webviews than ES modules.
        format: "iife",
        name: "TerminalBuddy",
        entryFileNames: "assets/app.js",
        chunkFileNames: "assets/[name].js",
        assetFileNames: "assets/[name][extname]",
        inlineDynamicImports: true,
      },
    },
  },
});
