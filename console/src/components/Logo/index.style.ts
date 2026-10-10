import { createStyles } from 'antd-style';

const useStyles = createStyles(({ token }) => {
  return {
    logo: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      height: '64px',
      padding: '0 16px',
      overflow: 'hidden',
      cursor: 'pointer',
      transition: 'all 0.3s',
      textDecoration: 'none',
    },
    logoCollapsed: {
      padding: '0',
      justifyContent: 'center',
    },
    logoImg: {
      width: '32px',
      height: '32px',
      transition: 'all 0.3s',
    },
    logoImgCollapsed: {
      width: '28px',
      height: '28px',
    },
    logoTitle: {
      margin: '0 0 0 12px',
      fontSize: '18px',
      fontWeight: 600,
      color: '#fff',
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      transition: 'all 0.3s',
    },
    logoTitleLight: {
      color: token.colorTextBase,
    },
  };
});

export default useStyles;
