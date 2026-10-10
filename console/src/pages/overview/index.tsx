import { listEvents, statsOverview } from '@/api/rabbithole';
import { fmtBytes, fmtTime } from '@/utils/format';
import { useQuery } from '@tanstack/react-query';
import { PageContainer } from '@ant-design/pro-components';
import { Card, Col, Row, Statistic, Table, Tag } from 'antd';
import React, { useRef } from 'react';
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from 'recharts';

const WINDOW_POINTS = 60; // 5s × 60 = 5 分钟滚动窗口

interface RatePoint {
  in: number;
  out: number;
  t: string;
}

/** 总览:统计卡片 + 流量速率图 + 最近事件;5s 轮询,失败保持上次数据。 */
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

  // 差分速率窗口:仅成功样本入列,失败期断线(缺点),不外推不补零
  const winRef = useRef<{ lastIn: null | number; lastOut: null | number; lastTs: null | number; points: RatePoint[] }>({
    lastIn: null,
    lastOut: null,
    lastTs: null,
    points: [],
  });
  if (stats && statsQ.dataUpdatedAt !== winRef.current.lastTs) {
    const w = winRef.current;
    if (w.lastIn !== null && w.lastOut !== null && w.lastTs !== null) {
      const dt = (statsQ.dataUpdatedAt - w.lastTs) / 1000;
      if (dt > 0) {
        const p: RatePoint = {
          t: new Date(statsQ.dataUpdatedAt).toLocaleTimeString('zh-CN', { hour12: false, minute: '2-digit', second: '2-digit' }),
          in: Math.max(0, (stats.bytes_in - w.lastIn) / dt),
          out: Math.max(0, (stats.bytes_out - w.lastOut) / dt),
        };
        w.points = [...w.points, p].slice(-WINDOW_POINTS);
      }
    }
    w.lastIn = stats.bytes_in;
    w.lastOut = stats.bytes_out;
    w.lastTs = statsQ.dataUpdatedAt;
  }
  const points = winRef.current.points;

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

      <Card
        style={{ marginTop: 16 }}
        title="流量速率(近 5 分钟,每 5s 采样)"
      >
        {/* 后端 bytes 恒 0(F1 未落地)时呈平线;失败期断线不外推 */}
        <ResponsiveContainer height={220} width="100%">
          <LineChart data={points}>
            <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
            <XAxis dataKey="t" fontSize={11} />
            <YAxis fontSize={11} tickFormatter={(v: number) => fmtBytes(v) + '/s' } width={86} />
            <RTooltip
              formatter={(v) => fmtBytes(Number(v ?? 0)) + '/s'}
              labelFormatter={(l) => String(l)}
            />
            <Line
              connectNulls={false}
              dataKey="in"
              dot={false}
              name="入站"
              stroke="#1890ff"
              strokeWidth={2}
              type="monotone"
            />
            <Line
              connectNulls={false}
              dataKey="out"
              dot={false}
              name="出站"
              stroke="#52c41a"
              strokeWidth={2}
              type="monotone"
            />
          </LineChart>
        </ResponsiveContainer>
      </Card>

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
