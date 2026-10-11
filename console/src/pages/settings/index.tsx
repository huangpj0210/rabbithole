import { changePassword, getServerConfig, putServerConfig } from '@/api/rabbithole';
import type { ServerConfigUpdate } from '@/api/rabbithole';
import { useAuthStore } from '@/stores/auth';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { history } from '@umijs/max';
import { PageContainer } from '@ant-design/pro-components';
import { Alert, App, Button, Card, Checkbox, Form, Input, InputNumber, Select, Typography } from 'antd';
import React from 'react';

interface PasswordForm {
  oldPassword: string;
  newPassword: string;
  confirmPassword: string;
}

interface SshForm {
  enabled: boolean;
  bind_addr: string;
  bind_port: number | null;
}

interface PortsForm {
  allow_ports: string;
  max_services_per_client: number | null;
}

/** 服务器配置(热生效子集):SSH 网关 / 端口策略 / 日志级别 / 公钥白名单。 */
const ServerConfigCard: React.FC = () => {
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const [sshForm] = Form.useForm<SshForm>();
  const [portsForm] = Form.useForm<PortsForm>();
  const [logLevel, setLogLevel] = React.useState('info');
  const [keys, setKeys] = React.useState('');

  const cfgQ = useQuery({
    queryKey: ['server-config'],
    queryFn: getServerConfig,
  });

  // 加载后回填(全部字段常驻渲染,无 setFieldsValue 挂起风险)
  React.useEffect(() => {
    if (!cfgQ.data) return;
    const h = cfgQ.data.hot;
    sshForm.setFieldsValue({
      enabled: h.ssh_gateway.enabled,
      bind_addr: h.ssh_gateway.bind_addr || '0.0.0.0',
      bind_port: h.ssh_gateway.bind_port || 2222,
    });
    portsForm.setFieldsValue({
      allow_ports: h.allow_ports.join(','),
      max_services_per_client: h.max_services_per_client,
    });
    setLogLevel(h.log_level || 'info');
    setKeys(cfgQ.data.authorized_keys ?? '');
  }, [cfgQ.data, sshForm, portsForm]);

  const saveMut = useMutation({
    mutationFn: (u: ServerConfigUpdate) => putServerConfig(u),
    onSuccess: (r) => {
      if (r.persisted) {
        message.success(`已生效:${r.applied.join(', ') || '(无变更)'}`);
      } else {
        message.warning(`已生效但写回文件失败:${r.error ?? '未知错误'}`);
      }
      queryClient.invalidateQueries({ queryKey: ['server-config'] });
    },
  });

  function saveAll() {
    const ssh = sshForm.getFieldsValue();
    const ports = portsForm.getFieldsValue();
    saveMut.mutate({
      ssh_gateway: {
        enabled: !!ssh.enabled,
        bind_addr: (ssh.bind_addr || '').trim(),
        bind_port: ssh.bind_port ?? 2222,
      },
      ports: {
        allow_ports: (ports.allow_ports ?? '')
          .split(',')
          .map((x: string) => x.trim())
          .filter(Boolean),
        max_services_per_client: ports.max_services_per_client ?? 0,
      },
      log: { level: logLevel },
      authorized_keys: keys,
    });
  }

  const hot = cfgQ.data?.hot;
  const restartNote = hot ? undefined : '加载中…';

  return (
    <Card
      loading={cfgQ.isLoading}
      style={{ marginTop: 16 }}
      title="服务器配置"
      styles={{ header: { borderBottom: 'none' } }}
      extra={<span style={{ fontSize: 12, color: 'rgba(0,0,0,0.45)' }}>保存后立即生效,无需重启</span>}
    >
      <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
        {restartNote ??
          '以下配置保存后立即生效并写回配置文件(原文件备份为 *.toml.bak,注释会丢失)。vhost 端口、主监听等其余配置仍需修改文件并重启。'}
      </Typography.Paragraph>
      <Form form={sshForm} labelCol={{ span: 6 }} layout="horizontal" wrapperCol={{ span: 14 }}>
        <Typography.Title level={5}>SSH Tunnel Gateway</Typography.Title>
        <Form.Item label="启用" name="enabled" valuePropName="checked">
          <Checkbox>开启 ssh -R 隧道入口</Checkbox>
        </Form.Item>
        <Form.Item extra="留空 = 0.0.0.0" label="监听地址" name="bind_addr">
          <Input placeholder="0.0.0.0" />
        </Form.Item>
        <Form.Item label="监听端口" name="bind_port">
          <InputNumber max={65535} min={1} precision={0} />
        </Form.Item>
        <Form.Item extra="每行一个公钥(用户名须为已启用客户端 ID);密码认证仍可用(客户端密钥)" label="公钥白名单">
          <Input.TextArea
            onChange={(e) => setKeys(e.target.value)}
            placeholder={'ssh-ed25519 AAAA... client-id\nssh-ed25519 AAAA... edge-2'}
            rows={4}
            value={keys}
          />
        </Form.Item>
      </Form>
      <Form form={portsForm} labelCol={{ span: 6 }} layout="horizontal" wrapperCol={{ span: 14 }}>
        <Typography.Title level={5}>端口策略</Typography.Title>
        <Form.Item extra="逗号分隔,支持范围(如 10000-20000,30000);空 = 不限制" label="允许端口">
          <Input placeholder="10000-40000" />
        </Form.Item>
        <Form.Item extra="0 = 不限;客户端级配额在「客户端」页按客户端覆盖" label="每客户端最大规则数">
          <InputNumber max={10000} min={0} precision={0} />
        </Form.Item>
        <Typography.Title level={5}>日志</Typography.Title>
        <Form.Item label="级别">
          <Select
            onChange={(v) => setLogLevel(v)}
            options={['trace', 'debug', 'info', 'warn', 'error'].map((v) => ({ label: v, value: v }))}
            style={{ width: 160 }}
            value={logLevel}
          />
        </Form.Item>
      </Form>
      <Button
        htmlType="button"
        loading={saveMut.isPending}
        onClick={saveAll}
        style={{ marginTop: 8 }}
        type="primary"
      >
        保存配置
      </Button>
      {saveMut.data && !saveMut.data.persisted && (
        <Alert
          description={saveMut.data.error}
          message="写回配置文件失败(运行时已生效,重启后会回退)"
          showIcon
          style={{ marginTop: 12 }}
          type="warning"
        />
      )}
    </Card>
  );
};

