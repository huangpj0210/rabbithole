import ProxyListPage from '@/components/ProxyListPage';
import React from 'react';

const DomainsPage: React.FC = () => (
  <ProxyListPage
    defaultKind='http'
    description="HTTP / HTTPS / TCPMux 域名规则"
    kinds={['http', 'https', 'tcpmux'] as const}
    title="域名服务"
  />
);

export default DomainsPage;
