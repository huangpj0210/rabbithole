import {
  createVisitor,
  deleteVisitor,
  disableVisitor,
  enableVisitor,
  listClients,
  listServices,
  listVisitors,
  updateVisitor,
} from '@/api/rabbithole';
import type { ClientRow, VisitorRow } from '@/api/rabbithole';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PlusOutlined } from '@ant-design/icons';
import { App, Button, Card, Checkbox, Empty, Form, Input, InputNumber, Modal, Popconfirm, Select, Switch, Table, Tag } from 'antd';
import { InfoLabel } from '@/components/ui';
import type { ColumnsType } from 'antd/es/table';
import React, { useEffect, useState } from 'react';

interface VisitorForm {
  client_id: string;
  name: string;
  kind: 'stcp' | 'sudp' | 'xtcp';
  bind_addr: string;
  bind_port: number | null;
  target_proxy_name: string;
  use_compression: boolean;
}

/** 从规则列表「创建访问器」带入的预填(目标规则 + 类型)。 */
export interface VisitorSeed {
  kind: 'stcp' | 'sudp' | 'xtcp';
  target: string;
}

/** 密钥访问页「访问器」面板:STCP/SUDP/XTCP 访问端规则 CRUD。
 *  密钥不在表单出现:访问器密钥留空 = 继承目标规则当前密钥(服务端连接时判定)。 */
