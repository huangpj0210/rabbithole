import ProxyListPage from '@/components/ProxyListPage';
import VisitorsPanel from '@/components/VisitorsPanel';
import { history, useLocation } from '@umijs/max';
import { PageContainer } from '@ant-design/pro-components';
import { Tabs } from 'antd';
import React from 'react';

/** 密钥访问:规则(stcp/sudp/xtcp)+ 访问器双 tab,tab 状态写 query 持久化。 */
const SecretPage: React.FC = () => {
  const location = useLocation();
  const tab = new URLSearchParams(location.search).get('tab') === 'visitors' ? 'visitors' : 'rules';
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
                defaultKind="stcp"
                description=""
                kinds={['stcp', 'sudp', 'xtcp'] as const}
                title=""
              />
            ),
          },
          { key: 'visitors', label: '访问器', children: <VisitorsPanel /> },
        ]}
        onChange={(key) => history.replace(`/rabbithole/secret?tab=${key === 'visitors' ? 'visitors' : 'rules'}`)}
      />
    </PageContainer>
  );
};

export default SecretPage;
