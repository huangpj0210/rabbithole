import { CopyOutlined } from '@ant-design/icons';
import { App, Button, Collapse, Input, Modal, Select, Typography } from 'antd';
import React, { useState } from 'react';

/**
 * 一次性密钥展示弹窗(新建/轮换后的 secret 仅此一次返回),
 * 内嵌 rh-client.toml 接入模板折叠区(F8):server_addr 可编辑、
 * transport noise/tls 下拉、client_id/secret 已填,一键复制。
 */
const SecretModal: React.FC<{
  clientId?: string;
  onCancel: () => void;
  open: boolean;
  secret: string;
  title?: string;
}> = ({ open, secret, clientId, onCancel, title = '客户端凭据(仅此一次展示)' }) => {
  const { message } = App.useApp();
  const [serverAddr, setServerAddr] = useState(
    `${window.location.hostname}:7000`,
  );
  const [transport, setTransport] = useState<'noise' | 'tls'>('noise');

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

  const toml = [
    `server_addr = "${serverAddr}"`,
    `protocol = "tcp"`,
    `transport = "${transport}"`,
    'multiplex = true',
    ...(clientId ? [`client_id = "${clientId}"`] : []),
    `secret = "${secret}"`,
    'log_level = "info"',
  ].join('\n');

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
      width={560}
    >
      <Typography.Paragraph type="warning">
        请立即复制密钥并配置到客户端,关闭后无法再次查看:
      </Typography.Paragraph>
      <Input readOnly value={secret} />
      <Button block icon={<CopyOutlined />} onClick={() => copyText(secret)} style={{ marginTop: 12 }} type="primary" ghost>
        复制密钥
      </Button>

      <Collapse
        ghost
        items={[
          {
            key: 'snippet',
            label: 'rh-client.toml 接入模板',
            children: (
              <>
                <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
                  <Input
                    addonBefore="server_addr"
                    onChange={(e) => setServerAddr(e.target.value)}
                    value={serverAddr}
                  />
                  <Select
                    onChange={(v) => setTransport(v)}
                    options={[
                      { label: 'noise', value: 'noise' },
                      { label: 'tls', value: 'tls' },
                    ]}
                    style={{ width: 96 }}
                    value={transport}
                  />
                </div>
                <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
                  默认地址按当前访问主机 + 控制端口 7000 推测,以实际部署为准。
                </Typography.Paragraph>
                <Input.TextArea readOnly rows={7} value={toml} />
                <Button
                  block
                  icon={<CopyOutlined />}
                  onClick={() => copyText(toml)}
                  size="small"
                  style={{ marginTop: 8 }}
                >
                  复制完整配置
                </Button>
              </>
            ),
          },
        ]}
        style={{ marginTop: 8 }}
      />
    </Modal>
  );
};

export default SecretModal;
