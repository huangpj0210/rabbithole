import type { ClientRow, ServiceRow } from '@/api/rabbithole';
import HeaderPairsInput from '@/components/HeaderPairsInput';
import type { HeaderPairValue } from '@/components/HeaderPairsInput';
import { Checkbox, Collapse, Divider, Form, Input, InputNumber, Modal, Select } from 'antd';
import React, { useMemo, useState } from 'react';

export type ProxyKind = 'http' | 'https' | 'sudp' | 'stcp' | 'tcp' | 'tcpmux' | 'udp' | 'xtcp';

export const KIND_OPTIONS: Array<{ label: string; value: ProxyKind }> = [
  { label: 'TCP', value: 'tcp' },
  { label: 'UDP', value: 'udp' },
  { label: 'HTTP', value: 'http' },
  { label: 'HTTPS', value: 'https' },
  { label: 'STCP', value: 'stcp' },
  { label: 'SUDP', value: 'sudp' },
  { label: 'XTCP', value: 'xtcp' },
  { label: 'TCPMux', value: 'tcpmux' },
];

const PLUGIN_KINDS = [
  'none',
  'unix_domain_socket',
  'http_proxy',
  'socks5',
  'static_file',
  'https2http',
  'https2https',
  'http2https',
  'http2http',
  'tls2raw',
] as const;
type PluginKindOpt = (typeof PLUGIN_KINDS)[number];

interface FormValues {
  client_id: string;
  name: string;
  kind: ProxyKind;
  enabled: boolean;
  bind_ip: string;
  bind_auto: boolean;
  bind_port: number | null;
  target_host: string;
  target_port: number | null;
  custom_domains: string;
  subdomain: string;
  secret_key: string;
  allow_users: string;
  use_compression: boolean;
  pool_size: number | null;
  bandwidth_limit: string;
  bandwidth_mode: 'client' | 'server';
  proxy_proto: 'none' | 'v1' | 'v2';
  group: string;
  hc_enabled: boolean;
  hc_type: 'http' | 'tcp';
  hc_interval: number | null;
  hc_timeout: number | null;
  hc_max_failed: number | null;
  hc_path: string;
  locations: string;
  host_rewrite: string;
  http_user: string;
  http_password: string;
  route_by_http_user: string;
  req_headers: HeaderPairValue[];
  resp_headers: HeaderPairValue[];
  not_found_html: string;
  cache_enabled: boolean;
  cache_ttl: number | null;
  cache_max_size_mb: number | null;
  cache_extensions: string;
  cache_max_item_size_kb: number | null;
  plugin_kind: PluginKindOpt;
  plugin_unix_path: string;
  plugin_http_user: string;
  plugin_http_password: string;
  plugin_user: string;
  plugin_password: string;
  plugin_local_path: string;
  plugin_strip_prefix: string;
  plugin_local_addr: string;
  plugin_crt_path: string;
  plugin_key_path: string;
  plugin_host_rewrite: string;
  plugin_request_headers: HeaderPairValue[];
}

const KIND_SECTION: Partial<Record<ProxyKind, string>> = {
  http: 'http',
  https: 'https',
  tcpmux: 'tcpmux',
  stcp: 'stcp',
  sudp: 'sudp',
  xtcp: 'xtcp',
};

/** 域名语法(与后端一致):最左标签单个通配,如 *.example.com。 */
function parseDomains(input: string): { error?: string; list: string[] } {
  const list = input
    .split(',')
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean);
  for (const d of list) {
    if (d === '*') return { error: '通配仅支持最左标签(如 *.example.com)', list: [] };
    if (d.startsWith('*.') && d.includes('*', 2)) {
      return { error: `仅支持单个通配标签:${d}`, list: [] };
    }
    if (d.includes('*') && !d.startsWith('*.')) return { error: `通配符位置非法:${d}`, list: [] };
    if (!/^(\*\.)?([a-z0-9-]+\.)*[a-z0-9-]+$/.test(d)) return { error: `域名格式非法:${d}`, list: [] };
  }
  return { list };
}

