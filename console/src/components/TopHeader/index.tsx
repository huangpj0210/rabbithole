import React, { useCallback } from 'react';
import { MenuFoldOutlined, MenuUnfoldOutlined } from '@ant-design/icons';
import { useModel } from '@umijs/max';
import { createStyles } from 'antd-style';
import { SelectLang } from '../RightContent';
import { AvatarDropdown, AvatarName } from '../RightContent/AvatarDropdown';

const SIDER_WIDTH = 208;
const SIDER_COLLAPSED_WIDTH = 48;

const useStyles = createStyles(({ token }) => ({
  topHeader: {
    position: 'fixed',
    top: 0,
    right: 0,
    zIndex: 99, // 低于侧边栏的 zIndex (100)，避免遮挡 Logo
    height: 48,
    background: token.colorBgContainer,
    // 使用 border-bottom 代替 box-shadow，避免阴影显示在侧边栏 Logo 上
    borderBottom: `1px solid ${token.colorBorderSecondary}`,
    transition: 'left 0.2s, background 0.3s',
  },
  topHeaderContent: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    height: '100%',
    padding: '0 16px',
  },
  topHeaderLeft: {
    display: 'flex',
    alignItems: 'center',
  },
  topHeaderRight: {
    display: 'flex',
    alignItems: 'center',
    height: '100%',
    gap: 4,
  },
  headerAction: {
    display: 'flex',
    alignItems: 'center',
    height: 48,
    padding: '0 12px',
    cursor: 'pointer',
    transition: 'all 0.3s',
    color: token.colorText,
    '&:hover': {
      background: token.colorBgTextHover,
    },
  },
  collapseButton: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 48,
    height: 48,
    fontSize: 18,
    cursor: 'pointer',
    transition: 'all 0.3s',
    color: token.colorText,
    '&:hover': {
      background: token.colorBgTextHover,
    },
  },
  avatar: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    color: token.colorText,
  },
  avatarImg: {
    width: 28,
    height: 28,
    borderRadius: '50%',
    verticalAlign: 'middle',
  },
}));

interface TopHeaderProps {
  initialState?: {
    collapsed?: boolean;
    currentUser?: {
      avatar?: string;
      name?: string;
    };
  };
}

const TopHeader: React.FC<TopHeaderProps> = ({ initialState }) => {
  const { styles } = useStyles();
  const { setInitialState } = useModel('@@initialState');

  const collapsed = initialState?.collapsed ?? false;

  // 点击折叠按钮 - 通过 setInitialState 触发 ProLayout 更新
  const handleToggleCollapse = useCallback(() => {
    setInitialState((prev: any) => ({
      ...prev,
      collapsed: !prev?.collapsed,
    }));
  }, [setInitialState]);

  return (
    <div
      className={styles.topHeader}
      style={{ left: collapsed ? SIDER_COLLAPSED_WIDTH : SIDER_WIDTH }}
    >
      <div className={styles.topHeaderContent}>
        <div className={styles.topHeaderLeft}>
          {/* 折叠按钮 */}
          <div className={styles.collapseButton} onClick={handleToggleCollapse}>
            {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
          </div>
        </div>
        <div className={styles.topHeaderRight}>
          <div className={styles.headerAction}>
            <SelectLang />
          </div>
          <AvatarDropdown>
            <span className={styles.avatar}>
              {initialState?.currentUser?.avatar && (
                <img
                  src={initialState?.currentUser?.avatar}
                  alt="avatar"
                  className={styles.avatarImg}
                />
              )}
              <AvatarName />
            </span>
          </AvatarDropdown>
        </div>
      </div>
    </div>
  );
};

export default TopHeader;
