import { listEvents, listServices, statsOverview } from '@/api/rabbithole';
import { fmtBytes, fmtTime } from '@/utils/format';
import { InfoLabel } from '@/components/ui';
import { Mono, StatusBadge, proxyStatusSignal } from '@/components/ui';
import { useQuery } from '@tanstack/react-query';
import { PageContainer } from '@ant-design/pro-components';
import { Button, Card, Select, Table, Tooltip } from 'antd';
import React, { useMemo, useRef, useState } from 'react';
import {
  Area,
  AreaChart,
  CartesianGrid,
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

/** 总览(§6.2):指标条 / 流量速率图 + 隧道健康 / 最近事件;5s 轮询,失败保持上次数据。 */
const OverviewPage: React.FC = () => {
  // 失败不清空:TanStack Query 出错时保留上次成功数据(data 不变)
  const statsQ = useQuery({
    queryKey: ['stats-overview'],
    queryFn: statsOverview,
    refetchInterval: 5000,
  });
  const [level, setLevel] = useState('');
  const eventsQ = useQuery({
    queryKey: ['events', 20, level],
    queryFn: () => listEvents(level ? { limit: 20, level } : { limit: 20 }),
    refetchInterval: 5000,
  });
  const servicesQ = useQuery({ queryKey: ['services'], queryFn: () => listServices() });

  const stats = statsQ.data;
  const events = eventsQ.data ?? [];
  const [eventsExpanded, setEventsExpanded] = useState(false);
  const shownEvents = eventsExpanded ? events : events.slice(0, 5);

  // 差分速率窗口:仅成功样本入列,失败期断线,不外推不补零
  const winRef = useRef<{
    lastIn: null | number;
    lastOut: null | number;
    lastTs: null | number;
    points: RatePoint[];
  }>({ lastIn: null, lastOut: null, lastTs: null, points: [] });
  let curIn = 0;
  let curOut = 0;
  if (stats && statsQ.dataUpdatedAt !== winRef.current.lastTs) {
    const w = winRef.current;
    if (w.lastIn !== null && w.lastOut !== null && w.lastTs !== null) {
      const dt = (statsQ.dataUpdatedAt - w.lastTs) / 1000;
      if (dt > 0) {
        curIn = Math.max(0, (stats.bytes_in - w.lastIn) / dt);
        curOut = Math.max(0, (stats.bytes_out - w.lastOut) / dt);
        w.points = [
          ...w.points,
          {
            t: new Date(statsQ.dataUpdatedAt).toLocaleTimeString('zh-CN', {
              hour12: false,
              minute: '2-digit',
              second: '2-digit',
            }),
            in: curIn,
            out: curOut,
          },
        ].slice(-WINDOW_POINTS);
      }
    }
    w.lastIn = stats.bytes_in;
    w.lastOut = stats.bytes_out;
    w.lastTs = statsQ.dataUpdatedAt;
  } else if (winRef.current.points.length > 0) {
    curIn = winRef.current.points[winRef.current.points.length - 1].in;
    curOut = winRef.current.points[winRef.current.points.length - 1].out;
  }
  const points = winRef.current.points;

  // 隧道健康面板:启用中的转发规则(运行态存在)
  const tunnels = useMemo(
    () => (servicesQ.data ?? []).filter((r) => r.status !== 'disabled'),
    [servicesQ.data],
  );

  const metricCell = (label: string, value: React.ReactNode, extra?: React.ReactNode, info?: string) => (
    <div
      key={label}
      style={{
        flex: '1 1 0',
        minWidth: 128,
        padding: '16px 20px',
        borderRight: '1px solid rgba(5,5,5,0.06)',
      }}
    >
      <div style={{ color: 'rgba(0,0,0,0.45)', fontSize: 12, marginBottom: 8 }}>{info ? <InfoLabel info={info} title={label} /> : label}</div>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
        <span className="rh-mono" style={{ fontSize: 28, fontWeight: 600, lineHeight: 1 }}>
          {value}
        </span>
        {extra}
      </div>
    </div>
  );

  const eventColumns = [
    {
      title: '级别',
      dataIndex: 'level',
      key: 'level',
      width: 84,
      render: (v: string) => (
        <StatusBadge signal={v === 'error' ? 'error' : v === 'warn' ? 'warn' : 'idle'} text={v} />
      ),
    },
    {
      title: '时间',
      dataIndex: 'ts',
      key: 'ts',
      width: 170,
      render: (v: number) => <Mono>{fmtTime(v)}</Mono>,
    },
    {
      title: '来源',
      dataIndex: 'source',
      key: 'source',
      width: 90,
      render: (v: string) => <Mono>{v}</Mono>,
    },
    { title: '消息', dataIndex: 'message', key: 'message' },
  ];

  return (
    <PageContainer subTitle="服务端运行状态与转发面健康" title="总览">
      {/* 指标条:单卡分格,mono + tabular-nums,刷新不抖动(§6.2) */}
      <Card styles={{ body: { padding: 0 } }} style={{ borderRadius: 8 }}>
        <div style={{ display: 'flex', flexWrap: 'wrap' }}>
          {metricCell(
            '在线客户端',
            stats ? `${stats.clients_online}` : '-',
            stats ? <Mono style={{ color: 'rgba(0,0,0,0.45)' }}>/ {stats.clients_total}</Mono> : undefined,
            '已连接服务端的客户端数 / 注册总数',
          )}
          {metricCell(
            '运行服务',
            stats?.services_running ?? 0,
            stats ? <Mono style={{ color: 'rgba(0,0,0,0.45)' }}>/ {stats.services_total}</Mono> : undefined,
            '正在监听公网入口的规则数 / 启用总数',
          )}
          {metricCell('当前连接', stats?.conns_current ?? 0, undefined, '全部规则当前承载的并发连接')}
          {metricCell('入站速率', `${fmtBytes(curIn)}/s`, undefined, '公网侧流入服务端的流量速率(按最近采样)')}
          {metricCell('出站速率', `${fmtBytes(curOut)}/s`, undefined, '服务端流向公网侧的流量速率(按最近采样)')}
          {metricCell('累计流量', stats ? `${fmtBytes(stats.bytes_in + stats.bytes_out)}` : '-', undefined, '服务端启动以来的入+出累计(进程内计量,重启归零)')}
        </div>
      </Card>

      <div style={{ display: 'flex', gap: 16, marginTop: 16, flexWrap: 'wrap' }}>
        {/* 流量速率图:渐变填充 + 当前值标注(§6.2) */}
        <Card
          extra={
            <span style={{ fontSize: 12, color: 'rgba(0,0,0,0.45)' }}>
              入 <Mono style={{ color: '#1890ff' }}>{fmtBytes(curIn)}/s</Mono>
              <span style={{ margin: '0 8px' }}>·</span>
              出 <Mono style={{ color: '#52c41a' }}>{fmtBytes(curOut)}/s</Mono>
            </span>
          }
          style={{ borderRadius: 8, flex: '1 1 480px' }}
          title="流量速率(近 5 分钟)"
        >
          <ResponsiveContainer height={220} width="100%">
            <AreaChart data={points}>
              <defs>
                <linearGradient id="rh-in" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#1890ff" stopOpacity={0.28} />
                  <stop offset="100%" stopColor="#1890ff" stopOpacity={0.02} />
                </linearGradient>
                <linearGradient id="rh-out" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#52c41a" stopOpacity={0.24} />
                  <stop offset="100%" stopColor="#52c41a" stopOpacity={0.02} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
              <XAxis dataKey="t" fontSize={11} />
              <YAxis fontSize={11} tickFormatter={(v: number) => fmtBytes(v)} width={70} />
              <RTooltip formatter={(v) => `${fmtBytes(Number(v ?? 0))}/s`} labelFormatter={(l) => String(l)} />
              <Area
                connectNulls={false}
                dataKey="in"
                fill="url(#rh-in)"
                name="入站"
                stroke="#1890ff"
                strokeWidth={2}
                type="monotone"
              />
              <Area
                connectNulls={false}
                dataKey="out"
                fill="url(#rh-out)"
                name="出站"
                stroke="#52c41a"
                strokeWidth={2}
                type="monotone"
              />
            </AreaChart>
          </ResponsiveContainer>
        </Card>

        {/* 隧道健康:每条启用规则一行(§6.2) */}
        <Card style={{ borderRadius: 8, flex: '1 1 360px' }} title={`隧道健康(${tunnels.length})`}>
          {tunnels.length === 0 ? (
            <div style={{ color: 'rgba(0,0,0,0.45)', fontSize: 13, padding: '24px 0' }}>
              还没有启用中的转发规则。先到「客户端」页接入客户端,再创建规则。
            </div>
          ) : (
            <div style={{ maxHeight: 264, overflow: 'auto' }}>
              {tunnels.map((r) => {
                const sig = proxyStatusSignal(r.status, r.healthy);
                const sec = r.spec?.[r.kind] as Record<string, any> | undefined;
                const listen =
                  r.kind === 'tcp' || r.kind === 'udp'
                    ? `${r.bind_ip}:${r.bind_port}`
                    : (sec?.custom_domains?.[0] ?? sec?.subdomain ?? '—');
                return (
                  <div
                    key={r.id}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 12,
                      padding: '8px 0',
                      borderBottom: '1px solid rgba(5,5,5,0.04)',
                    }}
                  >
                    <span
                      style={{
                        fontWeight: 500,
                        flex: '0 0 auto',
                        maxWidth: 140,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {r.name}
                    </span>
                    <Mono
                      style={{
                        flex: 1,
                        color: 'rgba(0,0,0,0.55)',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {String(listen)}
                    </Mono>
                    {r.status_reason ? (
                      <Tooltip title={r.status_reason}>
                        <StatusBadge signal={sig.signal} text={sig.text} />
                      </Tooltip>
                    ) : (
                      <StatusBadge signal={sig.signal} text={sig.text} />
                    )}
                    <Mono style={{ color: 'rgba(0,0,0,0.4)', flex: '0 0 auto' }}>池{r.pool_ready}</Mono>
                  </div>
                );
              })}
            </div>
          )}
        </Card>
      </div>

      {/* 最近事件:默认 5 条,可展开(§6.2);级别筛选 */}
      <Card
        extra={
          <>
            <Select
              allowClear
              onChange={(v) => setLevel(v ?? '')}
              options={[
                { label: '全部', value: '' },
                { label: 'info', value: 'info' },
                { label: 'warn', value: 'warn' },
                { label: 'error', value: 'error' },
              ]}
              placeholder="级别"
              size="small"
              style={{ width: 100, marginRight: 8 }}
              value={level || undefined}
            />
            {events.length > 5 && (
              <Button onClick={() => setEventsExpanded((v) => !v)} size="small" type="link">
                {eventsExpanded ? '收起' : `展开全部(${events.length})`}
              </Button>
            )}
          </>
        }
        style={{ borderRadius: 8, marginTop: 16 }}
        title="最近事件"
      >
        <Table
          columns={eventColumns}
          dataSource={shownEvents}
          loading={eventsQ.isLoading}
          pagination={false}
          rowKey="id"
          size="small"
        />
      </Card>
    </PageContainer>
  );
};

export default OverviewPage;
