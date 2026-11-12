import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

const host = process.env.TAURI_DEV_HOST;

// The PDF viewer (pdf.js, `src/lib/pdfPages.js`) draws text that a PDF doesn't embed (the standard
// fonts, such as the Helvetica LocalBooks' own forms use) from these font files. They are served
// from the app itself, as the content security policy requires: copied out of node_modules into the
// build, and served the same way while developing. Nothing is fetched from elsewhere.
const FONTS_DIR = join(dirname(fileURLToPath(import.meta.url)), 'node_modules/pdfjs-dist/standard_fonts');
const FONTS_URL = 'pdfjs/standard_fonts/';
function pdfStandardFonts() {
  const files = () => readdirSync(FONTS_DIR).filter((f) => !f.startsWith('.'));
  return {
    name: 'pdf-standard-fonts',
    configureServer(server) {
      // Under the app's base path, as `import.meta.env.BASE_URL` builds the URL in pdfPages.js.
      server.middlewares.use(`${server.config.base}${FONTS_URL}`, (req, res, next) => {
        let name;
        try {
          name = decodeURIComponent((req.url ?? '').replace(/^\//, '').split('?')[0]);
        } catch {
          return next();
        }
        if (!files().includes(name)) return next();
        res.setHeader('Content-Type', 'application/octet-stream');
        res.end(readFileSync(join(FONTS_DIR, name)));
      });
    },
    generateBundle() {
      for (const name of files()) this.emitFile({ type: 'asset', fileName: `${FONTS_URL}${name}`, source: readFileSync(join(FONTS_DIR, name)) });
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), pdfStandardFonts()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    rollupOptions: {
      output: {
        // Libraries (React, Tauri API, icons) change rarely: keep them in their own chunk. The PDF
        // viewer is large and loads only when a PDF is previewed.
        manualChunks: (id) => (id.includes('pdfjs-dist') ? 'pdfjs' : id.includes('node_modules') ? 'vendor' : undefined),
      },
    },
  },
});
