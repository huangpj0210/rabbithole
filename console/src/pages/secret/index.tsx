import ProxyListPage from '@/components/ProxyListPage';
import VisitorsPanel from '@/components/VisitorsPanel';
import type { VisitorSeed } from '@/components/VisitorsPanel';
import { history, useLocation } from '@umijs/max';
import { PageContainer } from '@ant-design/pro-components';
import { Tabs } from 'antd';
import React, { useState } from 'react';

/** 密钥访问:规则(stcp/sudp/xtcp)+ 访问器双 tab,tab 状态写 query 持久化。 */
const SecretPage: React.FC = () => {
  const location = useLocation();
  const tab = new URLSearchParams(location.search).get('tab') === 'visitors' ? 'visitors' : 'rules';
  // 规则行「创建访问器」带入的目标(密钥由服务端从目标规则继承,不再手工传递)
  const [visitorSeed, setVisitorSeed] = useState<VisitorSeed | null>(null);
  return (
    <PageContainer subTitle="STCP / SUDP / XTCP 密钥访问规则与访问器" title="密钥访问">
      <Tabs
        activeKey={tab}
        items={[
          {
            key: 'rules',
            label: '规则',
            children: (
              <ProxyListPage
                bare
                defaultKind="stcp"
                emptyNoun="密钥规则"
                kinds={['stcp', 'sudp', 'xtcp'] as const}
                onCreateVisitor={(row) => {
                  setVisitorSeed({ kind: row.kind as VisitorSeed['kind'], target: row.name });
                  history.replace('/rabbithole/secret?tab=visitors');
                }}
              />
            ),
          },
          {
            key: 'visitors',
            label: '访问器',
            children: (
              <VisitorsPanel
                seed={visitorSeed}
                onSeedUsed={() => setVisitorSeed(null)}
              />
            ),
          },
        ]}
        onChange={(key) => history.replace(`/rabbithole/secret?tab=${key === 'visitors' ? 'visitors' : 'rules'}`)}
      />
    </PageContainer>
  );
};

export default SecretPage;
