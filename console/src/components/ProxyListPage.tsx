import {
  batchServices,
  createService,
  deleteService,
  disableService,
  enableService,
  listClients,
  listServices,
  rotateServiceSecret,
  updateService,
} from '@/api/rabbithole';
import type { ClientRow, ServiceRow } from '@/api/rabbithole';
import ProxyFormModal from '@/components/ProxyFormModal';
import type { ProxyKind } from '@/components/ProxyFormModal';
import { Mono, StatusBadge, proxyStatusSignal } from '@/components/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { DownloadOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import { PageContainer } from '@ant-design/pro-components';
import {
  App,
  Button,
  Card,
  Empty,
  Input,
  Modal,
  Popconfirm,
  Select,
  Switch,
  Table,
  Tag,
  Tooltip,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import React, { useMemo, useState } from 'react';

/**
 * 参数化类型页(§6.3):tcp/udp(单 kind)、domains(http/https/tcpmux)、
 * secret(stcp/sudp/xtcp)共用;列模式/空态按页参数化,路径与数据行为不变。
 */
const ProxyListPage: React.FC<{
  defaultKind: ProxyKind;
  /** bare 模式:作为 Tab 面板等容器子元素嵌入,不渲染 PageContainer 页头(避免与外层页头重复)。 */
  bare?: boolean;
  description?: string;
  /** 空态文案中的规则名词(默认取 title,再退回「转发规则」)。 */
  emptyNoun?: string;
  kinds: ProxyKind[];
  /** 密钥访问页传入:行内「创建访问器」入口,把目标规则与类型带给访问器表单。 */
  onCreateVisitor?: (row: ServiceRow) => void;
  title?: string;
}> = ({ title, description, kinds, defaultKind, bare, emptyNoun, onCreateVisitor }) => {
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const [modal, setModal] = useState<{ editing: null | ServiceRow; open: boolean }>({
    open: false,
    editing: null,
  });
  const [batchOpen, setBatchOpen] = useState(false);
  const [batchText, setBatchText] = useState('');
  const [batchResults, setBatchResults] = useState<
    null | Array<{ code: number; name: string; msg: string }>
  >(null);
  const [clientFilter, setClientFilter] = useState<null | string>(null);
  const [batchRunning, setBatchRunning] = useState(false);
  const [selectedIds, setSelectedIds] = useState<number[]>([]);
  const [batchOpRunning, setBatchOpRunning] = useState(false);

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
  const hasConns = useMemo(() => !!rows.some((r) => typeof r.conns_current === 'number'), [rows]);

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

  // 密钥轮换:服务端生成新随机值;访问器连接时继承规则当前密钥,两端一起生效
  const rotateMut = useMutation({
    mutationFn: (id: number) => rotateServiceSecret(id),
    onSuccess: () => {
      message.success('密钥已轮换,访问器自动继承新密钥,无需其他操作');
      invalidate();
    },
  });

  /** 批量启停删(后端逐项执行并回报 code/message)。 */
  async function runBatchOp(action: 'delete' | 'disable' | 'enable') {
    if (selectedIds.length === 0 || batchOpRunning) return;
    setBatchOpRunning(true);
    try {
      const results = await batchServices(selectedIds, action);
      const ok = results.filter((r) => r.code === 0).length;
      const bad = results.filter((r) => r.code !== 0);
      message.success(
        `批量${action === 'enable' ? '启用' : action === 'disable' ? '停用' : '删除'}:${ok} 成功${bad.length ? `,${bad.length} 失败` : ''}`,
      );
      bad.slice(0, 3).forEach((r) => message.error(`#${r.id}:${r.message}`));
      setSelectedIds([]);
      invalidate();
    } finally {
      setBatchOpRunning(false);
    }
  }

  async function runBatch() {
    if (batchRunning) return;
    let arr: Array<Record<string, any>>;
    try {
      arr = JSON.parse(batchText);
      if (!Array.isArray(arr)) throw new Error('not array');
    } catch {
      message.error('不是合法的 JSON 数组');
      return;
    }
    setBatchRunning(true);
    const results: Array<{ code: number; name: string; msg: string }> = [];
    for (const item of arr) {
      try {
        await createService(item);
        results.push({ code: 0, name: item.name ?? '?', msg: '成功' });
      } catch (e) {
        results.push({
          code: -1,
          name: item.name ?? '?',
          msg: e instanceof Error ? e.message : '失败',
        });
      }
    }
    setBatchResults(results);
    setBatchRunning(false);
    invalidate();
  }

  const isTunnel = kinds.every((k) => k === 'tcp' || k === 'udp');
  const isDomain = kinds.every((k) => k === 'http' || k === 'https' || k === 'tcpmux');
  const isSecret = kinds.every((k) => k === 'stcp' || k === 'sudp' || k === 'xtcp');

  // 空态三件套(§6.3):发生了什么 / 该做什么 / 主按钮
  const noun = emptyNoun ?? title ?? '转发规则';
  const emptyText = (
    <Empty
      description={
        <span>
          {clients.length === 0
            ? `还没有客户端。${noun}需要一个在线客户端承载,先到「客户端」页创建。`
            : `还没有${noun}。`}
        </span>
      }
      image={Empty.PRESENTED_IMAGE_SIMPLE}
      style={{ padding: '32px 0' }}
    >
      {clients.length === 0 ? (
        <Button href="#/rabbithole/clients" type="primary">
          去创建客户端
        </Button>
      ) : (
        <Button onClick={() => setModal({ open: true, editing: null })} type="primary">
          新建规则
        </Button>
      )}
    </Empty>
  );

  const columns: ColumnsType<ServiceRow> = [
    {
      title: '名称',
      dataIndex: 'name',
      key: 'name',
      width: 150,
      render: (v: string) => <span style={{ fontWeight: 500 }}>{v}</span>,
    },
    ...(isTunnel
      ? [
          {
            title: '监听',
            key: 'bind',
            width: 170,
            render: (_: unknown, row: ServiceRow) =>
              row.bind_port > 0 ? (
                <Mono>{`${row.bind_ip}:${row.bind_port}`}</Mono>
              ) : (
                <Mono>共享端口</Mono>
              ),
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
              if (ds[0]) parts.push(<Mono key="d0">{ds[0]}</Mono>);
              if (ds.length > 1) parts.push(<Tag key="dn">+{ds.length - 1}</Tag>);
              if (sub) parts.push(<Mono key="sub">{`${sub}.*`}</Mono>);
              return parts.length ? <>{parts}</> : '-';
            },
          },
        ]
      : []),
    ...(isSecret
      ? [
          {
            title: '允许访问方',
            key: 'allow_users',
            width: 120,
            render: (_: unknown, row: ServiceRow) =>
              row.spec?.[row.kind]?.allow_users?.length
                ? (row.spec[row.kind].allow_users as string[]).join(',')
                : '任意',
          },
        ]
      : []),
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 150,
      render: (st: string, row) => {
        const sig = proxyStatusSignal(st, row.healthy);
        return row.status_reason ? (
          <Tooltip title={row.status_reason}>
            <StatusBadge signal={sig.signal} text={sig.text} />
          </Tooltip>
        ) : (
          <StatusBadge signal={sig.signal} text={sig.text} />
        );
      },
    },
    {
      title: '目标',
      key: 'target',
      render: (_, row) =>
        row.plugin ? <Mono>{row.plugin}</Mono> : <Mono>{`${row.target_host}:${row.target_port}`}</Mono>,
    },
    {
      title: '池',
      dataIndex: 'pool_ready',
      key: 'pool',
      width: 60,
      align: 'right' as const,
      render: (v: number) => <Mono>{v}</Mono>,
    },
    ...(hasConns
      ? [
          {
            title: '连接',
            dataIndex: 'conns_current' as const,
            key: 'conns',
            width: 70,
            align: 'right' as const,
            render: (v: number) => <Mono>{v ?? 0}</Mono>,
          },
        ]
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
      width: 250,
      render: (_, row: ServiceRow) => (
        <>
          <Button onClick={() => setModal({ open: true, editing: row })} size="small" type="link">
            编辑
          </Button>
          {isSecret && onCreateVisitor && (
            <Button onClick={() => onCreateVisitor(row)} size="small" type="link">
              创建访问器
            </Button>
          )}
          {isSecret && (
            <Popconfirm
              description="服务端生成新随机密钥,访问器自动继承,现有中继不受影响。"
              onConfirm={() => rotateMut.mutate(row.id)}
              title={`轮换 ${row.name} 的密钥?`}
            >
              <Button
                loading={rotateMut.isPending && rotateMut.variables === row.id}
                size="small"
                type="link"
              >
                轮换密钥
              </Button>
            </Popconfirm>
          )}
          <Popconfirm
            cancelText="取消"
            okButtonProps={{ danger: true }}
            okText="删除"
            onConfirm={() => deleteMut.mutate(row.id)}
            title={`删除规则 ${row.name}?此操作不可恢复。`}
          >
            <Button danger size="small" type="link">
              删除
            </Button>
          </Popconfirm>
        </>
      ),
    },
  ];

  const body = (
    <>
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
        style={{ borderRadius: 8 }}
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
        <Button
          icon={<ReloadOutlined />}
          onClick={() => invalidate()}
          size="small"
          style={{ marginLeft: 8 }}
        >
          刷新
        </Button>
        {selectedIds.length > 0 && (
          <>
            <Button
              ghost
              loading={batchOpRunning}
              onClick={() => runBatchOp('enable')}
              size="small"
              style={{ marginLeft: 8 }}
              type="primary"
            >
              批量启用({selectedIds.length})
            </Button>
            <Button
              loading={batchOpRunning}
              onClick={() => runBatchOp('disable')}
              size="small"
              style={{ marginLeft: 8 }}
            >
              批量停用
            </Button>
            <Popconfirm
              onConfirm={() => runBatchOp('delete')}
              title={`删除选中的 ${selectedIds.length} 条规则?此操作不可恢复。`}
            >
              <Button danger loading={batchOpRunning} size="small" style={{ marginLeft: 8 }}>
                批量删除
              </Button>
            </Popconfirm>
          </>
        )}
        <Table
          columns={columns}
          dataSource={filtered}
          loading={servicesQ.isLoading}
          locale={{ emptyText }}
          pagination={{ pageSize: 50, hideOnSinglePage: true }}
          rowKey="id"
          rowSelection={{
            onChange: (ids) => setSelectedIds(ids as number[]),
            selectedRowKeys: selectedIds,
          }}
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
        okButtonProps={{ loading: batchRunning }}
        okText="开始导入"
        onCancel={() => setBatchOpen(false)}
        onOk={runBatch}
        open={batchOpen}
        title="批量导入规则"
        width={680}
      >
        <p style={{ color: 'rgba(0,0,0,0.45)', fontSize: 12 }}>
          粘贴规则 JSON 数组(与单条创建载荷一致),逐条创建并回报每条结果。
        </p>
        <Input.TextArea
          onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) => setBatchText(e.target.value)}
          placeholder='[{"name":"svc1","kind":"tcp","client_id":"...","bind_port":0,"target_host":"127.0.0.1","target_port":80}]'
          rows={10}
          value={batchText}
        />
        {batchResults && (
          <div className="rh-mono" style={{ fontSize: 12, marginTop: 12, maxHeight: 200, overflow: 'auto' }}>
            {batchResults.map((r, i) => (
              <div key={i} style={{ color: r.code === 0 ? '#3f8600' : '#cf1322' }}>
                {r.code === 0 ? '✓' : '✗'} {r.name}:{r.msg}
              </div>
            ))}
          </div>
        )}
      </Modal>
    </>
  );
  if (bare) return body;
  return (
    <PageContainer subTitle={description} title={title}>
      {body}
    </PageContainer>
  );
};

export default ProxyListPage;
