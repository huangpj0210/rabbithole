import type { RouteRecordRaw } from 'vue-router';

const routes: RouteRecordRaw[] = [
  {
    meta: {
      icon: 'lucide:rabbit',
      order: -2,
      title: 'rabbithole',
    },
    name: 'Rabbithole',
    path: '/rabbithole',
    children: [
      {
        name: 'RhOverview',
        path: 'overview',
        component: () => import('#/views/rabbithole/overview/index.vue'),
        meta: { icon: 'lucide:gauge', title: '总览' },
      },
      {
        name: 'RhClients',
        path: 'clients',
        component: () => import('#/views/rabbithole/clients/index.vue'),
        meta: { icon: 'lucide:monitor-smartphone', title: '客户端' },
      },
      {
        name: 'RhServices',
        path: 'services',
        component: () => import('#/views/rabbithole/services/index.vue'),
        meta: { icon: 'lucide:share-2', title: '转发规则' },
      },
      {
        name: 'RhVisitors',
        path: 'visitors',
        component: () => import('#/views/rabbithole/visitors/index.vue'),
        meta: { icon: 'lucide:waypoints', title: '访问器' },
      },
      {
        name: 'RhSettings',
        path: 'settings',
        component: () => import('#/views/rabbithole/settings/index.vue'),
        meta: { icon: 'lucide:settings', title: '设置' },
      },
    ],
  },
];

export default routes;
