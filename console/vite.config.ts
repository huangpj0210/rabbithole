import { readFileSync } from 'node:fs';
import path from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as {
  version: string;
};

/**
 * 控制台构建配置。
 *
 * - 产物固定落在 `console/dist`:`crates/rh-server/build.rs` 只检测该路径,
 *   release.yml 亦按此目录上传工件。
 * - 不产出 sourcemap(内嵌分发,体积与源码暴露都不需要)。
 * - 开发期 `/api` 原样代理到本机 rh-server(无 rewrite);生产同源托管。
 */
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, 'src'),
    },
  },
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  server: {
    port: 8000,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      output: {
        /**
         * 拆 vendor:内嵌产物没有 CDN,拆包主要是让浏览器并行解析、
         * 并让升级时的缓存失效面更小。recharts 只被总览页用到,单独成块。
         */
        manualChunks: {
          react: ['react', 'react-dom', 'react-router'],
          query: ['@tanstack/react-query', '@tanstack/react-table'],
          forms: ['react-hook-form', 'zod', '@hookform/resolvers'],
          charts: ['recharts'],
        },
      },
    },
  },
});
