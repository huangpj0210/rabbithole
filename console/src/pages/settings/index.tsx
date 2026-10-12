import { changePassword, getServerConfig, putServerConfig } from '@/api/rabbithole';
import type { ServerConfigUpdate } from '@/api/rabbithole';
import { useAuthStore } from '@/stores/auth';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { history, useLocation } from '@umijs/max';
import { PageContainer } from '@ant-design/pro-components';
import { InfoLabel } from '@/components/ui';
import { Alert, App, Button, Card, Checkbox, Form, Input, InputNumber, Select, Tabs, theme, Typography } from 'antd';
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

/** tab 面板内卡片统一最大宽度(表单/说明类窄卡,避免随内容区整行拉伸)。 */
const CARD_MAX_WIDTH = 560;

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

  return (
    <Card loading={cfgQ.isLoading} style={{ maxWidth: CARD_MAX_WIDTH }}>
      {cfgQ.isError ? (
        // 加载失败不渲染表单:空表单点「保存配置」会把端口策略清空(P0 同源风险)
        <Alert
          action={
            <Button loading={cfgQ.isFetching} onClick={() => cfgQ.refetch()} size="small">
              重试
            </Button>
          }
          description={cfgQ.error?.message ?? '未知错误'}
          message="加载服务器配置失败"
          showIcon
          type="error"
        />
      ) : (
        <>
          <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
            以下配置保存后立即生效并写回配置文件(原文件备份为 *.toml.bak,注释会丢失)。vhost 端口、主监听等其余配置仍需修改文件并重启。
          </Typography.Paragraph>
          <Form form={sshForm} labelCol={{ span: 6 }} layout="horizontal" wrapperCol={{ span: 14 }}>
            <Typography.Title level={5}>SSH Tunnel Gateway</Typography.Title>
            <Form.Item label={<InfoLabel info="开启后可用 ssh -R 在服务端开临时转发端口,凭据与客户端一致" title="启用" />} name="enabled" valuePropName="checked">
              <Checkbox>开启 ssh -R 隧道入口</Checkbox>
            </Form.Item>
            <Form.Item label={<InfoLabel info="SSH 网关绑定地址,留空 = 0.0.0.0" title="监听地址" />} name="bind_addr">
              <Input placeholder="0.0.0.0" />
            </Form.Item>
            <Form.Item label={<InfoLabel info="SSH 网关端口" title="监听端口" />} name="bind_port">
              <InputNumber max={65535} min={1} precision={0} />
            </Form.Item>
            <Form.Item label={<InfoLabel info="每行一个公钥,用户名须为已启用客户端 ID;密码认证(客户端密钥)始终可用" title="公钥白名单" />}>
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
            <Form.Item label={<InfoLabel info="逗号分隔,支持范围如 10000-20000,30000;空 = 不限制" title="允许端口" />} name="allow_ports">
              <Input placeholder="10000-40000" />
            </Form.Item>
            <Form.Item label={<InfoLabel info="全局默认上限;单个客户端可在「客户端」页覆盖,0 = 不限" title="每客户端最大规则数" />} name="max_services_per_client">
              <InputNumber max={10000} min={0} precision={0} />
            </Form.Item>
            <Typography.Title level={5}>日志</Typography.Title>
            <Form.Item label={<InfoLabel info="服务端日志级别,保存立即生效" title="级别" />}>
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
        </>
      )}
    </Card>
  );
};

/** 修改密码:成功后服务端轮换 JWT 密钥,所有会话失效需重登。 */
const PasswordCard: React.FC = () => {
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

  return (
    <Card style={{ maxWidth: CARD_MAX_WIDTH }}>
      <Alert
        message="修改成功后所有会话将立即失效,需重新登录"
        showIcon
        style={{ marginBottom: 16 }}
        type="warning"
      />
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
  );
};

/** 监控指标:Prometheus 抓取地址与接入前提。 */
const MetricsCard: React.FC = () => {
  const { token } = theme.useToken();
  const metricsUrl = `${window.location.origin}/metrics`;
  return (
    <Card style={{ maxWidth: CARD_MAX_WIDTH }}>
      <Typography.Paragraph>
        Prometheus 抓取地址:
        <Typography.Text aria-label="复制 Prometheus 地址" className="rh-mono" copyable={{ tooltips: ['复制', '已复制'] }}>
          {metricsUrl}
        </Typography.Text>
      </Typography.Paragraph>
      <ol style={{ paddingLeft: 20, color: token.colorTextTertiary, fontSize: 13, lineHeight: 2 }}>
        <li>服务端配置 <span className="rh-mono">enable_prometheus = true</span> 并重启;</li>
        <li>Prometheus 抓取任务携带与管理台相同的 Bearer 令牌;</li>
        <li>可用指标:连接数、在线客户端、入/出站累计字节数。</li>
      </ol>
    </Card>
  );
};

const SETTINGS_TABS = ['server', 'metrics', 'password'] as const;

/** 设置页:服务器配置 / 监控指标 / 修改密码 三 tab,tab 状态写 query 持久化(同密钥访问页)。 */
const SettingsPage: React.FC = () => {
  const location = useLocation();
  const tabParam = new URLSearchParams(location.search).get('tab');
  const tab = (SETTINGS_TABS as readonly string[]).includes(tabParam ?? '')
    ? (tabParam as (typeof SETTINGS_TABS)[number])
    : 'server';
  return (
    <PageContainer subTitle="服务器配置、监控指标与修改密码" title="设置">
      <Tabs
        activeKey={tab}
        items={[
          { key: 'server', label: '服务器配置', children: <ServerConfigCard /> },
          { key: 'metrics', label: '监控指标', children: <MetricsCard /> },
          { key: 'password', label: '修改密码', children: <PasswordCard /> },
        ]}
        onChange={(key) => history.replace(`/rabbithole/settings?tab=${key}`)}
      />
    </PageContainer>
  );
};

export default SettingsPage;