/** 设置页:修改密码(成功后强制重登)+ 服务器配置 + Prometheus 地址展示。 */
const SettingsPage: React.FC = () => {
  const [form] = Form.useForm<PasswordForm>();
  const { message } = App.useApp();

  const changeMut = useMutation({
    mutationFn: (v: PasswordForm) => changePassword(v.oldPassword, v.newPassword),
    onSuccess: () => {
      message.success('密码已修改,所有会话已失效,请重新登录');
      // 后端已轮换 JWT 密钥:当前令牌必然失效,清 token 回登录页
      useAuthStore.getState().clear();
      history.replace('/user/login');
    },
  });

  const metricsUrl = `${window.location.origin}/metrics`;

  return (
    <PageContainer subTitle="修改密码与部署信息" title="设置">
      <Card style={{ maxWidth: 560 }} title="修改密码">
        <Form
          form={form}
          labelCol={{ span: 6 }}
          layout="horizontal"
          onFinish={(values) => changeMut.mutate(values)}
          wrapperCol={{ span: 16 }}
        >
          <Form.Item label="当前密码" name="oldPassword" rules={[{ required: true, message: '请输入当前密码' }]}>
            <Input.Password autoComplete="current-password" placeholder="输入当前密码" />
          </Form.Item>
          <Form.Item
            label="新密码"
            name="newPassword"
            rules={[
              { required: true, message: '请输入新密码' },
              { min: 8, message: '新密码至少 8 位' },
            ]}
          >
            <Input.Password autoComplete="new-password" placeholder="至少 8 位" />
          </Form.Item>
          <Form.Item
            dependencies={['newPassword']}
            label="确认新密码"
            name="confirmPassword"
            rules={[
              { required: true, message: '请再次输入新密码' },
              ({ getFieldValue }) => ({
                validator(_, value) {
                  if (!value || getFieldValue('newPassword') === value) {
                    return Promise.resolve();
                  }
                  return Promise.reject(new Error('两次输入的新密码不一致'));
                },
              }),
            ]}
          >
            <Input.Password autoComplete="new-password" placeholder="再次输入新密码" />
          </Form.Item>
          <Form.Item style={{ marginBottom: 0 }} wrapperCol={{ offset: 6 }}>
            <Button htmlType="submit" loading={changeMut.isPending} type="primary">
              修改密码
            </Button>
          </Form.Item>
        </Form>
      </Card>

      <Card style={{ maxWidth: 560, marginTop: 16 }} title="监控指标">
        <Typography.Paragraph>
          Prometheus 抓取地址:
          <Typography.Text aria-label="复制 Prometheus 地址" className="rh-mono" copyable={{ tooltips: ['复制', '已复制'] }}>
            {metricsUrl}
          </Typography.Text>
        </Typography.Paragraph>
        <ol style={{ paddingLeft: 20, color: 'rgba(0,0,0,0.55)', fontSize: 13, lineHeight: 2 }}>
          <li>服务端配置 <span className="rh-mono">enable_prometheus = true</span> 并重启;</li>
          <li>Prometheus 抓取任务携带与管理台相同的 Bearer 令牌;</li>
          <li>可用指标:连接数、在线客户端、入/出站累计字节数。</li>
        </ol>
      </Card>

      <ServerConfigCard />
    </PageContainer>
  );
};

export default SettingsPage;
