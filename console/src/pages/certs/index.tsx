import { PageContainer, ProCard } from '@ant-design/pro-components';
import React from 'react';

/** 证书(R0 占位,R1/R2/R3 填充真实实现)。 */
const CertsPage: React.FC = () => {
  return (
    <PageContainer>
      <ProCard title="证书">HTTPS 证书管理(SNI 自动匹配) —— 本页建设中。</ProCard>
    </PageContainer>
  );
};

export default CertsPage;
