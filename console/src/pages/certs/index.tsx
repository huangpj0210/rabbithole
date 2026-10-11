import { createCert, deleteCert, listCerts, updateCert } from '@/api/rabbithole';
import type { CertRow } from '@/api/rabbithole';
import { fmtTime } from '@/utils/format';
import { Mono, MonoCopy } from '@/components/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons';
import { PageContainer } from '@ant-design/pro-components';
import { Alert, App, Button, Card, Empty, Form, Input, Modal, Popconfirm, Table, Tag, Upload } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import React, { useState } from 'react';

interface CertForm {
  name: string;
  cert_pem: string;
  key_pem: string;
  domains: string;
}

/** PEM 结构粗校验(拦截明显误贴,最终以服务端解析为准)。 */
function looksLikePem(text: string, kind: 'CERTIFICATE' | 'PRIVATE KEY'): boolean {
  return (
    text.includes(`-----BEGIN ${kind}-----`) && text.includes(`-----END ${kind}-----`)
  );
}

/**
 * 证书页(F6 前端):按 design-parity §7.3 契约实现;
 * 后端 M3 落地前 GET /api/certs 404 → 空态 + 提示,表单仍完整可用。
 */
const CertsPage: React.FC = () => {
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const [modalOpen, setModalOpen] = useState(false);
  const [editId, setEditId] = useState<null | number>(null);
  const [modalSeq, setModalSeq] = useState(0);
  const [form] = Form.useForm<CertForm>();

  const certsQ = useQuery({
    queryKey: ['certs'],
    queryFn: listCerts,
    retry: false,
  });
  // M3 未落地:404 → 空态提示
  const notImplemented =
    !!certsQ.error && /404|not found|Request failed/i.test(String((certsQ.error as Error)?.message));

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['certs'] });

  const saveMut = useMutation({
    mutationFn: ({ id, values }: { id: null | number; values: CertForm }) => {
      const payload = {
        name: values.name,
        cert_pem: values.cert_pem,
        key_pem: values.key_pem,
        ...(values.domains.trim()
          ? { domains: values.domains.split(',').map((s) => s.trim()).filter(Boolean) }
          : {}),
      };
      return id ? updateCert(id, payload) : createCert(payload);
    },
    onSuccess: (_d, v) => {
      message.success(v.id ? '证书已更新' : '证书已上传');
      setModalOpen(false);
      invalidate();
    },
  });

  const deleteMut = useMutation({
    mutationFn: (id: number) => deleteCert(id),
    onSuccess: () => {
      message.success('证书已删除');
      invalidate();
    },
  });

  function openCreate() {
    setEditId(null);
    setModalSeq((n) => n + 1);
    setModalOpen(true);
  }

  function openEdit(row: CertRow) {
    setEditId(row.id);
    setModalSeq((n) => n + 1);
    setModalOpen(true);
    // key_pem 任何响应都不回显,编辑时需重新粘贴;等弹窗字段挂载后注入
    setTimeout(() => {
      form.setFieldsValue({
        name: row.name,
        cert_pem: '',
        key_pem: '',
        domains: (row.domains ?? []).join(','),
      });
    }, 60);
  }

  const columns: ColumnsType<CertRow> = [
    { title: '名称', dataIndex: 'name', key: 'name', width: 180 },
    {
      title: '域名',
      key: 'domains',
      render: (_, row) =>
        row.domains?.length ? (
          <>
            {row.domains.slice(0, 2).map((d) => (
              <Tag key={d} color="blue">
                {d}
              </Tag>
            ))}
            {row.domains.length > 2 ? <Tag>+{row.domains.length - 2}</Tag> : null}
          </>
        ) : (
          '-'
        ),
    },
    {
      // 到期倒计时是证书页核心信息(§6.3):<7 天红、<30 天橙
      title: '到期',
      key: 'expiry',
      width: 210,
      render: (_, row) => {
        if (!row.not_after) return '-';
        const days = Math.floor((row.not_after - Date.now()) / 86400000);
        const color = days < 7 ? 'red' : days < 30 ? 'orange' : 'green';
        return (
          <>
            <Tag color={color}>{days < 0 ? '已过期' : `${days} 天`}</Tag>
            <Mono style={{ color: 'rgba(0,0,0,0.45)' }}>{fmtTime(row.not_after)}</Mono>
          </>
        );
      },
    },
    {
      title: '指纹',
      dataIndex: 'fingerprint',
      key: 'fingerprint',
      width: 150,
      render: (v: string) =>
        v ? <MonoCopy ariaLabel="复制证书指纹" display={`${v.slice(0, 8)}…`} full={v} /> : '-',
    },
    {
      title: '操作',
      key: 'action',
      width: 150,
      render: (_, row: CertRow) => (
        <>
          <Button onClick={() => openEdit(row)} size="small" type="link">
            编辑
          </Button>
          <Popconfirm
        cancelText="取消"
        okButtonProps={{ danger: true }}
        okText="删除"
        onConfirm={() => deleteMut.mutate(row.id)}
        title={`删除证书 ${row.name}?绑定该域名的 HTTPS 将回退为端到端透传。`}
      >
            <Button danger icon={<DeleteOutlined />} size="small" type="link">
              删除
            </Button>
          </Popconfirm>
        </>
      ),
    },
  ];

  async function handleFinish(v: CertForm) {
    if (!looksLikePem(v.cert_pem, 'CERTIFICATE')) {
      form.setFields([{ errors: ['证书 PEM 结构非法(缺 BEGIN/END CERTIFICATE)'], name: 'cert_pem' }]);
      return;
    }
    if (!looksLikePem(v.key_pem, 'PRIVATE KEY')) {
      form.setFields([{ errors: ['私钥 PEM 结构非法(缺 BEGIN/END PRIVATE KEY)'], name: 'key_pem' }]);
      return;
    }
    saveMut.mutate({ id: editId, values: v });
  }

  return (
    <PageContainer subTitle="按 SNI 自动匹配、服务端终结 TLS 的 HTTPS 证书" title="证书">
      {notImplemented && (
        <Alert
          message="服务端尚未实现证书管理(M3)"
          showIcon
          style={{ marginBottom: 12 }}
          type="info"
          description="当前服务端无 /api/certs 端点;下方表单已按契约实现,M3 落地后即可直接使用。"
        />
      )}
      <Card>
        <Button icon={<PlusOutlined />} onClick={openCreate} type="primary">
          上传证书
        </Button>
        <Table
          columns={columns}
          dataSource={certsQ.data ?? []}
          loading={certsQ.isLoading}
          locale={{
            emptyText: notImplemented ? (
              '暂无证书(需服务端 M3)'
            ) : (
              <Empty
                description="还没有证书。上传 PEM 证书后,HTTPS 规则将按 SNI 自动匹配并由服务端终结 TLS。"
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                style={{ padding: '32px 0' }}
              >
                <Button onClick={openCreate} type="primary">
                  上传证书
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
        okButtonProps={{ loading: saveMut.isPending }}
        okText="保存"
        onCancel={() => setModalOpen(false)}
        onOk={() => form.submit()}
        open={modalOpen}
        title={editId ? `编辑证书 #${editId}` : '上传证书'}
        width={640}
      >
        <Form
          form={form}
          key={modalSeq}
          labelCol={{ span: 4 }}
          onFinish={handleFinish}
          wrapperCol={{ span: 19 }}
        >
          <Form.Item label="名称" name="name" rules={[{ required: true, message: '请输入名称' }]}>
            <Input placeholder="wildcard-example" />
          </Form.Item>
          <Form.Item label="域名" name="domains">
            <Input placeholder="留空从证书 SAN/CN 自动提取" />
          </Form.Item>
          <Form.Item
            label="证书 PEM"
            name="cert_pem"
            rules={[{ required: true, message: '请粘贴或上传证书 PEM' }]}
          >
            <Input.TextArea placeholder="-----BEGIN CERTIFICATE-----…" rows={5} />
          </Form.Item>
          <Form.Item
            extra="私钥仅上传时提交,任何响应不回显;编辑时需重新粘贴"
            label="私钥 PEM"
            name="key_pem"
            rules={[{ required: true, message: '请粘贴或上传私钥 PEM' }]}
          >
            <Input.Password placeholder="-----BEGIN PRIVATE KEY-----…" visibilityToggle />
          </Form.Item>
          <Form.Item label="文件上传">
            <Upload
              accept=".pem,.crt,.key,.txt"
              beforeUpload={async (file) => {
                const text = await file.text();
                if (file.name.includes('key') || looksLikePem(text, 'PRIVATE KEY')) {
                  form.setFieldsValue({ key_pem: text.trim() });
                } else {
                  form.setFieldsValue({ cert_pem: text.trim() });
                }
                return false;
              }}
              showUploadList={false}
            >
              <Button size="small">选择证书/私钥文件自动填充</Button>
            </Upload>
          </Form.Item>
        </Form>
      </Modal>
    </PageContainer>
  );
};

export default CertsPage;
