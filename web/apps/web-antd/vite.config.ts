import { defineConfig } from '@vben/vite-config';

export default defineConfig(async () => {
  return {
    application: {},
    vite: {
      // rabbithole 修复:pnpm 图中 vue-router 存在两个 peer 变体实例
      // (app 与 @vben/* 各解析到一份)。开发模式由 Vite 依赖优化合并为一份,
      // 生产构建(rolldown)会保留两份 → 两套 routerKey 注入符号,
      // 布局内 useRouter() 注入失败 → 登录后主界面空白。
      // 强制构建期按根解析去重,保证全图单实例。
      resolve: {
        dedupe: ['vue', 'vue-router', 'pinia', '@vue/devtools-api'],
      },
      server: {
        proxy: {
          '/api': {
            changeOrigin: true,
            // rabbithole: 开发代理目标为本地 Rust 后端
            // 后端路由以 /api 开头，与 VITE_GLOB_API_URL=/api 保持一致；
            // 若后端路由不带 /api 前缀，请恢复 rewrite 去掉前缀
            target: 'http://127.0.0.1:8080',
            ws: true,
          },
        },
      },
    },
  };
});