function pairsOf(v: unknown): HeaderPairValue[] {
  const set = (v as { set?: HeaderPairValue[] } | undefined)?.set;
  return Array.isArray(set) ? set.filter((p) => p && p.name) : [];
}

function csv(v: unknown): string {
  return Array.isArray(v) ? (v as string[]).join(',') : '';
}

/** 由编辑行/默认值推导整表 initialValues(每次打开重挂载,不依赖 setFieldsValue)。 */
function initialValuesOf(
  clients: ClientRow[],
  defaultKind: ProxyKind,
  editing: null | ServiceRow,
): FormValues {
  if (!editing) {
    return {
      client_id: clients[0]?.id ?? '',
      name: '',
      kind: defaultKind,
      enabled: true,
      bind_ip: '0.0.0.0',
      bind_auto: true,
      bind_port: null,
      target_host: '127.0.0.1',
      target_port: 80,
      custom_domains: '',
      subdomain: '',
      secret_key: '',
      allow_users: '',
      use_compression: false,
      pool_size: 0,
      bandwidth_limit: '',
      bandwidth_mode: 'server',
      proxy_proto: 'none',
      group: '',
      hc_enabled: false,
      hc_type: 'tcp',
      hc_interval: 10,
      hc_timeout: 3,
      hc_max_failed: 3,
      hc_path: '',
      locations: '',
      host_rewrite: '',
      http_user: '',
      http_password: '',
      route_by_http_user: '',
      req_headers: [],
      resp_headers: [],
      not_found_html: '',
      cache_enabled: false,
      cache_ttl: 300,
      cache_max_size_mb: 16,
      cache_extensions: 'js,css,png,jpg,jpeg,gif,svg,webp,ico,woff,woff2,ttf,html',
      cache_max_item_size_kb: 1024,
      plugin_kind: 'none',
      plugin_unix_path: '',
      plugin_http_user: '',
      plugin_http_password: '',
      plugin_user: '',
      plugin_password: '',
      plugin_local_path: '',
      plugin_strip_prefix: '',
      plugin_local_addr: '',
      plugin_crt_path: '',
      plugin_key_path: '',
      plugin_host_rewrite: '',
      plugin_request_headers: [],
    };
  }
  const s = editing.spec ?? {};
  const sectionKey = KIND_SECTION[editing.kind as ProxyKind];
  const section = (sectionKey ? s[sectionKey] : {}) as Record<string, any>;
  const hc = s.health_check as Record<string, any> | undefined;
  const cache = section?.cache as Record<string, any> | undefined;
  const plugin = s.plugin as Record<string, any> | undefined;
  return {
    client_id: editing.client_id,
    name: editing.name,
    kind: editing.kind as ProxyKind,
    enabled: editing.enabled,
    bind_ip: editing.bind_ip || '0.0.0.0',
    bind_auto: false,
    bind_port: editing.bind_port,
    target_host: editing.target_host,
    target_port: editing.target_port,
    custom_domains: csv(section?.custom_domains),
    subdomain: section?.subdomain ?? '',
    secret_key: section?.secret_key ?? '',
    allow_users: csv(section?.allow_users),
    use_compression: !!s.transport?.use_compression,
    pool_size: s.pool_size ?? 0,
    bandwidth_limit: s.transport?.bandwidth_limit ?? '',
    bandwidth_mode: s.transport?.bandwidth_mode ?? 'server',
    proxy_proto: s.transport?.proxy_proto ?? 'none',
    group: s.group ?? '',
    hc_enabled: !!hc,
    hc_type: hc?.type ?? 'tcp',
    hc_interval: hc?.interval_seconds ?? 10,
    hc_timeout: hc?.timeout_seconds ?? 3,
    hc_max_failed: hc?.max_failed ?? 3,
    hc_path: hc?.path ?? '',
    locations: csv(section?.locations),
    host_rewrite: section?.host_rewrite ?? '',
    http_user: section?.http_user ?? '',
    http_password: section?.http_password ?? '',
    route_by_http_user: section?.route_by_http_user ?? '',
    req_headers: pairsOf(section?.request_headers),
    resp_headers: pairsOf(section?.response_headers),
    not_found_html: section?.not_found_html ?? '',
    cache_enabled: !!cache,
    cache_ttl: cache?.ttl_seconds ?? 300,
    cache_max_size_mb: cache?.max_size_mb ?? 16,
    cache_extensions: csv(cache?.extensions),
    cache_max_item_size_kb: cache?.max_item_size_kb ?? 1024,
    plugin_kind: (plugin?.kind ?? 'none') as PluginKindOpt,
    plugin_unix_path: plugin?.unix_path ?? '',
    plugin_http_user: plugin?.http_user ?? '',
    plugin_http_password: plugin?.http_password ?? '',
    plugin_user: plugin?.user ?? '',
    plugin_password: plugin?.password ?? '',
    plugin_local_path: plugin?.local_path ?? '',
    plugin_strip_prefix: plugin?.strip_prefix ?? '',
    plugin_local_addr: plugin?.local_addr ?? '',
    plugin_crt_path: plugin?.crt_path ?? '',
    plugin_key_path: plugin?.key_path ?? '',
    plugin_host_rewrite: plugin?.host_rewrite ?? '',
    plugin_request_headers: pairsOf(plugin?.request_headers),
  };
}

