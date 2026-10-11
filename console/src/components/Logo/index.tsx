import React from 'react';
import { Link } from '@umijs/max';
import useStyles from './index.style';

export interface LogoProps {
  collapsed?: boolean;
  title?: string;
  logo?: string | React.ReactNode;
  theme?: 'dark' | 'light';
}

const Logo: React.FC<LogoProps> = ({
  collapsed = false,
  title = 'rabbithole',
  logo = '/logo.svg',
  theme = 'dark',
}) => {
  const { styles, cx } = useStyles();

  const renderLogo = () => {
    if (typeof logo === 'string') {
      return (
        <img
          src={logo}
          alt="logo"
          className={cx(styles.logoImg, collapsed && styles.logoImgCollapsed)}
        />
      );
    }
    return logo;
  };

  return (
    <Link to="/" className={cx(styles.logo, collapsed && styles.logoCollapsed)}>
      {renderLogo()}
      {!collapsed && (
        <h1 className={cx(styles.logoTitle, theme === 'light' && styles.logoTitleLight)}>
          {title}
        </h1>
      )}
    </Link>
  );
};

export default Logo;
