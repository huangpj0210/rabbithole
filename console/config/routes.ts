/**
 * rabbithole 路由(console-ui-redesign §5:两组 IA,路径全部不变)。
 * 平台面(总览/客户端) + 转发面(TCP/UDP/域名/密钥/证书) + 设置;
 * wrappers 的 auth 为登录守卫;旧 vben 路由做重定向兼容。
 */
export default [
  {
    path: '/user/login',
    layout: false,
    name: '登录',
    component: './user/login',
  },
  {
    path: '/user',
    layout: false,
    redirect: '/user/login',
  },

  // 旧路由兼容(vben 版路径)
  { path: '/rabbithole/services', redirect: '/rabbithole/tcp' },
  { path: '/rabbithole/visitors', redirect: '/rabbithole/secret?tab=visitors' },

  { path: '/rabbithole', redirect: '/rabbithole/overview' },
  {
    name: '总览',
    icon: 'dashboard',
    path: '/rabbithole/overview',
    component: './overview',
    wrappers: ['@/wrappers/auth'],
  },
  {
    name: '客户端',
    icon: 'cluster',
    path: '/rabbithole/clients',
    component: './clients',
    wrappers: ['@/wrappers/auth'],
  },
  {
    name: '转发',
    icon: 'shareAlt',
    // key 必须非空:ProLayout 计算 selectedKeys 用的 menuData 不过滤 redirect,
    // "/rabbithole" 重定向项会以路径前缀匹配所有子页混入 selectedKeys;rc-menu
    // 对这种未注册 key 的 keyPath 退化为 [''],若父菜单 eventKey 恰为空串
    // (无 path 的父级默认如此)会被误判为"子级选中"而常亮。
    key: 'forward',
    routes: [
      {
        name: 'TCP 隧道',
        icon: 'apartment',
        path: '/rabbithole/tcp',
        component: './tcp',
        wrappers: ['@/wrappers/auth'],
      },
      {
        name: 'UDP 隧道',
        icon: 'send',
        path: '/rabbithole/udp',
        component: './udp',
        wrappers: ['@/wrappers/auth'],
      },
      {
        name: '域名服务',
        icon: 'global',
        path: '/rabbithole/domains',
        component: './domains',
        wrappers: ['@/wrappers/auth'],
      },
      {
        name: '密钥访问',
        icon: 'key',
        path: '/rabbithole/secret',
        component: './secret',
        wrappers: ['@/wrappers/auth'],
      },
      {
        name: '证书',
        icon: 'safetyCertificate',
        path: '/rabbithole/certs',
        component: './certs',
        wrappers: ['@/wrappers/auth'],
      },
    ],
  },
  {
    name: '设置',
    icon: 'setting',
    path: '/rabbithole/settings',
    component: './settings',
    wrappers: ['@/wrappers/auth'],
  },

  { path: '/', redirect: '/rabbithole/overview' },
  {
    component: './404',
    path: '/*',
  },
];
