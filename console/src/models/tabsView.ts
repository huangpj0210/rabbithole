/**
 * 多标签页状态管理
 * Umi useModel 格式
 */
import { useState, useCallback } from 'react';
import { storage } from '@/utils/storage';

const TABS_ROUTES = 'TABS-ROUTES';

export interface TabItem {
  path: string;
  name: string;
  title: string;
  query?: Record<string, string>;
  affix?: boolean;
}

// Routes to exclude from tabs
const whiteList = ['login', 'redirect', '/user/login', '/user/register'];

export default function useTabsViewModel() {
  const [tabsList, setTabsList] = useState<TabItem[]>(() => {
    return storage.get<TabItem[]>(TABS_ROUTES, []) || [];
  });

  const saveTabs = useCallback((tabs: TabItem[]) => {
    storage.set(TABS_ROUTES, tabs);
  }, []);

  const addTab = useCallback(
    (route: { pathname: string; name?: string; title?: string; query?: Record<string, string>; affix?: boolean }) => {
      // Exclude whitelist routes
      if (whiteList.some((w) => route.pathname.includes(w))) return;
      // Exclude redirect paths
      if (route.pathname.startsWith('/redirect')) return;

      setTabsList((prev) => {
        const existIndex = prev.findIndex((item) => item.path === route.pathname);
        let newTabs: TabItem[];

        if (existIndex !== -1) {
          // Update existing tab
          newTabs = [...prev];
          newTabs[existIndex] = {
            path: route.pathname,
            name: route.name || route.pathname,
            title: route.title || route.name || route.pathname,
            query: route.query,
            affix: route.affix,
          };
        } else {
          // Add new tab
          newTabs = [
            ...prev,
            {
              path: route.pathname,
              name: route.name || route.pathname,
              title: route.title || route.name || route.pathname,
              query: route.query,
              affix: route.affix,
            },
          ];
        }

        saveTabs(newTabs);
        return newTabs;
      });
    },
    [saveTabs]
  );

  const closeTab = useCallback(
    (path: string) => {
      setTabsList((prev) => {
        const tab = prev.find((item) => item.path === path);
        // Don't close affix tabs
        if (tab?.affix) return prev;

        const newTabs = prev.filter((item) => item.path !== path);
        saveTabs(newTabs);
        return newTabs;
      });
    },
    [saveTabs]
  );

  const closeOtherTabs = useCallback(
    (path: string) => {
      setTabsList((prev) => {
        const newTabs = prev.filter((item) => item.path === path || item.affix);
        saveTabs(newTabs);
        return newTabs;
      });
    },
    [saveTabs]
  );

  const closeAllTabs = useCallback(() => {
    setTabsList((prev) => {
      const newTabs = prev.filter((item) => item.affix);
      saveTabs(newTabs);
      return newTabs;
    });
  }, [saveTabs]);

  const closeLeftTabs = useCallback(
    (path: string) => {
      setTabsList((prev) => {
        const index = prev.findIndex((item) => item.path === path);
        if (index <= 0) return prev;

        const newTabs = prev.filter((item, i) => i >= index || item.affix);
        saveTabs(newTabs);
        return newTabs;
      });
    },
    [saveTabs]
  );

  const closeRightTabs = useCallback(
    (path: string) => {
      setTabsList((prev) => {
        const index = prev.findIndex((item) => item.path === path);
        if (index >= prev.length - 1) return prev;

        const newTabs = prev.filter((item, i) => i <= index || item.affix);
        saveTabs(newTabs);
        return newTabs;
      });
    },
    [saveTabs]
  );

  const clearTabs = useCallback(() => {
    storage.remove(TABS_ROUTES);
    setTabsList([]);
  }, []);

  const updateTabTitle = useCallback(
    (path: string, title: string) => {
      setTabsList((prev) => {
        const index = prev.findIndex((item) => item.path === path);
        if (index === -1) return prev;

        const newTabs = [...prev];
        newTabs[index] = { ...newTabs[index]!, title };
        saveTabs(newTabs);
        return newTabs;
      });
    },
    [saveTabs]
  );

  const setTabAffix = useCallback(
    (path: string, affix: boolean) => {
      setTabsList((prev) => {
        const index = prev.findIndex((item) => item.path === path);
        if (index === -1) return prev;

        const newTabs = [...prev];
        newTabs[index] = { ...newTabs[index]!, affix };
        saveTabs(newTabs);
        return newTabs;
      });
    },
    [saveTabs]
  );

  return {
    tabsList,
    addTab,
    closeTab,
    closeOtherTabs,
    closeAllTabs,
    closeLeftTabs,
    closeRightTabs,
    clearTabs,
    updateTabTitle,
    setTabAffix,
  };
}
