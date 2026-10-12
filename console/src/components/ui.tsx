import { InfoCircleOutlined } from '@ant-design/icons';
import { theme, Tooltip, Typography } from 'antd';
import React from 'react';

/**
 * 设计系统共享件(console-ui-redesign §4.7)。
 * 状态徽章:8px 信号点 + 文字,四态语义(在线/离线/警告/未启用),禁止大色块底。
 */

export type RhSignal = 'ok' | 'off' | 'warn' | 'idle' | 'error';

const SIGNAL_COLOR: Record<RhSignal, string> = {
  ok: '#52c41a',
  off: '#d9d9d9',
  warn: '#faad14',
  idle: '#bfbfbf',
  error: '#ff4d4f',
};

export const StatusBadge: React.FC<{ signal: RhSignal; text?: React.ReactNode }> = ({
  signal,
  text,
}) => (
  <span
    style={{
      display: 'inline-flex',
      alignItems: 'center',
      gap: 6,
      whiteSpace: 'nowrap',
    }}
  >
    <span
      aria-hidden
      style={{
        width: 8,
        height: 8,
        borderRadius: '50%',
        background: SIGNAL_COLOR[signal],
        display: 'inline-block',
        flex: '0 0 auto',
      }}
    />
    <span>{text}</span>
  </span>
);

/** 服务规则状态 → 徽章信号(七态收敛为四态语义)。 */
export function proxyStatusSignal(status: string, healthy?: boolean): {
  signal: RhSignal;
  text: string;
} {
  if (!healthy && status === 'running') {
    return { signal: 'warn', text: 'running(不健康)' };
  }
  switch (status) {
    case 'running':
      return { signal: 'ok', text: 'running' };
    case 'pending':
      return { signal: 'warn', text: 'pending' };
    case 'bind_error':
      return { signal: 'error', text: 'bind_error' };
    case 'apply_error':
      return { signal: 'error', text: 'apply_error' };
    case 'offline':
      return { signal: 'off', text: 'offline' };
    default:
      return { signal: 'idle', text: '未启用' };
  }
}

/**
 * mono 展示 + 截断 + 一键复制(§4.7 复制交互)。
 * prefix: 前置固定部分(如 UUID 前 8 位折叠展示时用);full: 复制的完整值。
 */
export const MonoCopy: React.FC<{
  display?: React.ReactNode;
  full: string;
  ariaLabel?: string;
}> = ({ display, full, ariaLabel }) => (
  <Typography.Text
    className="rh-mono"
    copyable={{
      text: full,
      tooltips: ['复制', '已复制'],
    }}
    style={{ fontSize: 13 }}
    tabIndex={0}
    aria-label={ariaLabel ?? `复制 ${full}`}
  >
    {display ?? full}
  </Typography.Text>
);

/** 纯 mono 展示(端口/速率/时间戳等,§4.3)。 */
export const Mono: React.FC<{ children: React.ReactNode; style?: React.CSSProperties }> = ({
  children,
  style,
}) => (
  <span className="rh-mono" style={{ fontSize: 13, ...style }}>
    {children}
  </span>
);

/**
 * 带说明的名词(§7 文案规范升级):名词 + ℹ 图标,悬停或点按弹出该项的功能说明,
 * 替代"括号补充说明"式文案——说明内容不在界面常驻,界面只留名词本身。
 */
export const InfoLabel: React.FC<{ info: React.ReactNode; title: React.ReactNode }> = ({
  title,
  info,
}) => {
  // 图标颜色走 token:暗色主题下固定黑色透明度会不可读
  const { token } = theme.useToken();
  return (
    <span>
      {title}
      <Tooltip title={info} trigger={['hover', 'click']}>
        <InfoCircleOutlined
          aria-label={typeof title === 'string' ? `${title} 说明` : '说明'}
          style={{ color: token.colorTextTertiary, cursor: 'help', marginInlineStart: 4 }}
        />
      </Tooltip>
    </span>
  );
};
