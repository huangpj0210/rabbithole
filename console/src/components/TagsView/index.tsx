import React, { useEffect, useCallback } from 'react';
import { useLocation, useNavigate, useModel, useIntl, useSelectedRoutes } from '@umijs/max';
import { Tabs, Dropdown } from 'antd';
import type { MenuProps } from 'antd';
import {
  ReloadOutlined,
  CloseOutlined,
  VerticalLeftOutlined,
  VerticalRightOutlined,
} from '@ant-design/icons';
import useStyles from './index.style';

const TagsView: React.FC = () => {
  const { styles } = useStyles();
  const location = useLocation();
  const navigate = useNavigate();
  const intl = useIntl();
  const selectedRoutes = useSelectedRoutes();

  const {
    tabsList,
    addTab,
    closeTab,
    closeOtherTabs,
    closeLeftTabs,
    closeRightTabs,
  } = useModel('tabsView');

  // Get current route title from matched routes using i18n
  const getRouteTitle = useCallback(() => {
    if (selectedRoutes && selectedRoutes.length > 0) {
      // Build the menu i18n key from matched routes
      const routeNames: string[] = [];
      for (const routeMatch of selectedRoutes) {
        const route = routeMatch?.route as { name?: string };
        if (route?.name) {
          routeNames.push(route.name);
        }
      }

      if (routeNames.length > 0) {
        // Build i18n key like: menu.dashboard.analysis
        const menuKey = `menu.${routeNames.join('.')}`;
        const title = intl.formatMessage({
          id: menuKey,
          defaultMessage: routeNames[routeNames.length - 1],
        });
        return title;
      }
    }

    // Fallback: use path as title
    if (location.pathname === '/') {
      return intl.formatMessage({ id: 'menu.home', defaultMessage: '首页' });
    }
    const pathParts = location.pathname.split('/').filter(Boolean);
    return pathParts[pathParts.length - 1] || 'Home';
  }, [selectedRoutes, location.pathname, intl]);

  // Add tab when route changes
  useEffect(() => {
    if (location.pathname) {
      const title = getRouteTitle();
      addTab({
        pathname: location.pathname,
        name: location.pathname,
        title: title,
      });
    }
  }, [location.pathname, addTab, getRouteTitle]);

  const handleTabChange = useCallback(
    (activeKey: string) => {
      navigate(activeKey);
    },
    [navigate]
  );

  const handleClose = useCallback(
    (path: string) => {
      const currentPath = location.pathname;

      closeTab(path);

      // If closing current tab, navigate to the last tab
      if (path === currentPath) {
        const remainingTabs = tabsList.filter((tab) => tab.path !== path);
        if (remainingTabs.length > 0) {
          const lastTab = remainingTabs[remainingTabs.length - 1];
          if (lastTab) {
            navigate(lastTab.path);
          }
        } else {
          navigate('/');
        }
      }
    },
    [location.pathname, closeTab, tabsList, navigate]
  );

  const onEdit = useCallback(
    (
      targetKey: React.MouseEvent | React.KeyboardEvent | string,
      action: 'add' | 'remove'
    ) => {
      if (action === 'remove' && typeof targetKey === 'string') {
        handleClose(targetKey);
      }
    },
    [handleClose]
  );

  const handleContextMenu = useCallback(
    (key: string, targetPath: string) => {
      switch (key) {
        case 'reload':
          // Navigate to the target tab first if not current
          if (location.pathname !== targetPath) {
            navigate(targetPath);
          }
          // Use redirect pattern to reload
          setTimeout(() => {
            navigate(`/redirect${targetPath}`);
          }, 0);
          break;
        case 'closeOther':
          closeOtherTabs(targetPath);
          if (location.pathname !== targetPath) {
            navigate(targetPath);
          }
          break;
        case 'closeLeft':
          closeLeftTabs(targetPath);
          break;
        case 'closeRight':
          closeRightTabs(targetPath);
          break;
      }
    },
    [location.pathname, navigate, closeOtherTabs, closeLeftTabs, closeRightTabs]
  );

  const getContextMenuItems = (tabPath: string): MenuProps['items'] => [
    {
      key: 'reload',
      icon: <ReloadOutlined />,
      label: intl.formatMessage({ id: 'component.tagsView.reload', defaultMessage: '重新加载' }),
      onClick: () => handleContextMenu('reload', tabPath),
    },
    {
      type: 'divider',
    },
    {
      key: 'closeOther',
      icon: <CloseOutlined />,
      label: intl.formatMessage({ id: 'component.tagsView.closeOther', defaultMessage: '关闭其他' }),
      onClick: () => handleContextMenu('closeOther', tabPath),
    },
    {
      key: 'closeLeft',
      icon: <VerticalRightOutlined />,
      label: intl.formatMessage({ id: 'component.tagsView.closeLeft', defaultMessage: '关闭左侧' }),
      onClick: () => handleContextMenu('closeLeft', tabPath),
    },
    {
      key: 'closeRight',
      icon: <VerticalLeftOutlined />,
      label: intl.formatMessage({ id: 'component.tagsView.closeRight', defaultMessage: '关闭右侧' }),
      onClick: () => handleContextMenu('closeRight', tabPath),
    },
  ];

  const tabItems = tabsList.map((tab) => ({
    key: tab.path,
    label: (
      <Dropdown menu={{ items: getContextMenuItems(tab.path) }} trigger={['contextMenu']}>
        <span className={styles.tabTitle}>{tab.title}</span>
      </Dropdown>
    ),
    closable: !tab.affix && tabsList.length > 1,
  }));

  return (
    <div className={styles.tabsView}>
      <div className={styles.tabsViewContent}>
        <Tabs
          activeKey={location.pathname}
          type="editable-card"
          size="small"
          hideAdd
          items={tabItems}
          onChange={handleTabChange}
          onEdit={onEdit}
        />
      </div>
    </div>
  );
};

export default TagsView;
