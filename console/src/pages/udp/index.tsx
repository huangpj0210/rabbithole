import ProxyListPage from '@/components/ProxyListPage';
import React from 'react';

const UdpPage: React.FC = () => (
  <ProxyListPage
    defaultKind='udp'
    description="UDP 端口转发规则"
    kinds={['udp'] as const}
    title="UDP 隧道"
  />
);

export default UdpPage;
