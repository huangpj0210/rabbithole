// https://umijs.org/config/
import { join } from 'node:path';
import { defineConfig } from '@umijs/max';
import defaultSettings from './defaultSettings';
import proxy from './proxy';
import routes from './routes';

const { REACT_APP_ENV = 'dev' } = process.env;

export default defineConfig({
  /**
   * 产物带 hash 后缀,配合 rh-server 内嵌静态托管避免浏览器缓存旧版本。
   */
  hash: true,
  publicPath: '/',

  routes,

  ignoreMomentLocale: true,

  /**
   * 本地联调代理:dev server 的 /api 原样转发给 rh-server(无 rewrite),
   * 生产构建不走代理,由 rh-server 同源托管 /api。
   */
  proxy: proxy[REACT_APP_ENV as keyof typeof proxy],

  fastRefresh: true,

  //============== max 插件 ===============
  model: {},
  initialState: {},

  title: 'rabbithole 控制台',
  layout: {
    locale: false,
    ...defaultSettings,
  },

  moment2dayjs: {
    preset: 'antd',
    plugins: ['duration'],
  },

  locale: {
    default: 'zh-CN',
    antd: true,
    baseNavigator: false,
  },

  antd: {
    appConfig: {},
    configProvider: {
      theme: {
        cssVar: true,
      },
    },
  },

  access: {},

  headScripts: [
    // 首屏 loading 动画,避免 JS 未就绪时白屏
    { src: join('/', 'scripts/loading.js'), async: true },
  ],

  presets: ['umi-presets-pro'],

  mako: {},
  esbuildMinifyIIFE: true,

  define: {
    'process.env.CI': process.env.CI,
  },
});
