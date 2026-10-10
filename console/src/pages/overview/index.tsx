import { listEvents, statsOverview } from '@/api/rabbithole';
import { fmtBytes, fmtTime } from '@/utils/format';
import { useQuery } from '@tanstack/react-query';
import { PageContainer } from '@ant-design/pro-components';
import { Card, Col, Row, Statistic, Table, Tag } from 'antd';
import React from 'react';

/** 总览:统计卡片 + 最近事件;5s 轮询,失败保持上次数据。 */
const OverviewPage: React.FC = () => {
  // 失败不清空:TanStack Query 出错时保留上次成功数据(data 不变)
  const statsQ = useQuery({
    queryKey: ['stats-overview'],
    queryFn: statsOverview,
    refetchInterval: 5000,
  });
  const eventsQ = useQuery({
    queryKey: ['events', 20],
    queryFn: () => listEvents({ limit: 20 }),
    refetchInterval: 5000,
  });

  const stats = statsQ.data;
  const events = eventsQ.data ?? [];

  const eventColumns = [
    { title: '时间', dataIndex: 'ts', key: 'ts', width: 170, render: (v: number) => fmtTime(v) },
    {
      title: '级别',
      dataIndex: 'level',
      key: 'level',
      width: 76,
      render: (v: string) => (
        <Tag color={v === 'warn' ? 'orange' : v === 'error' ? 'red' : 'blue'}>{v}</Tag>
      ),
    },
    { title: '来源', dataIndex: 'source', key: 'source', width: 100 },
    { title: '消息', dataIndex: 'message', key: 'message' },
  ];

  return (
    <PageContainer subTitle="rabbithole 服务端运行状态" title="总览">
      <Row gutter={[16, 16]}>
        <Col span={6}>
          <Card>
            <Statistic title="客户端" value={stats?.clients_total ?? 0} />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic
              title="在线客户端"
              value={stats?.clients_online ?? 0}
              valueStyle={{ color: stats?.clients_online ? '#3f8600' : '#cf1322' }}
            />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic title="运行中服务" value={stats?.services_running ?? 0} />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic title="当前连接" value={stats?.conns_current ?? 0} />
          </Card>
        </Col>
        <Col span={12}>
          <Card>
            <Statistic title="入站流量" value={fmtBytes(stats?.bytes_in)} />
          </Card>
        </Col>
        <Col span={12}>
          <Card>
            <Statistic title="出站流量" value={fmtBytes(stats?.bytes_out)} />
          </Card>
        </Col>
      </Row>

      <Card style={{ marginTop: 16 }} title="最近事件">
        <Table
          columns={eventColumns}
          dataSource={events}
          loading={eventsQ.isLoading}
          pagination={{ pageSize: 10, hideOnSinglePage: true }}
          rowKey="id"
          size="small"
        />
      </Card>
    </PageContainer>
  );
};

export default OverviewPage;
