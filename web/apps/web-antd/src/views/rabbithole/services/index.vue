<script lang="ts" setup>
import type { ClientRow, ServiceRow } from '#/api/rabbithole';

import { computed, onMounted, ref } from 'vue';

import { Page } from '@vben/common-ui';

import {
  Button,
  Card,
  Checkbox,
  Form,
  FormItem,
  Input,
  InputNumber,
  message,
  Modal,
  Popconfirm,
  Select,
  Switch,
  Table,
  Tag,
} from 'ant-design-vue';

import {
  createService,
  deleteService,
  disableService,
  enableService,
  listClients,
  listServices,
  updateService,
} from '#/api/rabbithole';

const rows = ref<ServiceRow[]>([]);
const clients = ref<ClientRow[]>([]);
const loading = ref(false);
const modalOpen = ref(false);
const editId = ref<null | number>(null);

const KINDS = [
  { label: 'TCP', value: 'tcp' },
  { label: 'UDP', value: 'udp' },
  { label: 'HTTP', value: 'http' },
  { label: 'HTTPS', value: 'https' },
  { label: 'STCP', value: 'stcp' },
  { label: 'SUDP', value: 'sudp' },
  { label: 'XTCP', value: 'xtcp' },
  { label: 'TCPMux', value: 'tcpmux' },
];

const form = ref({
  client_id: '',
  name: '',
  kind: 'tcp',
  enabled: true,
  bind_ip: '0.0.0.0',
  bind_port: 0,
  target_host: '127.0.0.1',
  target_port: 80,
  custom_domains: '',
  subdomain: '',
  secret_key: '',
  use_compression: false,
});

async function refresh() {
  loading.value = true;
  try {
    [rows.value, clients.value] = await Promise.all([
      listServices(),
      listClients(),
    ]);
  } finally {
    loading.value = false;
  }
}

function openCreate() {
  editId.value = null;
  form.value = {
    client_id: clients.value[0]?.id ?? '',
    name: '',
    kind: 'tcp',
    enabled: true,
    bind_ip: '0.0.0.0',
    bind_port: 0,
    target_host: '127.0.0.1',
    target_port: 80,
    custom_domains: '',
    subdomain: '',
    secret_key: '',
    use_compression: false,
  };
  modalOpen.value = true;
}

function openEdit(row: ServiceRow) {
  editId.value = row.id;
  const s = row.spec ?? {};
  form.value = {
    client_id: row.client_id,
    name: row.name,
    kind: row.kind,
    enabled: row.enabled,
    bind_ip: row.bind_ip || '0.0.0.0',
    bind_port: row.bind_port,
    target_host: row.target_host,
    target_port: row.target_port,
    custom_domains: (
      s.http?.custom_domains ??
      s.https?.custom_domains ??
      s.tcpmux?.custom_domains ??
      []
    ).join(','),
    subdomain: s.http?.subdomain ?? s.https?.subdomain ?? '',
    secret_key:
      s.stcp?.secret_key ?? s.sudp?.secret_key ?? s.xtcp?.secret_key ?? '',
    use_compression: !!s.transport?.use_compression,
  };
  modalOpen.value = true;
}

const usesBindPort = computed(() => ['tcp', 'udp'].includes(form.value.kind));
const usesDomain = computed(() =>
  ['http', 'https', 'tcpmux'].includes(form.value.kind),
);
const usesSecret = computed(() =>
  ['stcp', 'sudp', 'xtcp'].includes(form.value.kind),
);

async function submit() {
  if (!form.value.name || !form.value.client_id) {
    message.warning('名称与所属客户端必填');
    return;
  }
  const f = form.value;
  const spec: Record<string, any> = {
    client_id: f.client_id,
    name: f.name,
    kind: f.kind,
    enabled: f.enabled,
    target_host: f.target_host,
    target_port: f.target_port,
    transport: {
      use_compression: f.use_compression,
      bandwidth_mode: 'server',
      proxy_proto: 'none',
    },
  };
  if (usesBindPort.value) {
    spec.bind_ip = f.bind_ip;
    spec.bind_port = f.bind_port;
  }
  const domains = f.custom_domains
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
  if (f.kind === 'http') {
    spec.http = { custom_domains: domains, subdomain: f.subdomain };
  } else if (f.kind === 'https') {
    spec.https = { custom_domains: domains, subdomain: f.subdomain };
  } else if (f.kind === 'tcpmux') {
    spec.tcpmux = { custom_domains: domains };
  } else if (usesSecret.value) {
    (spec as any)[f.kind] = { secret_key: f.secret_key };
  }
  try {
    if (editId.value) {
      await updateService(editId.value, spec);
      message.success('已保存');
    } else {
      await createService(spec);
      message.success('已创建');
    }
    modalOpen.value = false;
    refresh();
  } catch (error: any) {
    message.error(error?.message ?? '操作失败');
  }
}

