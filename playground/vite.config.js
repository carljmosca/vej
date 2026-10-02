import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    host: true,
    port: 3000,
    open: true,
    hmr: {
      protocol: 'ws',
      host: 'localhost',
      port: 3000
    },
    proxy: {
      '/v1/proxy/jev': {
        target: 'https://api.typesafe.ai',
        changeOrigin: true,
        rewrite: () => '/v1/systemone'
      },
      '/v1': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true
      },
      '/health': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true
      }
    }
  }
});
