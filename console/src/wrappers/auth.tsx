import { Navigate, Outlet, useLocation } from '@umijs/max';
import React from 'react';
import { useAuthStore } from '@/stores/auth';

/** 登录守卫:无 token 访问业务路由 → 登录页(保留回跳);有 token 放行子路由。 */
const AuthWrapper: React.FC = () => {
  const token = useAuthStore((s) => s.token);
  const location = useLocation();
  if (!token) {
    const redirect = encodeURIComponent(location.pathname + location.search);
    return <Navigate replace to={`/user/login?redirect=${redirect}`} />;
  }
  return <Outlet />;
};

export default AuthWrapper;
