import {
  createClient,
  deleteClient,
  listClients,
  rotateClientSecret,
  updateClient,
} from '@/api/rabbithole';
import type { ClientRow } from '@/api/rabbithole';
import SecretModal from '@/components/SecretModal';
import { Mono, MonoCopy, StatusBadge } from '@/components/ui';
import { fmtBytes } from '@/utils/format';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  EditOutlined,
  KeyOutlined,
  PlusOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { PageContainer } from '@ant-design/pro-components';
import {
  App,
  Button,
  Card,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Popover,
  Switch,
  Table,
  Tag,
  Tooltip,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import React, { useMemo, useState } from 'react';

interface ClientForm {
  name: string;
  remark: string;
  // F2 配额字段:后端 M1 落地前忽略,属预期(容错降级见 design-parity §3)
  rate_limit: string;
  max_conns: number | null;
  max_services: number | null;
  flow_limit_mb: number | null;
}

/** 相对时间:"2 分钟前";超过 1h 给绝对时间。 */
function relTime(ts: null | number): string {
  if (!ts) return '-';
  const diff = Date.now() - ts;
  if (diff < 60_000) return '刚刚';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return `${Math.floor(diff / 86_400_000)} 天前`;
}

/** 客户端管理:列表/新建(一次性密钥)/编辑/轮换/启停/删除。 */
const ClientsPage: React.FC = () => {
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const [modalOpen, setModalOpen] = useState(false);
  const [editId, setEditId] = useState<null | string>(null);
  const [secretModal, setSecretModal] = useState<{ clientId?: string; open: boolean; secret: string }>({
    open: false,
    secret: '',
  });
  const [form] = Form.useForm<ClientForm>();

  const { data: rows, isLoading } = useQuery({
    queryKey: ['clients'],
    queryFn: listClients,
  });

  // F1 计量字段:后端 M1 落地前不返回,相关列整体不渲染
  const hasMeter = useMemo(
    () => !!rows?.some((r) => typeof r.bytes_in === 'number' || typeof r.bytes_out === 'number'),
    [rows],
  );
  const hasConns = useMemo(() => !!rows?.some((r) => typeof r.conns_current === 'number'), [rows]);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['clients'] });

  const createMut = useMutation({
    mutationFn: (data: ClientForm) =>
      createClient({
        name: data.name,
        remark: data.remark,
        rate_limit: data.rate_limit?.trim() || undefined,
        max_conns: data.max_conns || undefined,
        max_services: data.max_services || undefined,
        flow_limit: data.flow_limit_mb ? Math.round(data.flow_limit_mb * 1024 * 1024) : undefined,
      }),
    onSuccess: (r) => {
      message.success('客户端已创建');
      setModalOpen(false);
      setSecretModal({ clientId: r?.id, open: true, secret: r?.secret ?? '' });
      invalidate();
    },
  });

  const updateMut = useMutation({
    mutationFn: ({ id, data }: { data: Partial<ClientForm> & { enabled?: boolean }; id: string }) =>
      updateClient(id, {
        name: data.name,
        remark: data.remark,
        enabled: data.enabled,
        rate_limit: data.rate_limit,
        max_conns: data.max_conns || undefined,
        max_services: data.max_services || undefined,
        flow_limit: data.flow_limit_mb ? Math.round(data.flow_limit_mb * 1024 * 1024) : undefined,
      }),
    onSuccess: () => {
      message.success('已保存');
      setModalOpen(false);
      invalidate();
    },
  });

  const toggleMut = useMutation({
    mutationFn: ({ enabled, row }: { enabled: boolean; row: ClientRow }) =>
      updateClient(row.id, { enabled }),
    onSuccess: invalidate,
  });

  const rotateMut = useMutation({
    mutationFn: (id: string) => rotateClientSecret(id),
    onSuccess: (r, id) => {
      setSecretModal({ clientId: id, open: true, secret: r?.secret ?? '' });
      invalidate();
    },
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => deleteClient(id),
    onSuccess: () => {
      message.success('已删除');
      invalidate();
    },
  });

  function openCreate() {
    setEditId(null);
    form.resetFields();
    setModalOpen(true);
  }

  function openEdit(row: ClientRow) {
    setEditId(row.id);
    form.setFieldsValue({
      name: row.name,
      remark: row.remark,
      rate_limit: row.rate_limit ?? '',
      max_conns: row.max_conns ?? null,
      max_services: row.max_services ?? null,
      flow_limit_mb: row.flow_limit ? Math.round(row.flow_limit / 1024 / 1024) : null,
    });
    setModalOpen(true);
  }

  async function submit() {
    const values = await form.validateFields();
    if (editId) {
      updateMut.mutate({ id: editId, data: values });
    } else {
      createMut.mutate(values);
    }
  }

  const columns: ColumnsType<ClientRow> = [
    {
      title: '名称',
      dataIndex: 'name',
      key: 'name',
      render: (v: string) => <span style={{ fontWeight: 500 }}>{v}</span>,
    },
    {
      title: 'ID',
      dataIndex: 'id',
      key: 'id',
      width: 150,
      // UUID 后 8 位 + 一键复制,悬停 title 展示全量(§6.4)
      render: (_: unknown, row: ClientRow) => (
        <MonoCopy ariaLabel="复制客户端 ID" display={row.id.slice(-8)} full={row.id} />
      ),
    },
    {
      title: '在线',
      dataIndex: 'online',
      key: 'online',
      width: 150,
      render: (v: boolean, row: ClientRow) =>
        v ? (
          <StatusBadge signal="ok" text="在线" />
        ) : (
          <Tooltip title={row.last_online_at ? `最近在线 ${relTime(row.last_online_at)}` : '从未上线'}>
            <StatusBadge signal="off" text={`离线 · ${relTime(row.last_online_at)}`} />
          </Tooltip>
        ),
    },
    {
      title: '启用',
      dataIndex: 'enabled',
      key: 'enabled',
      width: 76,
      render: (v: boolean, row: ClientRow) => {
        // 乐观受控:PUT 完成到列表重取完成之间开关即反映目标态,防快速双击发出重复反向请求
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
      title: '版本 / 主机',
      key: 'meta',
      width: 210,
      render: (_, row: ClientRow) => (
        <Mono>{`${row.client_version || '-'} / ${row.hostname || '-'}`}</Mono>
      ),
    },
    ...(hasMeter
      ? [
          {
            title: '流量(入/出)',
            key: 'bytes',
            width: 170,
            render: (_: unknown, row: ClientRow) => `${fmtBytes(row.bytes_in)} / ${fmtBytes(row.bytes_out)}`,
          },
        ]
      : []),
    ...(hasConns
      ? [
          {
            title: '连接',
            key: 'conns',
            width: 90,
            render: (_: unknown, row: ClientRow) =>
              row.max_conns ? `${row.conns_current ?? 0}/${row.max_conns}` : `${row.conns_current ?? 0}`,
          },
        ]
      : []),
    { title: '备注', dataIndex: 'remark', key: 'remark', ellipsis: true },
    {
      title: '操作',
      key: 'action',
      width: 230,
      render: (_, row: ClientRow) => (
        <>
          <Button icon={<EditOutlined />} onClick={() => openEdit(row)} size="small" type="link">
            编辑
          </Button>
          <Popover
            content={
              <div>
                <div>轮换后旧密钥立即失效,客户端需更新配置重连。</div>
                <Button
                  danger
                  onClick={() => rotateMut.mutate(row.id)}
                  size="small"
                  style={{ marginTop: 8 }}
                  type="primary"
                >
                  确认轮换
                </Button>
              </div>
            }
            title="确认轮换密钥?"
            trigger="click"
          >
            <Button icon={<KeyOutlined />} size="small" type="link">
              轮换密钥
            </Button>
          </Popover>
          <Popconfirm
            onConfirm={() => deleteMut.mutate(row.id)}
            title={`删除客户端 ${row.name} 及其全部规则?此操作不可恢复。`}
          >
            <Button danger size="small" type="link">
              删除
            </Button>
          </Popconfirm>
        </>
      ),
    },
  ];

  return (
    <PageContainer subTitle="接入 rabbithole 的边缘客户端" title="客户端">
      <Card
        extra={
          <Button icon={<ReloadOutlined />} onClick={() => invalidate()} size="small">
            刷新
          </Button>
        }
      >
        <Button icon={<PlusOutlined />} onClick={openCreate} type="primary">
          新建客户端
        </Button>
        <Table
          columns={columns}
          dataSource={rows}
          expandable={{
            expandedRowRender: (row: ClientRow) => (
              <div style={{ display: 'flex', gap: 40, flexWrap: 'wrap', padding: '4px 8px' }}>
                <div>
                  <div style={{ color: 'rgba(0,0,0,0.45)', fontSize: 12 }}>连接 / 并发上限</div>
                  <Mono style={{ fontSize: 15 }}>{`${row.conns_current ?? 0}${row.max_conns ? ` / ${row.max_conns}` : ''}`}</Mono>
                </div>
                <div>
                  <div style={{ color: 'rgba(0,0,0,0.45)', fontSize: 12 }}>流量(入 / 出)</div>
                  <Mono style={{ fontSize: 15 }}>{`${fmtBytes(row.bytes_in)} / ${fmtBytes(row.bytes_out)}`}</Mono>
                </div>
                <div>
                  <div style={{ color: 'rgba(0,0,0,0.45)', fontSize: 12 }}>规则数上限</div>
                  <Mono style={{ fontSize: 15 }}>{row.max_services || '不限'}</Mono>
                </div>
                <div>
                  <div style={{ color: 'rgba(0,0,0,0.45)', fontSize: 12 }}>限速</div>
                  <Mono style={{ fontSize: 15 }}>{row.rate_limit || '不限'}</Mono>
                </div>
              </div>
            ),
          }}
          loading={isLoading}
          locale={{
            emptyText: (
              <Empty
                description="还没有客户端。创建客户端后,控制台会给出接入配置,客户端进程上线即可承载转发规则。"
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                style={{ padding: '32px 0' }}
              >
                <Button onClick={openCreate} type="primary">
                  新建客户端
                </Button>
              </Empty>
            ),
          }}
          pagination={{ pageSize: 50, hideOnSinglePage: true }}
          rowKey="id"
          size="middle"
          style={{ marginTop: 12 }}
        />
      </Card>

      <Modal
        cancelText="取消"
        okButtonProps={{ loading: createMut.isPending || updateMut.isPending }}
        okText="保存"
        onCancel={() => setModalOpen(false)}
        onOk={submit}
        open={modalOpen}
        title={editId ? '编辑客户端' : '新建客户端'}
      >
        <Form form={form} labelCol={{ span: 5 }} wrapperCol={{ span: 17 }}>
          <Form.Item
            label="名称"
            name="name"
            rules={[
              { required: true, message: '请输入名称' },
              { max: 64, message: '最多 64 字符' },
            ]}
          >
            <Input placeholder="edge-1" />
          </Form.Item>
          <Form.Item label="备注" name="remark">
            <Input.TextArea placeholder="可选" rows={2} />
          </Form.Item>
          <Form.Item label="高级配置" style={{ marginBottom: 0 }}>
            <Card size="small" style={{ background: '#fafafa' }}>
              <div style={{ color: '#999', fontSize: 12, marginBottom: 8 }}>
                配额保存即生效:并发/流量上限在接入时执行,限速在 TCP 隧道转发中执行
              </div>
              <Form.Item label="限速" name="rate_limit" style={{ marginBottom: 8 }}>
                <Input placeholder="如 1MB / 512KB,留空不限" />
              </Form.Item>
              <Form.Item label="最大连接" name="max_conns" style={{ marginBottom: 8 }}>
                <InputNumber min={0} placeholder="0 = 不限" precision={0} style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item label="最大规则数" name="max_services" style={{ marginBottom: 8 }}>
                <InputNumber min={0} placeholder="0 = 不限" precision={0} style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item label="流量上限(MB)" name="flow_limit_mb" style={{ marginBottom: 0 }}>
                <InputNumber min={0} placeholder="0 = 不限" precision={0} style={{ width: '100%' }} />
              </Form.Item>
            </Card>
          </Form.Item>
        </Form>
      </Modal>

      <SecretModal
        clientId={secretModal.clientId}
        onCancel={() => setSecretModal((s) => ({ ...s, open: false }))}
        open={secretModal.open}
        secret={secretModal.secret}
      />
    </PageContainer>
  );
};

export default ClientsPage;