/**
 * 新建/编辑规则弹窗:字段矩阵按 design-parity §8.1 显隐。
 * 提交走 Form onFinish(onOk 触发 form.submit),编辑以原 spec 深拷贝为底本,
 * 仅覆盖表单管理的字段;kind/plugin 用本地 state 驱动条件渲染。
 */
const ProxyFormModal: React.FC<{
  clients: ClientRow[];
  defaultKind: ProxyKind;
  editing: null | ServiceRow;
  kinds: ProxyKind[];
  onCancel: () => void;
  onFinish: (spec: Record<string, any>) => Promise<void>;
  open: boolean;
  submitting: boolean;
}> = ({ open, editing, clients, kinds, defaultKind, onCancel, onFinish, submitting }) => {
  const [form] = Form.useForm<FormValues>();
  const [kind, setKind] = useState<ProxyKind>(defaultKind);
  const [pluginKind, setPluginKind] = useState<PluginKindOpt>('none');
  const [formKey, setFormKey] = useState('');
  const [wasOpen, setWasOpen] = useState(false);

  const multiKind = kinds.length > 1;
  const usesBind = kind === 'tcp' || kind === 'udp';
  const usesDomain = kind === 'http' || kind === 'https' || kind === 'tcpmux';
  const usesSecret = kind === 'stcp' || kind === 'sudp' || kind === 'xtcp';
  const supportsGroup = usesBind || kind === 'http' || kind === 'https';
  const supportsHc = supportsGroup;
  const pluginOn = pluginKind !== 'none';

  const initialValues = useMemo(
    () => initialValuesOf(clients, defaultKind, editing),
    [clients, defaultKind, editing],
  );
  // 每次打开(或打开目标变化)整表以新 key 重挂载,initialValues 全量注入,
  // 杜绝同一目标二次打开时的表单残留与 rc-* 显示缓存问题
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setFormKey(`${editing?.id ?? 'new'}-${Date.now()}`);
      setKind(editing ? (editing.kind as ProxyKind) : defaultKind);
      setPluginKind(initialValues.plugin_kind);
    }
  }

  async function handleFinish(v: FormValues) {
    const domains = parseDomains(v.custom_domains ?? '');
    if (usesDomain && domains.error) {
      form.setFields([{ errors: [domains.error], name: 'custom_domains' }]);
      return;
    }
    if ((kind === 'http' || kind === 'https') && domains.list.length === 0 && !v.subdomain?.trim()) {
      const msg = kind === 'http' ? 'HTTP 规则需填写自定义域名或子域名' : 'HTTPS 规则需填写自定义域名或子域名';
      form.setFields([{ errors: [msg], name: 'custom_domains' }]);
      return;
    }
    if (kind === 'tcpmux' && domains.list.length === 0) {
      form.setFields([{ errors: ['TCPMux 规则需填写自定义域名'], name: 'custom_domains' }]);
      return;
    }
    if (usesBind && !editing && !v.bind_auto && !v.bind_port) {
      form.setFields([{ errors: ['请填写监听端口,或改用自动分配'], name: 'bind_port' }]);
      return;
    }

    // 编辑:以原 spec 为底本,仅覆盖表单管理的字段
    const spec: Record<string, any> = editing
      ? JSON.parse(JSON.stringify(editing.spec ?? {}))
      : {};
    spec.client_id = v.client_id;
    spec.name = v.name;
    spec.kind = kind;
    spec.enabled = v.enabled;
    if (!pluginOn) {
      spec.target_host = v.target_host;
      spec.target_port = v.target_port;
    } else {
      delete spec.target_host;
      delete spec.target_port;
    }
    if (usesBind) {
      spec.bind_ip = v.bind_ip;
      // "自动分配"开关替代魔法值 0(§6.5);编辑禁用自动分配
      spec.bind_port = !editing && v.bind_auto ? 0 : (v.bind_port ?? 0);
    }
    if (kind === 'tcp') spec.pool_size = v.pool_size ?? 0;
    spec.transport = {
      ...(spec.transport ?? {}),
      use_compression: !!v.use_compression,
      bandwidth_mode: v.bandwidth_mode ?? 'server',
      proxy_proto: v.proxy_proto ?? 'none',
    };
    if (v.bandwidth_limit?.trim()) {
      spec.transport.bandwidth_limit = v.bandwidth_limit.trim();
    } else {
      delete spec.transport.bandwidth_limit;
    }
    if (supportsGroup && v.group?.trim()) {
      spec.group = v.group.trim();
    } else {
      delete spec.group;
    }
    if (v.hc_enabled && supportsHc) {
      spec.health_check = {
        type: v.hc_type,
        interval_seconds: v.hc_interval ?? 10,
        timeout_seconds: v.hc_timeout ?? 3,
        max_failed: v.hc_max_failed ?? 3,
        ...(v.hc_type === 'http' && v.hc_path ? { path: v.hc_path } : {}),
      };
    } else {
      delete spec.health_check;
    }
    if (pluginOn) {
      const p: Record<string, any> = { kind: pluginKind };
      switch (pluginKind) {
        case 'unix_domain_socket':
          p.unix_path = v.plugin_unix_path;
          break;
        case 'http_proxy':
          p.http_user = v.plugin_http_user ?? '';
          p.http_password = v.plugin_http_password ?? '';
          break;
        case 'socks5':
          p.user = v.plugin_user ?? '';
          p.password = v.plugin_password ?? '';
          break;
        case 'static_file':
          p.local_path = v.plugin_local_path;
          p.strip_prefix = v.plugin_strip_prefix ?? '';
          p.http_user = v.plugin_http_user ?? '';
          p.http_password = v.plugin_http_password ?? '';
          break;
        case 'https2http':
        case 'https2https':
          p.local_addr = v.plugin_local_addr;
          p.crt_path = v.plugin_crt_path;
          p.key_path = v.plugin_key_path;
          p.host_rewrite = v.plugin_host_rewrite ?? '';
          p.request_headers = { set: (v.plugin_request_headers ?? []).filter((r) => r.name) };
          break;
        case 'http2https':
        case 'http2http':
          p.local_addr = v.plugin_local_addr;
          p.host_rewrite = v.plugin_host_rewrite ?? '';
          p.request_headers = { set: (v.plugin_request_headers ?? []).filter((r) => r.name) };
          break;
        case 'tls2raw':
          p.local_addr = v.plugin_local_addr;
          p.crt_path = v.plugin_crt_path;
          p.key_path = v.plugin_key_path;
          break;
        default:
          break;
      }
      spec.plugin = p;
    } else {
      delete spec.plugin;
    }
    for (const k of ['http', 'https', 'tcpmux', 'stcp', 'sudp', 'xtcp'] as const) {
      if (k === kind) {
        if (k === 'http') {
          spec.http = {
            ...(spec.http ?? {}),
            custom_domains: domains.list,
            subdomain: v.subdomain?.trim() ?? '',
            locations: (v.locations ?? '').split(',').map((x) => x.trim()).filter(Boolean),
            host_rewrite: v.host_rewrite ?? '',
            http_user: v.http_user ?? '',
            http_password: v.http_password ?? '',
            route_by_http_user: v.route_by_http_user ?? '',
            request_headers: { set: (v.req_headers ?? []).filter((r) => r.name) },
            response_headers: { set: (v.resp_headers ?? []).filter((r) => r.name) },
          };
          // M2 增强字段(后端落地前 serde 忽略,属预期降级)
          if (v.not_found_html?.trim()) spec.http.not_found_html = v.not_found_html;
          if (v.cache_enabled) {
            spec.http.cache = {
              ttl_seconds: v.cache_ttl ?? 300,
              max_size_mb: v.cache_max_size_mb ?? 16,
              extensions: (v.cache_extensions ?? 'js,css').split(',').map((x) => x.trim()).filter(Boolean),
              max_item_size_kb: v.cache_max_item_size_kb ?? 1024,
            };
          }
        } else if (k === 'https') {
          spec.https = { ...(spec.https ?? {}), custom_domains: domains.list, subdomain: v.subdomain?.trim() ?? '' };
        } else if (k === 'tcpmux') {
          spec.tcpmux = { ...(spec.tcpmux ?? {}), custom_domains: domains.list };
        } else {
          spec[k] = {
            ...(spec[k] ?? {}),
            secret_key: v.secret_key ?? '',
            allow_users: (v.allow_users ?? '').split(',').map((x) => x.trim()).filter(Boolean),
          };
        }
      } else {
        delete spec[k];
      }
    }
    await onFinish(spec);
  }

  const advancedItems = useMemo(() => {
    const items = [
      {
        key: 'transport',
        label: '传输与限速',
        children: (
          <>
            <Form.Item label="压缩" name="use_compression" valuePropName="checked">
              <Checkbox>启用 snappy 压缩</Checkbox>
            </Form.Item>
            <Form.Item extra='如 "1MB" / "512KB",留空不限' label="带宽限制" name="bandwidth_limit">
              <Input placeholder="留空不限" />
            </Form.Item>
            <Form.Item label="限速位置" name="bandwidth_mode">
              <Select
                options={[
                  { label: '服务端(默认)', value: 'server' },
                  { label: '客户端', value: 'client' },
                ]}
              />
            </Form.Item>
            {(kind === 'tcp' || kind === 'http') && (
              <Form.Item label="PROXY protocol" name="proxy_proto">
                <Select
                  options={[
                    { label: '不启用', value: 'none' },
                    { label: 'v1', value: 'v1' },
                    { label: 'v2', value: 'v2' },
                  ]}
                />
              </Form.Item>
            )}
            {kind === 'tcp' && (
              <Form.Item extra="0 = 使用服务端默认" label="连接池大小" name="pool_size">
                <InputNumber max={16} min={0} precision={0} />
              </Form.Item>
            )}
          </>
        ),
      },
    ];
    if (supportsGroup) {
      items.push({
        key: 'group',
        label: '负载均衡组',
        children: (
          <Form.Item extra="组内同 (kind,bind) 规则共享监听并轮询分发" label="分组名" name="group">
            <Input placeholder="留空不分组" />
          </Form.Item>
        ),
      });
      items.push({
        key: 'hc',
        label: '健康检查',
        children: (
          <>
            <Form.Item label="启用" name="hc_enabled" valuePropName="checked">
              <Checkbox>客户端探测本地目标</Checkbox>
            </Form.Item>
            <Form.Item label="检查类型" name="hc_type">
              <Select
                options={[
                  { label: 'TCP 端口连通', value: 'tcp' },
                  { label: 'HTTP 请求', value: 'http' },
                ]}
              />
            </Form.Item>
            <Form.Item label="间隔(秒)" name="hc_interval">
              <InputNumber min={1} precision={0} />
            </Form.Item>
            <Form.Item label="超时(秒)" name="hc_timeout">
              <InputNumber max={60} min={1} precision={0} />
            </Form.Item>
            <Form.Item label="连续失败阈值" name="hc_max_failed">
              <InputNumber min={1} precision={0} />
            </Form.Item>
            <Form.Item label="HTTP 路径" name="hc_path">
              <Input placeholder="/healthz(仅 http 检查)" />
            </Form.Item>
          </>
        ),
      });
    }
    if (kind === 'http') {
      items.push({
        key: 'http-extras',
        label: 'HTTP 高级',
        children: (
          <>
            <Form.Item extra="逗号分隔,如 /api,/static;空 = 全部" label="路由路径" name="locations">
              <Input placeholder="/api,/static" />
            </Form.Item>
            <Form.Item label="Host 改写" name="host_rewrite">
              <Input placeholder="留空不改写" />
            </Form.Item>
            <Form.Item label="Basic Auth 用户" name="http_user">
              <Input placeholder="留空不启用" />
            </Form.Item>
            <Form.Item label="Basic Auth 密码" name="http_password">
              <Input placeholder="与用户名成对填写" />
            </Form.Item>
            <Form.Item label="请求头改写" name="req_headers">
              <HeaderPairsInput />
            </Form.Item>
            <Form.Item label="响应头改写" name="resp_headers">
              <HeaderPairsInput />
            </Form.Item>
            <Form.Item extra="经 PROXY protocol 传入的终端用户二级路由,留空不启用" label="按用户路由" name="route_by_http_user">
              <Input placeholder="留空不启用" />
            </Form.Item>
          </>
        ),
      });
      items.push({
        key: 'vhost-enhance',
        label: '域名增强(需服务端 M2)',
        children: (
          <>
            <Form.Item label="自定义 404 页" name="not_found_html">
              <Input.TextArea
                maxLength={8192}
                placeholder="HTML 内容,请求未命中 locations 时返回(≤8KiB)"
                rows={3}
                showCount
              />
            </Form.Item>
            <Form.Item label="静态缓存" name="cache_enabled" valuePropName="checked">
              <Checkbox>启用 HTTP 静态资源缓存</Checkbox>
            </Form.Item>
            <Form.Item label="TTL(秒)" name="cache_ttl">
              <InputNumber max={86400} min={10} precision={0} />
            </Form.Item>
            <Form.Item label="缓存容量(MB)" name="cache_max_size_mb">
              <InputNumber max={256} min={1} precision={0} />
            </Form.Item>
            <Form.Item extra="逗号分隔" label="缓存扩展名" name="cache_extensions">
              <Input placeholder="js,css,png" />
            </Form.Item>
            <Form.Item label="单条上限(KB)" name="cache_max_item_size_kb">
              <InputNumber min={1} precision={0} />
            </Form.Item>
          </>
        ),
      });
    }
    return items;
  }, [kind, supportsGroup]);

  return (
    <Modal
      cancelText="取消"
      okButtonProps={{ loading: submitting }}
      okText={editing ? '保存修改' : '创建规则'}
      onCancel={onCancel}
      onOk={() => form.submit()}
      open={open}
      styles={{ mask: { backgroundColor: 'rgba(22,35,43,0.45)' } }}
      title={editing ? `编辑规则:${editing.name}` : '新建规则'}
      width={720}
    >
      {/* key 随打开目标变化整表重挂载,以 initialValues 注入;避开 setFieldsValue/validateFields 的坑 */}
      <Form
        form={form}
        initialValues={initialValues}
        key={formKey}
        labelCol={{ span: 5 }}
        layout="horizontal"
        onFinish={handleFinish}
        wrapperCol={{ span: 18 }}
      >
        <Divider plain style={{ margin: '4px 0 16px' }} titlePlacement="left">
          连接
        </Divider>
        <Form.Item label="所属客户端" name="client_id" rules={[{ required: true, message: '请选择客户端' }]}>
          <Select
            disabled={!!editing}
            options={clients.map((c) => ({ label: c.name, value: c.id }))}
            placeholder="选择客户端"
          />
        </Form.Item>
        <Form.Item
          label="名称"
          name="name"
          rules={[
            { required: true, message: '请输入名称' },
            { pattern: /^[a-zA-Z0-9_-]{1,64}$/, message: '1-64 位字母/数字/下划线/连字符' },
          ]}
        >
          <Input placeholder="web-nginx" />
        </Form.Item>
        {multiKind && (
          <Form.Item label="类型" name="kind" rules={[{ required: true }]}>
            <Select
              disabled={!!editing}
              onChange={(v) => setKind(v as ProxyKind)}
              options={KIND_OPTIONS.filter((o) => kinds.includes(o.value))}
            />
          </Form.Item>
        )}
        {usesBind && (
          <>
            <Form.Item label="监听 IP" name="bind_ip">
              <Input placeholder="0.0.0.0" />
            </Form.Item>
            {editing ? (
              <Form.Item label="监听端口" name="bind_port" rules={[{ required: true, message: '请输入监听端口' }]}>
                <InputNumber className="rh-mono" max={65535} min={1} precision={0} />
              </Form.Item>
            ) : (
              <>
                <Form.Item extra="开启后由服务端在允许范围内挑选空闲端口" label="自动分配" name="bind_auto" valuePropName="checked">
                  <Checkbox>自动分配监听端口</Checkbox>
                </Form.Item>
                <Form.Item noStyle shouldUpdate={(a, b) => a.bind_auto !== b.bind_auto}>
                  {({ getFieldValue }) =>
                    !getFieldValue('bind_auto') ? (
                      <Form.Item label="监听端口" name="bind_port" rules={[{ required: true, message: '请输入监听端口' }]}>
                        <InputNumber className="rh-mono" max={65535} min={1} precision={0} />
                      </Form.Item>
                    ) : null
                  }
                </Form.Item>
              </>
            )}
          </>
        )}
        {!pluginOn && (
          <>
            <Divider plain style={{ margin: '4px 0 16px' }} titlePlacement="left">
              目标(内网服务)
            </Divider>
            <Form.Item label="目标地址" name="target_host" rules={[{ required: true, message: '请输入目标地址' }]}>
              <Input placeholder="127.0.0.1" />
            </Form.Item>
            <Form.Item label="目标端口" name="target_port" rules={[{ required: true, message: '请输入目标端口' }]}>
              <InputNumber max={65535} min={1} precision={0} />
            </Form.Item>
          </>
        )}
        {usesDomain && (
          <>
            <Form.Item extra="逗号分隔,支持 *.example.com 单层通配" label="自定义域名" name="custom_domains">
              <Input placeholder="a.example.com,*.example.com" />
            </Form.Item>
            {kind !== 'tcpmux' && (
              <Form.Item extra="拼接 vhost.subdomain_host" label="子域名" name="subdomain">
                <Input placeholder="app" />
              </Form.Item>
            )}
          </>
        )}
        {usesSecret && (
          <>
            <Form.Item label="访问密钥" name="secret_key">
              <Input placeholder="与访问器一致才允许中继" />
            </Form.Item>
            <Form.Item extra="逗号分隔的客户端名;空 = 任意" label="允许访问方" name="allow_users">
              <Input placeholder="visitor-a,visitor-b" />
            </Form.Item>
          </>
        )}
        <Divider plain style={{ margin: '4px 0 16px' }} titlePlacement="left">
          行为
        </Divider>
        <Form.Item label="启用" name="enabled" valuePropName="checked">
          <Checkbox>创建后立即启用</Checkbox>
        </Form.Item>

        <Form.Item label="插件" name="plugin_kind">
          <Select
            onChange={(v) => setPluginKind(v as PluginKindOpt)}
            options={PLUGIN_KINDS.map((k) => ({
              label: k === 'none' ? '不启用(直连目标)' : k,
              value: k,
            }))}
          />
        </Form.Item>
        {pluginOn && (
          <Form.Item label="插件参数" style={{ marginBottom: 0 }}>
            <Collapse
              defaultActiveKey={['plugin']}
              ghost
              items={[
                {
                  key: 'plugin',
                  label: '展开配置',
                  children: <PluginFields kind={pluginKind} />,
                },
              ]}
            />
          </Form.Item>
        )}

        <Form.Item label="高级" style={{ marginTop: 8 }}>
          <Collapse ghost items={advancedItems} />
        </Form.Item>
      </Form>
    </Modal>
  );
};

