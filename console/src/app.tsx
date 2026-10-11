import type { Settings as LayoutSettings } from '@ant-design/pro-components';
import { SettingDrawer } from '@ant-design/pro-components';
import type { RunTimeLayoutConfig } from '@umijs/max';
import { history } from '@umijs/max';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App as AntApp } from 'antd';
import React, { useEffect } from 'react';
import { userInfoApi } from '@/api/auth';
import { LOGIN_PATH } from '@/api/request';
import { AvatarDropdown, TagsView, TopHeader } from '@/components';
import { bindFeedback } from '@/components/feedback';
import { useAuthStore } from '@/stores/auth';
import defaultSettings from '../config/defaultSettings';
import './global.less';

// 仅真实本地开发展示设置抽屉;绝不受构建机环境变量(CI)影响
// (曾因 define 内联 CI=true 导致 SettingDrawer 泄漏进生产包,§2.1 V3)
const isDev = process.env.NODE_ENV === 'development';

/** 把 antd App 上下文的 message 绑给模块级出口,供 axios 拦截器在 React 树外弹错。 */
function FeedbackBinder() {
  const { message } = AntApp.useApp();
  useEffect(() => {
    bindFeedback(message);
  }, [message]);
  return null;
}

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
});

/**
 * 登录链路(design.md §8.2):
 * token 存 zustand(localStorage 持久化);非登录页时用 token 拉 /user/info,
 * 失败(token 失效/网络)则清空并交给守卫跳登录。
 * @see https://umijs.org/docs/api/runtime-config#getinitialstate
 */
export async function getInitialState(): Promise<{
  settings?: Partial<LayoutSettings>;
  currentUser?: API.CurrentUser;
  loading?: boolean;
  collapsed?: boolean;
  fetchUserInfo?: () => Promise<API.CurrentUser | undefined>;
}> {
  const fetchUserInfo = async () => {
    const token = useAuthStore.getState().token;
    if (!token) {
      return undefined;
    }
    try {
      const info = await userInfoApi();
      const user = { name: info?.username || 'admin', avatar: '' };
      useAuthStore.getState().setAuth(token, user.name);
      return user;
    } catch {
      // 401 拦截器已清 token;其他失败保持现状,守卫按无 token 处理
      return undefined;
    }
  };
  const { location } = history;
  if (location.pathname !== LOGIN_PATH) {
    const currentUser = await fetchUserInfo();
    return {
      fetchUserInfo,
      currentUser,
      collapsed: false,
      settings: defaultSettings as Partial<LayoutSettings>,
    };
  }
  return {
    fetchUserInfo,
    collapsed: false,
    settings: defaultSettings as Partial<LayoutSettings>,
  };
}

// ProLayout 支持的 api https://procomponents.ant.design/components/layout
export const layout: RunTimeLayoutConfig = ({ initialState, setInitialState }) => {
  const settings = (initialState?.settings || defaultSettings) as typeof defaultSettings;
  const currentLayout = settings.layout || 'side';
  const isSideLayout = currentLayout === 'side';
  const showHeader = settings.headerRender !== false;
  const showMenu = settings.menuRender !== false;
  const showMenuHeader = settings.menuHeaderRender !== false;

  return {
    layout: currentLayout,
    navTheme: settings.navTheme || 'light',
    colorPrimary: settings.colorPrimary,
    fixedHeader: isSideLayout ? false : (settings.fixedHeader ?? true),
    fixSiderbar: settings.fixSiderbar ?? true,
    siderWidth: settings.siderWidth || 208,
    title: settings.title,
    logo: settings.logo,
    contentWidth: settings.contentWidth || 'Fluid',
    siderMenuType: settings.siderMenuType || 'sub',

    menuRender: showMenu ? undefined : false,
    // 侧栏头部固定为产品标识(ProLayout 默认标题是模板字样,显式覆盖)
    menuHeaderRender: showMenuHeader
      ? (_logo: React.ReactNode, _title: React.ReactNode, props?: { collapsed?: boolean }) => (
          <a
            href="/rabbithole/overview"
            style={{
              alignItems: 'center',
              color: 'inherit',
              display: 'flex',
              fontSize: 16,
              fontWeight: 600,
              gap: 8,
              height: '100%',
              justifyContent: props?.collapsed ? 'center' : 'flex-start',
              overflow: 'hidden',
              padding: '0 12px',
            }}
          >
            <img alt="rabbithole" height={26} src="/logo.svg" width={26} />
            {!props?.collapsed && <span>rabbithole</span>}
          </a>
        )
      : false,

    // side 布局:禁用默认 header,使用自定义 TopHeader
    headerRender: isSideLayout ? false : showHeader ? undefined : false,
    rightContentRender: isSideLayout ? false : undefined,

    ...(isSideLayout
      ? {}
      : {
          actionsRender: () => [],
          avatarProps: {
            src: initialState?.currentUser?.avatar,
            title: initialState?.currentUser?.name,
            render: (_: any, avatarChildren: any) => (
              <AvatarDropdown>{avatarChildren}</AvatarDropdown>
            ),
          },
        }),

    collapsed: initialState?.collapsed,
    onCollapse: (collapsed: boolean) => {
      setInitialState((prev) => ({ ...prev, collapsed }));
    },
    collapsedButtonRender: isSideLayout ? false : undefined,

    // 水印移除(§2.1 V2):对单管理员自托管场景是纯噪音

    // 版本号入侧栏底部(§5);Footer 独立页脚已移除
    menuFooterRender: () => (
      <div style={{ padding: '8px 16px', fontSize: 12, color: 'rgba(0,0,0,0.45)' }}>
        rabbithole v{String(process.env.APP_VERSION ?? '').replace(/"/g, '')}
      </div>
    ),

    footerRender: false,

    onPageChange: () => {
      const { location } = history;
      if (!initialState?.currentUser && location.pathname !== LOGIN_PATH) {
        history.push(LOGIN_PATH);
      }
    },

    childrenRender: (children) => {
      return (
        <>
          <FeedbackBinder />
          {isSideLayout && showHeader && showMenu && <TopHeader initialState={initialState} />}
          {settings?.multiTabs !== false && <TagsView />}
          {children}
          {isDev && (
            <SettingDrawer
              disableUrlParams
              enableDarkTheme
              settings={initialState?.settings}
              onSettingChange={(newSettings) => {
                setInitialState((preInitialState) => ({
                  ...preInitialState,
                  settings: newSettings,
                }));
              }}
            />
          )}
        </>
      );
    },

    token: settings.token,
  };
};

/** TanStack Query 根容器挂载。 */
export function rootContainer(container: React.ReactNode) {
  return <QueryClientProvider client={queryClient}>{container}</QueryClientProvider>;
}