const VisitorsPanel: React.FC<{ seed?: VisitorSeed | null; onSeedUsed?: () => void }> = ({
  seed,
  onSeedUsed,
}) => {
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const [modalOpen, setModalOpen] = useState(false);
  const [editId, setEditId] = useState<null | number>(null);
  const [form] = Form.useForm<VisitorForm>();
  // 经 ref 读取一次性 seed,避免 seed 清空触发表单重置
  const seedRef = React.useRef<VisitorSeed | null>(null);

  const visitorsQ = useQuery({ queryKey: ['visitors'], queryFn: () => listVisitors() });
  const clientsQ = useQuery({ queryKey: ['clients'], queryFn: listClients });
  const servicesQ = useQuery({ queryKey: ['services'], queryFn: () => listServices() });
  const clients: ClientRow[] = clientsQ.data ?? [];
  const rows = visitorsQ.data ?? [];

  // 目标代理候选:同类型规则名去重(与服务端解析的 名称+类型 匹配口径一致)
  const targetOptions = Array.from(
    new Set(
      (servicesQ.data ?? [])
        .filter((r) => ['stcp', 'sudp', 'xtcp'].includes(r.kind))
        .map((r) => r.name),
    ),
  )
    .sort()
    .map((n) => ({ label: n, value: n }));

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['visitors'] });
    queryClient.invalidateQueries({ queryKey: ['clients'] });
  };

  const saveMut = useMutation({
    mutationFn: ({ id, values }: { id: null | number; values: VisitorForm }) => {
      const spec = {
        name: values.name,
        kind: values.kind,
        enabled: id ? (rows.find((r) => r.id === id)?.enabled ?? true) : true,
        bind_addr: values.bind_addr,
        bind_port: values.bind_port ?? 0,
        target_proxy_name: values.target_proxy_name,
        use_compression: !!values.use_compression,
        ...(id ? {} : { client_id: values.client_id }),
      };
      return id ? updateVisitor(id, spec) : createVisitor(spec);
    },
    onSuccess: (_d, v) => {
      message.success(v.id ? '已保存' : '已创建');
      setModalOpen(false);
      invalidate();
    },
  });

  const toggleMut = useMutation({
    mutationFn: ({ enabled, row }: { enabled: boolean; row: VisitorRow }) =>
      enabled ? enableVisitor(row.id) : disableVisitor(row.id),
    onSuccess: invalidate,
  });

  const deleteMut = useMutation({
    mutationFn: (id: number) => deleteVisitor(id),
    onSuccess: () => {
      message.success('已删除');
      invalidate();
    },
  });

  const editRow = editId ? rows.find((r) => r.id === editId) : null;
  // 规则页「创建访问器」跳入:带目标与类型打开新建弹窗
  useEffect(() => {
    if (!seed) return;
    seedRef.current = seed;
    setEditId(null);
    setModalOpen(true);
  }, [seed]);
  useEffect(() => {
    if (!modalOpen) return;
    form.resetFields();
    if (editId) {
      const row = rows.find((r) => r.id === editId);
      if (row) {
        form.setFieldsValue({
          client_id: row.client_id,
          name: row.name,
          kind: row.kind as VisitorForm['kind'],
          bind_addr: row.bind_addr,
          bind_port: row.bind_port,
          target_proxy_name: row.target_proxy_name,
          use_compression: !!row.spec?.use_compression,
        });
      }
    } else {
      const s = seedRef.current;
      form.setFieldsValue({
        client_id: clients[0]?.id ?? '',
        name: '',
        kind: s?.kind ?? 'stcp',
        bind_addr: '127.0.0.1',
        bind_port: 6000,
        target_proxy_name: s?.target ?? '',
        use_compression: false,
      });
      if (s) {
        seedRef.current = null;
        onSeedUsed?.();
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modalOpen, editId]);

  const columns: ColumnsType<VisitorRow> = [
    { title: '名称', dataIndex: 'name', key: 'name', width: 140 },
    { title: '类型', dataIndex: 'kind', key: 'kind', width: 80, render: (v: string) => <Tag>{v.toUpperCase()}</Tag> },
    { title: '本地监听', key: 'bind', width: 150, render: (_, row) => <span className="rh-mono">{`${row.bind_addr}:${row.bind_port}`}</span> },
    { title: '目标代理', dataIndex: 'target_proxy_name', key: 'target' },
    {
      title: '压缩',
      key: 'compress',
      width: 70,
      render: (_, row) => (
        <Tag color={row.spec?.use_compression ? 'green' : 'default'}>
          {row.spec?.use_compression ? '是' : '否'}
        </Tag>
      ),
    },
    {
      title: '启用',
      dataIndex: 'enabled',
      key: 'enabled',
      width: 76,
      render: (v: boolean, row: VisitorRow) => {
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
      render: (_, row: VisitorRow) => (
        <>
          <Button onClick={() => { setEditId(row.id); setModalOpen(true); }} size="small" type="link">
            编辑
          </Button>
          <Popconfirm onConfirm={() => deleteMut.mutate(row.id)} title={`删除访问器 ${row.name}?此操作不可恢复。`}>
            <Button danger size="small" type="link">
              删除
            </Button>
          </Popconfirm>
        </>
      ),
    },
  ];

  return (
    <Card>
      <Button icon={<PlusOutlined />} onClick={() => { setEditId(null); setModalOpen(true); }} type="primary">
        新建访问器
      </Button>
      <Table
        columns={columns}
        dataSource={rows}
        loading={visitorsQ.isLoading}
        locale={{
          emptyText: (
            <Empty
              description="还没有访问器。访问器在另一台机器上打开本地端口,经服务端中继或打洞直达密钥规则的目标。"
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              style={{ padding: '32px 0' }}
            >
              <Button onClick={() => { setEditId(null); setModalOpen(true); }} type="primary">
                新建访问器
              </Button>
            </Empty>
          ),
        }}
        pagination={{ pageSize: 50, hideOnSinglePage: true }}
        rowKey="id"
        size="middle"
        style={{ marginTop: 12 }}
      />

      <Modal
        cancelText="取消"
        okButtonProps={{ loading: saveMut.isPending }}
        okText="保存"
        onCancel={() => setModalOpen(false)}
        onOk={() => form.submit()}
        open={modalOpen}
        title={editId ? '编辑访问器' : '新建访问器'}
      >
        <Form
          form={form}
          labelCol={{ span: 6 }}
          onFinish={(values) => saveMut.mutate({ id: editId, values })}
          wrapperCol={{ span: 16 }}
        >
          <Form.Item label={<InfoLabel info="在这台客户端上打开本地监听端口,访问器随它运行" title="所属客户端" />} name="client_id" rules={[{ required: true, message: '请选择客户端' }]}>
            <Select disabled={!!editId} options={clients.map((c) => ({ label: c.name, value: c.id }))} />
          </Form.Item>
          <Form.Item
            label={<InfoLabel info="访问器标识,同一客户端内唯一" title="名称" />}
            name="name"
            rules={[
              { required: true, message: '请输入名称' },
              { pattern: /^[a-zA-Z0-9_-]{1,64}$/, message: '1-64 位字母/数字/下划线/连字符' },
            ]}
          >
            <Input />
          </Form.Item>
          <Form.Item label={<InfoLabel info="须与目标规则类型一致" title="类型" />} name="kind" rules={[{ required: true }]}>
            <Select
              disabled={!!editId}
              options={[
                { label: 'STCP', value: 'stcp' },
                { label: 'SUDP', value: 'sudp' },
                { label: 'XTCP', value: 'xtcp' },
              ]}
            />
          </Form.Item>
          <Form.Item label={<InfoLabel info="访问端本地绑定地址,访问方连接它进入隧道" title="本地监听地址" />} name="bind_addr">
            <Input placeholder="127.0.0.1" />
          </Form.Item>
          <Form.Item label={<InfoLabel info="本地入口端口,流量经服务端中继或打洞直达目标规则" title="本地监听端口" />} name="bind_port" rules={[{ required: true, message: '请输入端口' }]}>
            <InputNumber max={65535} min={1} precision={0} />
          </Form.Item>
          <Form.Item
            label={<InfoLabel info="选择要访问的密钥访问规则,类型须一致;中继凭据自动继承目标规则,无需填写" title="目标代理名" />}
            name="target_proxy_name"
            rules={[{ required: true, message: '请选择目标代理' }]}
          >
            <Select
              notFoundContent="暂无 STCP/SUDP/XTCP 规则,请先在「规则」页创建"
              options={targetOptions}
              placeholder="选择目标规则"
              showSearch
            />
          </Form.Item>
          <Form.Item label={<InfoLabel info="中继通道启用 snappy 压缩" title="压缩" />} name="use_compression" valuePropName="checked">
            <Checkbox>启用 snappy 压缩</Checkbox>
          </Form.Item>
        </Form>
      </Modal>
    </Card>
  );
};

export default VisitorsPanel;
