import { DeleteOutlined, PlusOutlined } from '@ant-design/icons';
import { Button, Input, Space } from 'antd';
import React from 'react';

export interface HeaderPairValue {
  name: string;
  value: string;
}

/**
 * HTTP 头键值对行编辑器(增/改/删行),作为受控组件用于 Form.Item,
 * 提交时由调用方组装为后端 HeaderOp.set 结构(空 name 行过滤)。
 */
const HeaderPairsInput: React.FC<{
  onChange?: (v: HeaderPairValue[]) => void;
  value?: HeaderPairValue[];
}> = ({ value = [], onChange }) => {
  const rows = value ?? [];

  function setRow(i: number, patch: Partial<HeaderPairValue>) {
    const next = rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r));
    onChange?.(next);
  }

  return (
    <div>
      {rows.map((row, i) => (
        <Space key={i} style={{ display: 'flex', marginBottom: 4 }} align="baseline">
          <Input
            onChange={(e) => setRow(i, { name: e.target.value })}
            placeholder="Header 名"
            style={{ width: 160 }}
            value={row.name}
          />
          <Input
            onChange={(e) => setRow(i, { value: e.target.value })}
            placeholder="值"
            style={{ width: 240 }}
            value={row.value}
          />
          <Button
            aria-label={`删除第 ${i + 1} 行 Header`}
            icon={<DeleteOutlined />}
            onClick={() => onChange?.(rows.filter((_, idx) => idx !== i))}
            size="small"
            type="text"
          />
        </Space>
      ))}
      <Button
        block
        icon={<PlusOutlined />}
        onClick={() => onChange?.([...rows, { name: '', value: '' }])}
        size="small"
        type="dashed"
      >
        添加 Header
      </Button>
    </div>
  );
};

export default HeaderPairsInput;
