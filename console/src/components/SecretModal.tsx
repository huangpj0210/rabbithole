import { CopyOutlined } from '@ant-design/icons';
import { App, Button, Input, Modal, Select, Typography } from 'antd';
import React, { useMemo, useState } from 'react';

/** env / toml 模板共用的协议列表(与服务端 bind_protocol 一致)。 */
const PROTOCOLS = ['tcp', 'tls', 'ws', 'wss', 'quic', 'kcp'];

/**
 * 一次性密钥展示弹窗(新建/轮换后的 secret 仅此一次返回)。
 * 首推 Docker 一键命令(纯环境变量接入,凭据已填);折叠区提供 rh-client.toml
 * 模板(裸机/高级项场景)。server_addr 按当前访问主机推测,以实际部署为准。
 */
const SecretModal: React.FC<{
  clientId?: string;
  onCancel: () => void;
  open: boolean;
  secret: string;
  title?: string;
}> = ({ open, secret, clientId, onCancel, title = '客户端凭据(仅此一次展示)' }) => {
  const { message } = App.useApp();
  const [serverAddr, setServerAddr] = useState(`${window.location.hostname}:7100`);
  const [protocol, setProtocol] = useState('tcp');
  const [tab, setTab] = useState<'docker' | 'toml'>('docker');

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

  // Docker 纯 env 接入:3 个必填字符串(协议非默认 tcp 时显式带上)
  const dockerCmd = useMemo(() => {
    const proto = protocol !== 'tcp' ? ` \\\n  -e RH_PROTOCOL=${protocol}` : '';
    return [
      'docker run -d --name rh-client --restart unless-stopped \\',
      `  -e RH_SERVER=${serverAddr} \\${proto}`,
      `  -e RH_CLIENT_ID=${clientId ?? '<客户端ID>'} \\`,
      `  -e RH_SECRET=${secret} \\`,
      '  ghcr.io/xbclub/rabbithole-client:latest',
    ].join('\n');
  }, [serverAddr, protocol, clientId, secret]);

  const toml = useMemo(
    () =>
      [
        `server_addr = "${serverAddr}"`,
        `protocol = "${protocol}"`,
        'multiplex = true',
        ...(clientId ? [`client_id = "${clientId}"`] : []),
        `secret = "${secret}"`,
        'log_level = "info"',
        // 高级项(可选):trusted_ca_file / tls_pin_sha256 / proxy_url / stun_server / dns_server
      ].join('\n'),
    [serverAddr, protocol, clientId, secret],
  );

  const snippet = tab === 'docker' ? dockerCmd : toml;

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
          options={[
            { label: 'Docker 一键命令(推荐)', value: 'docker' },
            { label: 'rh-client.toml 配置模板', value: 'toml' },
          ]}
          style={{ width: 220 }}
          value={tab}
          variant="borderless"
        />
      </div>
      <Input.TextArea
        className="rh-mono"
        readOnly
        rows={tab === 'docker' ? 6 : 7}
        value={snippet}
      />
      <Button
        block
        icon={<CopyOutlined />}
        onClick={() => copyText(snippet)}
        size="small"
        style={{ marginTop: 8 }}
      >
        {tab === 'docker' ? '复制 Docker 命令' : '复制完整配置'}
      </Button>
    </Modal>
  );
};

export default SecretModal;
