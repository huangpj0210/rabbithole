import ProxyListPage from '@/components/ProxyListPage';
import React from 'react';

const TcpPage: React.FC = () => (
  <ProxyListPage
    defaultKind='tcp'
    description="TCP 端口转发规则"
    kinds={['tcp'] as const}
    title="TCP 隧道"
  />
);

export default TcpPage;
