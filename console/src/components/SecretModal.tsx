import { CopyOutlined } from '@ant-design/icons';
import { App, Button, Input, Modal, Select, Typography } from 'antd';
import React, { useMemo, useState } from 'react';

/** env / toml 模板共用的协议列表(与服务端 bind_protocol 一致)。 */
const PROTOCOLS = ['tls', 'tcp', 'ws', 'wss', 'quic', 'kcp'];
const SCRIPT_URL =
  'https://raw.githubusercontent.com/xbclub/rabbithole/main/deploy/install-client.sh';

/**
 * 一次性密钥展示弹窗(新建/轮换后的 secret 仅此一次返回)。
 * 三档接入凭据全填好:一键脚本(推荐,Linux systemd)/ Docker 一键命令 /
 * rh-client.toml 模板(裸机手动)。server_addr 按当前访问主机推测,可编辑。
 */
const SecretModal: React.FC<{
  clientId?: string;
  onCancel: () => void;
  open: boolean;
  secret: string;
  title?: string;
}> = ({ open, secret, clientId, onCancel, title = '客户端凭据(仅此一次展示)' }) => {
  const { message } = App.useApp();
  // 默认 tls:与服务端一键部署脚本/生产建议一致;纯内网 tcp 部署自行切换
  const [serverAddr, setServerAddr] = useState(`${window.location.hostname}:7100`);
  const [protocol, setProtocol] = useState('tls');
  const [tab, setTab] = useState<'script' | 'docker' | 'toml'>('script');

  async function copyText(text: string) {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
        message.success('已复制');
        return;
      }
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.append(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      if (ok) {
        message.success('已复制');
      } else {
        message.warning('复制失败,请手动选中复制');
      }
    } catch {
      message.warning('复制失败,请手动选中复制');
    }
  }

  const id = clientId ?? '<客户端ID>';
  // 一键脚本:下载 + 校验 + systemd 服务(凭据经 /etc/rabbithole/client.env 接入)
  const scriptCmd = useMemo(
    () =>
      `curl -fsSL ${SCRIPT_URL} | sudo bash -s -- \\\n  --server ${serverAddr} --protocol ${protocol} \\\n  --id ${id} --secret ${secret}`,
    [serverAddr, protocol, id, secret],
  );
  // Docker:纯环境变量接入
  const dockerCmd = useMemo(
    () =>
      [
        'docker run -d --name rh-client --restart unless-stopped \\',
        `  -e RH_SERVER=${serverAddr} \\`,
        `  -e RH_PROTOCOL=${protocol} \\`,
        `  -e RH_CLIENT_ID=${id} \\`,
        `  -e RH_SECRET=${secret} \\`,
        '  ghcr.io/xbclub/rabbithole-client:latest',
      ].join('\n'),
    [serverAddr, protocol, id, secret],
  );
  const toml = useMemo(
    () =>
      [
        `server_addr = "${serverAddr}"`,
        `protocol = "${protocol}"`,
        'multiplex = true',
        `client_id = "${id}"`,
        `secret = "${secret}"`,
        'log_level = "info"',
        // 高级项(可选):trusted_ca_file / tls_pin_sha256 / proxy_url / stun_server / dns_server
      ].join('\n'),
    [serverAddr, protocol, id, secret],
  );

  const snippet = tab === 'script' ? scriptCmd : tab === 'docker' ? dockerCmd : toml;
  const TABS = [
    { label: '一键安装脚本(推荐)', value: 'script' },
    { label: 'Docker 一键命令', value: 'docker' },
    { label: 'rh-client.toml 模板', value: 'toml' },
  ];
  const COPY_LABEL: Record<typeof tab, string> = {
    script: '复制安装命令',
    docker: '复制 Docker 命令',
    toml: '复制完整配置',
  };

  return (
    <Modal
      footer={
        <Button onClick={onCancel} type="primary">
          我已保存,关闭
        </Button>
      }
      onCancel={onCancel}
      open={open}
      title={title}
      width={600}
    >
      <Typography.Paragraph type="warning">
        请立即复制密钥并配置到客户端,关闭后无法再次查看:
      </Typography.Paragraph>
      <Input readOnly value={secret} />
      <Button
        block
        icon={<CopyOutlined />}
        onClick={() => copyText(secret)}
        style={{ marginTop: 12 }}
        type="primary"
        ghost
      >
        复制密钥
      </Button>

      <div style={{ display: 'flex', gap: 8, margin: '16px 0 8px' }}>
        <Input
          addonBefore="server_addr"
          onChange={(e) => setServerAddr(e.target.value)}
          style={{ flex: 1 }}
          value={serverAddr}
        />
        <Select
          onChange={(v) => setProtocol(v)}
          options={PROTOCOLS.map((p) => ({ label: p, value: p }))}
          style={{ width: 96 }}
          value={protocol}
        />
      </div>
      <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
        默认地址按当前访问主机 + 控制端口 7100 推测,协议与服务端 bind_protocol 一致,以实际部署为准。
      </Typography.Paragraph>
      <div style={{ marginBottom: 8 }}>
        <Select
          onChange={(v) => setTab(v)}
          options={TABS}
          style={{ width: 210 }}
          value={tab}
          variant="borderless"
        />
      </div>
      <Input.TextArea className="rh-mono" readOnly rows={7} value={snippet} />
      <Button
        block
        icon={<CopyOutlined />}
        onClick={() => copyText(snippet)}
        size="small"
        style={{ marginTop: 8 }}
      >
        {COPY_LABEL[tab]}
      </Button>
      {tab === 'script' && (
        <Typography.Paragraph style={{ fontSize: 12, marginBottom: 0, marginTop: 8 }} type="secondary">
          在内网 Linux 机器执行;自动下载二进制并注册 systemd 服务,凭据写入
          /etc/rabbithole/client.env(0600)。轮换密钥后重跑同一命令即可。需 v0.2.2+。
        </Typography.Paragraph>
      )}
    </Modal>
  );
};

export default SecretModal;