/** 按插件类型渲染参数子表单(flat plugin_* 字段)。 */
function PluginFields({ kind }: { kind: PluginKindOpt }) {
  const f = (name: keyof FormValues, label: string, required = false, placeholder = '') => (
    <Form.Item
      key={String(name)}
      label={label}
      name={name}
      rules={required ? [{ required: true, message: `请输入${label}` }] : undefined}
    >
      <Input placeholder={placeholder} />
    </Form.Item>
  );
  switch (kind) {
    case 'unix_domain_socket':
      return f('plugin_unix_path', 'Unix socket 路径', true, '/var/run/app.sock');
    case 'http_proxy':
      return (
        <>
          {f('plugin_http_user', '代理用户名')}
          {f('plugin_http_password', '代理密码')}
        </>
      );
    case 'socks5':
      return (
        <>
          {f('plugin_user', '用户名')}
          {f('plugin_password', '密码')}
        </>
      );
    case 'static_file':
      return (
        <>
          {f('plugin_local_path', '本地目录', true, '/var/www')}
          {f('plugin_strip_prefix', '剥离前缀', false, '/static')}
          {f('plugin_http_user', 'Basic Auth 用户')}
          {f('plugin_http_password', 'Basic Auth 密码')}
        </>
      );
    case 'https2http':
    case 'https2https':
      return (
        <>
          {f('plugin_local_addr', '本地地址', true, '127.0.0.1:8080')}
          {f('plugin_crt_path', '证书路径', true, '/path/tls.crt')}
          {f('plugin_key_path', '私钥路径', true, '/path/tls.key')}
          {f('plugin_host_rewrite', 'Host 改写')}
          <Form.Item label="请求头改写" name="plugin_request_headers">
            <HeaderPairsInput />
          </Form.Item>
        </>
      );
    case 'http2https':
    case 'http2http':
      return (
        <>
          {f('plugin_local_addr', '本地地址', true, '127.0.0.1:8080')}
          {f('plugin_host_rewrite', 'Host 改写')}
          <Form.Item label="请求头改写" name="plugin_request_headers">
            <HeaderPairsInput />
          </Form.Item>
        </>
      );
    case 'tls2raw':
      return (
        <>
          {f('plugin_local_addr', '上游地址', true, '127.0.0.1:9000')}
          {f('plugin_crt_path', '证书路径', true, '/path/tls.crt')}
          {f('plugin_key_path', '私钥路径', true, '/path/tls.key')}
        </>
      );
    default:
      return null;
  }
}

export default ProxyFormModal;
