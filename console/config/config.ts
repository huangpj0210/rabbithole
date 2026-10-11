// https://umijs.org/config/
import { readFileSync } from 'node:fs';
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
        token: {
          // 形状(§4.4):控件 6px;容器卡片 8px 在各卡片 borderRadius 指定
          borderRadius: 6,
          borderRadiusLG: 8,
          wireframe: false,
        },
      },
    },
  },

  access: {},

  headScripts: [
    // 首屏 loading 动画,避免 JS 未就绪时白屏
    { src: join('/', 'scripts/loading.js'), async: true },
  ],

  links: [
    // 字体自托管(§4.3):离线环境零第三方请求;两个最常用字重预加载
    { rel: 'preload', as: 'font', type: 'font/woff2', href: '/fonts/AlibabaSans-Regular.woff2', crossorigin: 'anonymous' },
    { rel: 'preload', as: 'font', type: 'font/woff2', href: '/fonts/AlibabaSans-Medium.woff2', crossorigin: 'anonymous' },
  ],

  presets: ['umi-presets-pro'],

  mako: {},
  esbuildMinifyIIFE: true,

  define: {
    // 版本号入侧栏底部(§5);绝不内联 CI 等构建机环境变量——
    // 曾导致 SettingDrawer 泄漏进生产包(console-ui-redesign §2.1 V3)
    'process.env.APP_VERSION': JSON.stringify(
      JSON.parse(readFileSync(join(__dirname, '..', 'package.json'), 'utf8')).version,
    ),
  },
});
