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
    flexDirection: 'column',
    height: '100vh',
    overflow: 'auto',
    backgroundImage:
      'linear-gradient(150deg, #e0f2fe 0%, #f0f9ff 40%, #f8fafc 100%)',
    backgroundSize: '100% 100%',
  },
  lang: {
    width: 42,
    height: 42,
    lineHeight: '42px',
    position: 'fixed',
    right: 16,
    borderRadius: token.borderRadius,
    ':hover': {
      backgroundColor: token.colorBgTextHover,
    },
  },
}));

/** 登录页:账号/密码,无第三方/注册/忘记密码入口(契约见 design.md §7.1)。 */
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
      <LoginForm
        logo={<img alt="logo" src="/logo.svg" />}
        title="rabbithole"
        subTitle="端口转发与内网穿透控制台"
        onFinish={handleSubmit}
        submitter={{ searchConfig: { submitText: '登录' }, submitButtonProps: { loading: submitting } }}
      >
        <ProFormText
          name="username"
          fieldProps={{
            size: 'large',
            prefix: <UserOutlined />,
            placeholder: '用户名',
          }}
          placeholder="用户名"
          rules={[{ required: true, message: '请输入用户名' }]}
        />
        <ProFormText.Password
          name="password"
          fieldProps={{
            size: 'large',
            prefix: <LockOutlined />,
            placeholder: '密码',
          }}
          placeholder="密码"
          rules={[{ required: true, message: '请输入密码' }]}
        />
      </LoginForm>
    </div>
  );
};

export default Login;
