import {
  createService,
  deleteService,
  disableService,
  enableService,
  listClients,
  listServices,
  updateService,
} from '@/api/rabbithole';
import type { ClientRow, ServiceRow } from '@/api/rabbithole';
import ProxyFormModal from '@/components/ProxyFormModal';
import type { ProxyKind } from '@/components/ProxyFormModal';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { DownloadOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import { PageContainer } from '@ant-design/pro-components';
import { App, Button, Card, Input, Modal, Popconfirm, Select, Switch, Table, Tag, Tooltip } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import React, { useMemo, useState } from 'react';

const STATUS_COLOR: Record<string, string> = {
  running: 'green',
  pending: 'blue',
  offline: 'default',
  disabled: 'default',
  bind_error: 'red',
  apply_error: 'red',
  unhealthy: 'orange',
};

/**
 * 参数化类型页:tcp/udp(单 kind,无选择器)、domains(http/https/tcpmux)、
 * secret(stcp/sudp/xtcp)共用;列表按 kinds 过滤,弹窗按 §8.1 矩阵渲染。
 */
const ProxyListPage: React.FC<{
  defaultKind: ProxyKind;
  description: string;
  kinds: ProxyKind[];
  title: string;
}> = ({ title, description, kinds, defaultKind }) => {
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const [modal, setModal] = useState<{ editing: null | ServiceRow; open: boolean }>({
    open: false,
    editing: null,
  });
  const [batchOpen, setBatchOpen] = useState(false);
  const [batchText, setBatchText] = useState('');
  const [batchResults, setBatchResults] = useState<null | Array<{ code: number; name: string; msg: string }>>(null);
  const [clientFilter, setClientFilter] = useState<null | string>(null);

  const servicesQ = useQuery({ queryKey: ['services'], queryFn: () => listServices() });
  const clientsQ = useQuery({ queryKey: ['clients'], queryFn: listClients });
  const clients: ClientRow[] = clientsQ.data ?? [];

  const rows = useMemo(
    () => (servicesQ.data ?? []).filter((r) => kinds.includes(r.kind as ProxyKind)),
    [servicesQ.data, kinds],
  );
  const filtered = useMemo(
    () => (clientFilter ? rows.filter((r) => r.client_id === clientFilter) : rows),
    [rows, clientFilter],
  );
  // F1 字段(后端 M1 落地前不返回,列不渲染)
  const hasConns = useMemo(
    () => !!rows.some((r) => typeof r.conns_current === 'number'),
    [rows],
  );

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['services'] });

  const saveMut = useMutation({
    mutationFn: ({ editing, spec }: { editing: null | ServiceRow; spec: Record<string, any> }) =>
      editing ? updateService(editing.id, spec) : createService(spec),
    onSuccess: (_d, v) => {
      message.success(v.editing ? '已保存' : '已创建');
      setModal({ open: false, editing: null });
      invalidate();
    },
  });

  const toggleMut = useMutation({
    mutationFn: ({ enabled, row }: { enabled: boolean; row: ServiceRow }) =>
      enabled ? enableService(row.id) : disableService(row.id),
    onSuccess: invalidate,
  });

  const deleteMut = useMutation({
    mutationFn: (id: number) => deleteService(id),
    onSuccess: () => {
      message.success('已删除');
      invalidate();
    },
  });

  async function runBatch() {
    let arr: Array<Record<string, any>>;
    try {
      arr = JSON.parse(batchText);
      if (!Array.isArray(arr)) throw new Error('not array');
    } catch {
      message.error('不是合法的 JSON 数组');
      return;
    }
    const results: Array<{ code: number; name: string; msg: string }> = [];
    for (const item of arr) {
      try {
        await createService(item);
        results.push({ code: 0, name: item.name ?? '?', msg: '成功' });
      } catch (e) {
        results.push({ code: -1, name: item.name ?? '?', msg: e instanceof Error ? e.message : '失败' });
      }
    }
    setBatchResults(results);
    invalidate();
  }

  const isTunnel = kinds.every((k) => k === 'tcp' || k === 'udp');
  const isDomain = kinds.every((k) => k === 'http' || k === 'https' || k === 'tcpmux');
  const isSecret = kinds.every((k) => k === 'stcp' || k === 'sudp' || k === 'xtcp');

  const columns: ColumnsType<ServiceRow> = [
    { title: '名称', dataIndex: 'name', key: 'name', width: 140 },
    ...(isTunnel
      ? [
          {
            title: '监听',
            key: 'bind',
            width: 150,
            render: (_: unknown, row: ServiceRow) =>
              row.bind_port > 0 ? `${row.bind_ip}:${row.bind_port}` : '共享端口',
          },
        ]
      : []),
    ...(isDomain
      ? [
          {
            title: '域名',
            key: 'domains',
            render: (_: unknown, row: ServiceRow) => {
              const s = row.spec ?? {};
              const ds: string[] =
                s.http?.custom_domains ?? s.https?.custom_domains ?? s.tcpmux?.custom_domains ?? [];
              const sub = s.http?.subdomain ?? s.https?.subdomain;
              const parts: React.ReactNode[] = [];
              if (ds[0]) parts.push(<Tag key="d0" color="blue">{ds[0]}</Tag>);
              if (ds.length > 1) parts.push(<Tag key="dn">+{ds.length - 1}</Tag>);
              if (sub) parts.push(<Tag key="sub">{`${sub}.*`}</Tag>);
              return parts.length ? <>{parts}</> : '-';
            },
          },
        ]
      : []),
    ...(isSecret
      ? [
          {
            title: '密钥',
            key: 'secret',
            width: 120,
            render: (_: unknown, row: ServiceRow) => {
              const k = row.spec?.[row.kind]?.secret_key;
              return k ? <Tag color="purple">{`${String(k).slice(0, 6)}…`}</Tag> : '-';
            },
          },
          {
            title: '目标代理',
            dataIndex: 'name',
            key: 'target_proxy',
            width: 110,
            render: (_: unknown, row: ServiceRow) => row.spec?.[row.kind]?.allow_users?.length
              ? (row.spec[row.kind].allow_users as string[]).join(',')
              : '任意',
          },
        ]
      : []),
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 130,
      render: (st: string, row) => (
        <>
          <Tag color={STATUS_COLOR[st] ?? 'default'}>{st}</Tag>
          {row.status_reason ? (
            <Tooltip title={row.status_reason}>
              <span style={{ color: '#999', fontSize: 12 }}>{row.status_reason.slice(0, 16)}…</span>
            </Tooltip>
          ) : null}
        </>
      ),
    },
    {
      title: '目标',
      key: 'target',
      render: (_, row) =>
        row.plugin ? (
          <Tag color="purple">{row.plugin}</Tag>
        ) : (
          `${row.target_host}:${row.target_port}`
        ),
    },
    { title: '池', dataIndex: 'pool_ready', key: 'pool', width: 60 },
    ...(hasConns
      ? [{ title: '连接', dataIndex: 'conns_current' as const, key: 'conns', width: 70 }]
      : []),
    {
      title: '启用',
      dataIndex: 'enabled',
      key: 'enabled',
      width: 76,
      render: (v: boolean, row: ServiceRow) => {
        const pending = toggleMut.isPending && toggleMut.variables?.row.id === row.id;
        return (
          <Switch
            checked={pending ? (toggleMut.variables?.enabled ?? v) : v}
            loading={pending}
            onChange={(checked) => toggleMut.mutate({ row, enabled: checked })}
            size="small"
          />
        );
      },
    },
    {
      title: '操作',
      key: 'action',
      width: 140,
      render: (_, row: ServiceRow) => (
        <>
          <Button onClick={() => setModal({ open: true, editing: row })} size="small" type="link">
            编辑
          </Button>
          <Popconfirm onConfirm={() => deleteMut.mutate(row.id)} title="确认删除该规则?">
            <Button danger size="small" type="link">
              删除
            </Button>
          </Popconfirm>
        </>
      ),
    },
  ];

  return (
    <PageContainer subTitle={description} title={title}>
      <Card
        extra={
          <Select
            allowClear
            onChange={(v) => setClientFilter(v ?? null)}
            options={clients.map((c) => ({ label: c.name, value: c.id }))}
            placeholder="按客户端筛选"
            style={{ width: 160, marginRight: 8 }}
            value={clientFilter}
          />
        }
      >
        <Tooltip title={clients.length === 0 ? '请先在「客户端」页创建客户端' : ''}>
          <Button
            disabled={clients.length === 0}
            icon={<PlusOutlined />}
            onClick={() => setModal({ open: true, editing: null })}
            type="primary"
          >
            新建规则
          </Button>
        </Tooltip>
        <Button
          icon={<DownloadOutlined />}
          onClick={() => {
            setBatchText('');
            setBatchResults(null);
            setBatchOpen(true);
          }}
          style={{ marginLeft: 8 }}
        >
          批量导入
        </Button>
        <Button icon={<ReloadOutlined />} onClick={() => invalidate()} size="small" style={{ marginLeft: 8 }}>
          刷新
        </Button>
        <Table
          columns={columns}
          dataSource={filtered}
          loading={servicesQ.isLoading}
          pagination={false}
          rowKey="id"
          size="middle"
          style={{ marginTop: 12 }}
        />
      </Card>

      <ProxyFormModal
        clients={clients}
        defaultKind={defaultKind}
        editing={modal.editing}
        kinds={kinds}
        onCancel={() => setModal({ open: false, editing: null })}
        onFinish={async (spec) => {
          await saveMut.mutateAsync({ editing: modal.editing, spec });
        }}
        open={modal.open}
        submitting={saveMut.isPending}
      />

      <Modal
        cancelText="关闭"
        okButtonProps={{ loading: false }}
        okText="开始导入"
        onCancel={() => setBatchOpen(false)}
        onOk={runBatch}
        open={batchOpen}
        title="批量导入规则(JSON 数组)"
        width={680}
      >
        <p style={{ color: '#999', fontSize: 12 }}>
          粘贴 ProxySpec JSON 数组(与 POST /api/services 载荷一致),逐条创建并回报结果。
        </p>
        <Input.TextArea
          onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) => setBatchText(e.target.value)}
          placeholder='[{"name":"svc1","kind":"tcp","client_id":"...","bind_port":0,"target_host":"127.0.0.1","target_port":80}]'
          rows={10}
          value={batchText}
        />
        {batchResults && (
          <div style={{ marginTop: 12, maxHeight: 200, overflow: 'auto' }}>
            {batchResults.map((r, i) => (
              <div key={i} style={{ color: r.code === 0 ? '#3f8600' : '#cf1322' }}>
                {r.code === 0 ? '✓' : '✗'} {r.name}:{r.msg}
              </div>
            ))}
          </div>
        )}
      </Modal>
    </PageContainer>
  );
};

export default ProxyListPage;
