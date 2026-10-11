import type { ProLayoutProps } from '@ant-design/pro-components';

/**
 * rabbithole 控制台布局配置(ProLayout)。
 */
const Settings: ProLayoutProps & {
  pwa?: boolean;
  logo?: string;
  /** 是否显示多标签页 */
  multiTabs?: boolean;
} = {
  navTheme: 'light',
  colorPrimary: '#1890ff',
  layout: 'side',
  contentWidth: 'Fluid',
  fixedHeader: true,
  fixSiderbar: true,
  colorWeak: false,
  title: 'rabbithole',
  pwa: false,
  logo: '/logo.svg',
  iconfontUrl: '',
  // TagsView 默认关闭(§5):8 页小控制台不需要多标签 chrome
  multiTabs: false,
  siderWidth: 208,
  footerRender: false,
};

export default Settings;
