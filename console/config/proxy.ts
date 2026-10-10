/**
 * 本地开发代理:/api 原样转发给本机 rh-server,无 rewrite。
 * 生产环境由 rh-server 同源托管,代理不生效。
 */
export default {
  dev: {
    '/api/': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
  },
  test: {
    '/api/': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
  },
  pre: {
    '/api/': {
      target: 'http://127.0.0.1:8080',
      changeOrigin: false,
    },
  },
};
