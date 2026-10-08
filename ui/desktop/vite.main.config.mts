import { defineConfig } from 'vite';

// https://vitejs.dev/config
export default defineConfig({
  define: {
    'process.env.GITHUB_OWNER': JSON.stringify(process.env.GITHUB_OWNER || 'BezotCorp'),
    'process.env.GITHUB_REPO': JSON.stringify(process.env.GITHUB_REPO || 'ai-platform'),
    'process.env.BCAIP_BUNDLE_NAME': JSON.stringify(process.env.BCAIP_BUNDLE_NAME || 'BCAIP'),
  },
});
