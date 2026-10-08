import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

// https://vitejs.dev/config
export default defineConfig({
  define: {
    'process.env.BCAIP_TUNNEL': JSON.stringify(process.env.BCAIP_TUNNEL !== 'no' && process.env.BCAIP_TUNNEL !== 'none'),
  },

  plugins: [tailwindcss()],

  // Vite caches a copy of @bezotcorp/bcaip-acp-client and doesn't notice when we rebuild it
  // locally, so it serves stale code until you clear node_modules/.vite by hand.
  // Excluding it makes Vite always read the latest ui/goose-acp-client/dist build.
  // Dev-server only — release builds ignore optimizeDeps.
  optimizeDeps: {
    exclude: ['@bezotcorp/bcaip-acp-client'],
  },

  build: {
    target: 'esnext'
  },
});
