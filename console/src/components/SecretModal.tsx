import { CopyOutlined } from '@ant-design/icons';
import { App, Button, Input, Modal, Typography } from 'antd';
import React from 'react';

/**
 * 一次性密钥展示弹窗(新建/轮换后的 secret 仅此一次返回)。
 * R3 会在其中追加 rh-client.toml 模板折叠区。
 */
const SecretModal: React.FC<{
  onCancel: () => void;
  open: boolean;
  secret: string;
  title?: string;
}> = ({ open, secret, onCancel, title = '客户端凭据(仅此一次展示)' }) => {
  const { message } = App.useApp();

  async function copySecret() {
    // 自托管常见 plain HTTP 访问场景下 navigator.clipboard 不存在,
    // 需降级 execCommand;复制失败必须如实提示(密钥仅此一次展示)
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(secret);
        message.success('已复制');
        return;
      }
      const ta = document.createElement('textarea');
      ta.value = secret;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.append(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      if (ok) {
        message.success('已复制');
      } else {
        message.warning('复制失败,请手动选中密钥复制');
      }
    } catch {
      message.warning('复制失败,请手动选中密钥复制');
    }
  }

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
    >
      <Typography.Paragraph type="warning">
        请立即复制密钥并配置到客户端,关闭后无法再次查看:
      </Typography.Paragraph>
      <Input readOnly value={secret} />
      <Button block icon={<CopyOutlined />} onClick={copySecret} style={{ marginTop: 12 }} type="primary" ghost>
        复制密钥
      </Button>
    </Modal>
  );
};

export default SecretModal;
