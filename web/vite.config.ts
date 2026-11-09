import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  resolve: { dedupe: ['vue'] },
  optimizeDeps: { include: ['vue', 'vue-router', 'pinia', 'naive-ui', 'splitpanes'] },
  server: {
    port: 5173,
    watch: { usePolling: true, interval: 300 },
    // 本地开发代理到 Rust 后端
    proxy: { '/api': 'http://127.0.0.1:7600' },
  },
  build: { chunkSizeWarningLimit: 4000 },
})
