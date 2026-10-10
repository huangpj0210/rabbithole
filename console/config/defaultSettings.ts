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
  multiTabs: true,
  siderWidth: 208,
};

export default Settings;
