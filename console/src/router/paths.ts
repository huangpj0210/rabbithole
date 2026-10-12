/**
 * 路由路径表(与旧版逐字一致,URL 稳定)。
 * 旧 vben 路径 /rabbithole/services、/rabbithole/visitors 保留重定向。
 */
export const ROUTES = {
  login: '/user/login',
  overview: '/rabbithole/overview',
  clients: '/rabbithole/clients',
  tcp: '/rabbithole/tcp',
  udp: '/rabbithole/udp',
  domains: '/rabbithole/domains',
  secret: '/rabbithole/secret',
  certs: '/rabbithole/certs',
  settings: '/rabbithole/settings',
} as const;

/** 侧栏分组(两组 + 设置),路径与旧版一致。 */
export const NAV_GROUPS = [
  {
    label: null,
    items: [
      { title: '总览', to: ROUTES.overview, icon: 'dashboard' },
      { title: '客户端', to: ROUTES.clients, icon: 'clients' },
    ],
  },
  {
    label: '转发',
    items: [
      { title: 'TCP 隧道', to: ROUTES.tcp, icon: 'tcp' },
      { title: 'UDP 隧道', to: ROUTES.udp, icon: 'udp' },
      { title: '域名服务', to: ROUTES.domains, icon: 'domains' },
      { title: '密钥访问', to: ROUTES.secret, icon: 'secret' },
      { title: '证书', to: ROUTES.certs, icon: 'certs' },
    ],
  },
  {
    label: null,
    items: [{ title: '设置', to: ROUTES.settings, icon: 'settings' }],
  },
] as const;
