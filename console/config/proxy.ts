/**
 * 本地开发代理:/api 原样转发给本机 rh-server,无 rewrite。
 * 生产环境由 rh-server 同源托管,代理不生效。
 * /metrics 与管理 API 同端口,一并转发:设置页展示的抓取地址在开发模式下才可用,
 * 否则打到 umi dev server 返回前端页面。
 */
export default {
  dev: {
    '/api/': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
    '/metrics': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
  },
  test: {
    '/api/': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
    '/metrics': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
  },
  pre: {
    '/api/': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
    '/metrics': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
  },
};