async function toggle(row: ServiceRow, enabled: boolean) {
  await (enabled ? enableService(row.id) : disableService(row.id));
  refresh();
}

async function doDelete(row: ServiceRow) {
  await deleteService(row.id);
  message.success('已删除');
  refresh();
}

const statusColor: Record<string, string> = {
  running: 'green',
  pending: 'blue',
  offline: 'default',
  disabled: 'default',
  bind_error: 'red',
  apply_error: 'red',
  unhealthy: 'orange',
};

const columns = [
  { title: '名称', dataIndex: 'name', key: 'name' },
  { title: '类型', dataIndex: 'kind', key: 'kind', width: 80 },
  { title: '状态', dataIndex: 'status', key: 'status', width: 110 },
  { title: '监听', key: 'bind', width: 130 },
  { title: '目标', key: 'target' },
  { title: '池', dataIndex: 'pool_ready', key: 'pool', width: 60 },
  { title: '启用', dataIndex: 'enabled', key: 'enabled', width: 80 },
  { title: '操作', key: 'action', width: 220 },
];

onMounted(refresh);
</script>

<template>
  <Page
    title="转发规则"
    description="全部代理类型:TCP/UDP/HTTP/HTTPS/STCP/SUDP/XTCP/TCPMux"
  >
    <Card>
      <div style="margin-bottom: 12px">
        <Button type="primary" @click="openCreate">新建规则</Button>
      </div>
      <Table
        :columns="columns"
        :data-source="rows"
        :loading="loading"
        row-key="id"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'status'">
            <Tag :color="statusColor[record.status] ?? 'default'">
              {{ record.status }}
            </Tag>
            <span
              v-if="record.status_reason"
              style="font-size: 12px; color: #999"
            >
              {{ record.status_reason.slice(0, 24) }}
            </span>
          </template>
          <template v-else-if="column.key === 'bind'">
            {{
              record.bind_port > 0
                ? `${record.bind_ip}:${record.bind_port}`
                : '共享端口'
            }}
          </template>
          <template v-else-if="column.key === 'target'">
            {{ record.target_host }}:{{ record.target_port }}
            <Tag v-if="record.plugin" color="purple">{{ record.plugin }}</Tag>
          </template>
          <template v-else-if="column.key === 'enabled'">
            <Switch
              :checked="record.enabled"
              size="small"
              @change="(v: any) => toggle(record as ServiceRow, !!v)"
            />
          </template>
          <template v-else-if="column.key === 'action'">
            <Button size="small" @click="openEdit(record as ServiceRow)">
              编辑
            </Button>
            <Popconfirm
              title="确认删除该规则?"
              @confirm="doDelete(record as ServiceRow)"
            >
              <Button danger size="small" style="margin-left: 8px">删除</Button>
            </Popconfirm>
          </template>
        </template>
      </Table>
    </Card>

    <Modal
      v-model:open="modalOpen"
      :title="editId ? '编辑规则' : '新建规则'"
      :width="640"
      @ok="submit"
    >
      <Form layout="vertical">
        <FormItem label="所属客户端" required>
          <Select
            v-model:value="form.client_id"
            :options="clients.map((c) => ({ label: c.name, value: c.id }))"
            placeholder="选择客户端"
          />
        </FormItem>
        <FormItem label="名称" required>
          <Input v-model:value="form.name" placeholder="web" />
        </FormItem>
        <FormItem label="类型">
          <Select
            v-model:value="form.kind"
            :options="KINDS"
            :disabled="!!editId"
          />
        </FormItem>
        <template v-if="usesBindPort">
          <FormItem label="监听 IP">
            <Input v-model:value="form.bind_ip" placeholder="0.0.0.0" />
          </FormItem>
          <FormItem label="监听端口(0 = 自动分配)">
            <InputNumber v-model:value="form.bind_port" :min="0" :max="65535" />
          </FormItem>
        </template>
        <FormItem label="目标地址">
          <Input v-model:value="form.target_host" placeholder="127.0.0.1" />
        </FormItem>
        <FormItem label="目标端口">
          <InputNumber v-model:value="form.target_port" :min="1" :max="65535" />
        </FormItem>
        <template v-if="usesDomain">
          <FormItem label="自定义域名(逗号分隔)">
            <Input
              v-model:value="form.custom_domains"
              placeholder="a.example.com,b.example.com"
            />
          </FormItem>
          <FormItem v-if="form.kind !== 'tcpmux'" label="子域名">
            <Input v-model:value="form.subdomain" placeholder="app" />
          </FormItem>
        </template>
        <template v-if="usesSecret">
          <FormItem label="访问密钥">
            <Input v-model:value="form.secret_key" placeholder="secret" />
          </FormItem>
        </template>
        <FormItem label="压缩">
          <Checkbox v-model:checked="form.use_compression">
            启用 snappy 压缩
          </Checkbox>
        </FormItem>
      </Form>
    </Modal>
  </Page>
</template>
