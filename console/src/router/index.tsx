import { Navigate, createBrowserRouter } from 'react-router';

import { ConsoleLayout } from '@/components/layout/console-layout';
import CertsPage from '@/pages/certs';
import ClientsPage from '@/pages/clients';
import DomainsPage from '@/pages/domains';
import { ForbiddenPage, NotFoundPage, RouteErrorPage } from '@/pages/error-page';
import LoginPage from '@/pages/login';
import OverviewPage from '@/pages/overview';
import SecretPage from '@/pages/secret';
import SettingsPage from '@/pages/settings';
import TcpPage from '@/pages/tcp';
import UdpPage from '@/pages/udp';
import { RequireAuth } from '@/router/require-auth';
import { ROUTES } from '@/router/paths';

export const router = createBrowserRouter([
  {
    path: '/',
    element: <Navigate to={ROUTES.overview} replace />,
    errorElement: <RouteErrorPage />,
  },
  { path: ROUTES.login, element: <LoginPage /> },
  { path: '/user', element: <Navigate to={ROUTES.login} replace /> },
  { path: '/403', element: <ForbiddenPage /> },
  {
    path: '/rabbithole',
    element: (
      <RequireAuth>
        <ConsoleLayout />
      </RequireAuth>
    ),
    errorElement: <RouteErrorPage />,
    children: [
      { index: true, element: <Navigate to={ROUTES.overview} replace /> },
      { path: 'overview', element: <OverviewPage /> },
      { path: 'clients', element: <ClientsPage /> },
      { path: 'tcp', element: <TcpPage /> },
      { path: 'udp', element: <UdpPage /> },
      { path: 'domains', element: <DomainsPage /> },
      { path: 'secret', element: <SecretPage /> },
      { path: 'certs', element: <CertsPage /> },
      { path: 'settings', element: <SettingsPage /> },
      // 旧 vben 路径兼容
      { path: 'services', element: <Navigate to={ROUTES.tcp} replace /> },
      { path: 'visitors', element: <Navigate to={`${ROUTES.secret}?tab=visitors`} replace /> },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
]);
