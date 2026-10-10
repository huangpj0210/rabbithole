import { createStyles } from 'antd-style';

const useStyles = createStyles(({ token }) => {
  return {
    tabsView: {
      position: 'sticky',
      top: 0,
      zIndex: 99,
      display: 'flex',
      alignItems: 'center',
      background: token.colorBgContainer,
      padding: '6px 12px 0',
      height: '44px',
      boxShadow: '0 1px 4px rgba(0, 21, 41, 0.08)',
      marginBottom: 0,
    },
    tabsViewContent: {
      flex: 1,
      overflow: 'hidden',

      '.ant-tabs': {
        '.ant-tabs-nav': {
          margin: 0,

          '&::before': {
            border: 'none',
          },
        },

        '.ant-tabs-tab': {
          padding: '8px 16px',
          margin: '0 4px 0 0 !important',
          background: token.colorBgContainer,
          border: `1px solid ${token.colorBorderSecondary}`,
          borderRadius: '6px 6px 0 0',
          transition: 'all 0.2s',

          '&:hover': {
            color: token.colorPrimary,
          },

          '&.ant-tabs-tab-active': {
            background: token.colorBgContainer,
            borderBottomColor: token.colorBgContainer,

            '.ant-tabs-tab-btn': {
              color: token.colorPrimary,
            },
          },
        },

        '.ant-tabs-tab-remove': {
          marginLeft: '6px',
          marginRight: '-4px',
          color: token.colorTextTertiary,
          fontSize: '12px',

          '&:hover': {
            color: token.colorText,
          },
        },

        '.ant-tabs-nav-operations': {
          display: 'none',
        },
      },
    },
    tabTitle: {
      display: 'inline-block',
      padding: '0 4px',
      maxWidth: '120px',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      whiteSpace: 'nowrap',
    },
    menuIcon: {
      marginRight: '8px',
    },
  };
});

export default useStyles;
