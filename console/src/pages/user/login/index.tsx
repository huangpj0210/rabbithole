import { LockOutlined, UserOutlined } from '@ant-design/icons';
import { LoginForm, ProFormText } from '@ant-design/pro-components';
import { history, useModel } from '@umijs/max';
import { App } from 'antd';
import { createStyles } from 'antd-style';
import React, { useEffect, useState } from 'react';
import { flushSync } from 'react-dom';
import { codesApi, loginApi, userInfoApi } from '@/api/auth';
import { safeRedirect } from '@/api/request';
import { useAuthStore } from '@/stores/auth';

const useStyles = createStyles(({ token }) => ({
  container: {
    display: 'flex',
    minHeight: '100vh',
    // 品牌瞬间(§4.5):仅登录页允许一次入场过渡;reduced-motion 由 global.less 静止
    '@media (prefers-reduced-motion: no-preference)': {
      '& .rh-brand, & .rh-login-panel': {
        animation: 'rh-fade-in 320ms ease-out both',
      },
    },
  },
  brand: {
    flex: '1 1 62%',
    display: 'flex',
    flexDirection: 'column',
    justifyContent: 'center',
    padding: '64px 72px',
    background:
      'linear-gradient(160deg, #f4f9ff 0%, #eef5fb 45%, #f7fafd 100%)',
    position: 'relative',
    overflow: 'hidden',
    '@media (max-width: 768px)': {
      // 移动端品牌区收成顶部窄条(§6.1)
      display: 'none',
    },
  },
  brandTitle: {
    fontSize: 40,
    fontWeight: 700,
    letterSpacing: 1,
    margin: 0,
    color: 'rgba(0,0,0,0.88)',
  },
  brandDesc: {
    fontSize: 16,
    lineHeight: 1.8,
    color: 'rgba(0,0,0,0.55)',
    maxWidth: 460,
    marginTop: 16,
  },
  // "洞层"主题图形:三条越走越深的隧道线,克制表达(§4.1)
  brandArt: {
    marginTop: 56,
    fontFamily: "ui-monospace, 'SF Mono', Menlo, Consolas, monospace",
    fontSize: 13,
    lineHeight: 2.1,
    color: 'rgba(0,0,0,0.38)',
    fontVariantNumeric: 'tabular-nums',
    whiteSpace: 'pre',
  },
  loginPanel: {
    flex: '1 1 38%',
    display: 'flex',
    flexDirection: 'column',
    justifyContent: 'center',
    alignItems: 'center',
    padding: '32px 24px',
    background: '#fff',
    minWidth: 320,
  },
  mobileBrand: {
    display: 'none',
    '@media (max-width: 768px)': {
      display: 'block',
      width: '100%',
      padding: '20px 24px 4px',
      fontSize: 18,
      fontWeight: 600,
    },
  },
  loginBox: {
    width: '100%',
    maxWidth: 320,
  },
  loginFooter: {
    marginTop: 24,
    fontSize: 12,
    color: 'rgba(0,0,0,0.35)',
  },
  keyframes: {
    '@keyframes rh-fade-in': {
      from: { opacity: 0, transform: 'translateY(8px)' },
      to: { opacity: 1, transform: 'translateY(0)' },
    },
  },
}));

/** 登录页:唯一品牌位(§6.1)。账号/密码,无第三方/注册入口(契约见 design.md §7.1)。 */
const Login: React.FC = () => {
  const [submitting, setSubmitting] = useState(false);
  const { initialState, setInitialState } = useModel('@@initialState');
  const { styles } = useStyles();
  const { message } = App.useApp();
  const token = useAuthStore((s) => s.token);

  // 已登录访问登录页 → 回控制台
  useEffect(() => {
    if (token && initialState?.currentUser) {
      history.push('/rabbithole/overview');
    }
  }, [token, initialState?.currentUser]);

  const handleSubmit = async (values: { username: string; password: string }) => {
    setSubmitting(true);
    try {
      const { accessToken } = await loginApi(values);
      useAuthStore.getState().setAuth(accessToken, values.username);
      // 登录后并行请求 user/info + auth/codes(codes 预期 [],失败不阻断)
      const [info] = await Promise.all([
        userInfoApi().catch(() => null),
        codesApi().catch(() => []),
      ]);
      const user = { name: info?.username || values.username, avatar: '' };
      useAuthStore.getState().setAuth(accessToken, user.name);
      flushSync(() => {
        setInitialState((s) => ({ ...s, currentUser: user }));
      });
      message.success('登录成功');
      const redirect = safeRedirect(new URLSearchParams(window.location.search).get('redirect'));
      history.push(redirect || '/rabbithole/overview');
      return;
    } catch {
      // 错误提示由响应拦截器统一弹出
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className={styles.container}>
      <div className={styles.brand}>
        <h1 className={styles.brandTitle}>rabbithole</h1>
        <p className={styles.brandDesc}>
          自托管的端口转发与内网穿透平台。把公网的一个入口,变成通往内网每台机器的隧道。
        </p>
        <div className={styles.brandArt}>
          {'公网入口    0.0.0.0:20000 ──▶ 隧道 ──▶ 内网 10.0.0.1:22\n'}
          {'域名入口    app.example.com ──▶ 隧道 ──▶ 内网 10.0.0.2:8080\n'}
          {'密钥入口    visitor:6000 ──▶ 打洞直连 ──▶ 内网 10.0.0.3:5432'}
        </div>
      </div>
      <div className={styles.loginPanel}>
        <div className={styles.mobileBrand}>rabbithole</div>
        <div className={styles.loginBox}>
          <LoginForm
            logo={<img alt="logo" src="/logo.svg" width={44} height={44} />}
            title="控制台"
            subTitle="登录后管理客户端、隧道与证书"
            onFinish={handleSubmit}
            submitter={{
              searchConfig: { submitText: '进入控制台' },
              submitButtonProps: { loading: submitting, block: true },
            }}
          >
            <ProFormText
            name="username"
            fieldProps={{
              size: 'large',
              prefix: <UserOutlined />,
              placeholder: '用户名(默认 admin)',
              autoComplete: 'username',
            }}
            placeholder="用户名"
            rules={[{ required: true, message: '请输入用户名' }]}
          />
          <ProFormText.Password
            name="password"
            fieldProps={{
              size: 'large',
              prefix: <LockOutlined />,
              placeholder: '密码(首启随机密码见服务端日志)',
              autoComplete: 'current-password',
            }}
            placeholder="密码"
            rules={[{ required: true, message: '请输入密码' }]}
          />
          </LoginForm>
        </div>
        <div className={styles.loginFooter}>
          忘记密码?重启服务端并删除数据目录中的数据库可重新引导(数据将丢失)
        </div>
      </div>
    </div>
  );
};

export default Login;
