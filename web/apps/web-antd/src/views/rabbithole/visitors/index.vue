<script lang="ts" setup>
import type { ClientRow, VisitorRow } from '#/api/rabbithole';

import { onMounted, ref } from 'vue';

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
  createVisitor,
  deleteVisitor,
  disableVisitor,
  enableVisitor,
  listClients,
  listVisitors,
  updateVisitor,
} from '#/api/rabbithole';

const rows = ref<VisitorRow[]>([]);
const clients = ref<ClientRow[]>([]);
const loading = ref(false);
const modalOpen = ref(false);
const editId = ref<null | number>(null);

const form = ref({
  client_id: '',
  name: '',
  kind: 'stcp',
  enabled: true,
  bind_addr: '127.0.0.1',
  bind_port: 6000,
  target_proxy_name: '',
  secret_key: '',
  use_compression: false,
});

async function refresh() {
  loading.value = true;
  try {
    [rows.value, clients.value] = await Promise.all([
      listVisitors(),
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
    kind: 'stcp',
    enabled: true,
    bind_addr: '127.0.0.1',
    bind_port: 6000,
    target_proxy_name: '',
    secret_key: '',
    use_compression: false,
  };
  modalOpen.value = true;
}

function openEdit(row: VisitorRow) {
  editId.value = row.id;
  form.value = {
    client_id: row.client_id,
    name: row.name,
    kind: row.kind,
    enabled: row.enabled,
    bind_addr: row.bind_addr,
    bind_port: row.bind_port,
    target_proxy_name: row.target_proxy_name,
    secret_key: '',
    use_compression: row.use_compression,
  };
  modalOpen.value = true;
}

async function submit() {
  if (
    !form.value.name ||
    !form.value.client_id ||
    !form.value.target_proxy_name
  ) {
    message.warning('名称、客户端与目标代理必填');
    return;
  }
  const spec: Record<string, any> = { ...form.value };
  if (editId.value && !spec.secret_key) delete spec.secret_key;
  try {
    if (editId.value) {
      await updateVisitor(editId.value, spec);
      message.success('已保存');
    } else {
      await createVisitor(spec);
      message.success('已创建');
    }
    modalOpen.value = false;
    refresh();
  } catch (error: any) {
    message.error(error?.message ?? '操作失败');
  }
}

async function toggle(row: VisitorRow, enabled: boolean) {
  await (enabled ? enableVisitor(row.id) : disableVisitor(row.id));
  refresh();
}

async function doDelete(row: VisitorRow) {
  await deleteVisitor(row.id);
  message.success('已删除');
  refresh();
}

const columns = [
  { title: '名称', dataIndex: 'name', key: 'name' },
  { title: '类型', dataIndex: 'kind', key: 'kind', width: 80 },
  { title: '本地监听', key: 'bind', width: 150 },
  { title: '目标代理', dataIndex: 'target_proxy_name', key: 'target' },
  { title: '压缩', key: 'compress', width: 70 },
  { title: '启用', dataIndex: 'enabled', key: 'enabled', width: 80 },
  { title: '操作', key: 'action', width: 220 },
];

onMounted(refresh);
</script>

<template>
  <Page title="访问器" description="STCP/SUDP/XTCP 的访问端规则">
    <Card>
      <div style="margin-bottom: 12px">
        <Button type="primary" @click="openCreate">新建访问器</Button>
      </div>
      <Table
        :columns="columns"
        :data-source="rows"
        :loading="loading"
        row-key="id"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'bind'">
            {{ record.bind_addr }}:{{ record.bind_port }}
          </template>
          <template v-else-if="column.key === 'compress'">
            <Tag :color="record.use_compression ? 'green' : 'default'">
              {{ record.use_compression ? '是' : '否' }}
            </Tag>
          </template>
          <template v-else-if="column.key === 'enabled'">
            <Switch
              :checked="record.enabled"
              size="small"
              @change="(v: any) => toggle(record as VisitorRow, !!v)"
            />
          </template>
          <template v-else-if="column.key === 'action'">
            <Button size="small" @click="openEdit(record as VisitorRow)">
              编辑
            </Button>
            <Popconfirm
              title="确认删除该访问器?"
              @confirm="doDelete(record as VisitorRow)"
            >
              <Button danger size="small" style="margin-left: 8px">删除</Button>
            </Popconfirm>
          </template>
        </template>
      </Table>
    </Card>

    <Modal
      v-model:open="modalOpen"
      :title="editId ? '编辑访问器' : '新建访问器'"
      @ok="submit"
    >
      <Form layout="vertical">
        <FormItem label="所属客户端" required>
          <Select
            v-model:value="form.client_id"
            :options="clients.map((c) => ({ label: c.name, value: c.id }))"
          />
        </FormItem>
        <FormItem label="名称" required>
          <Input v-model:value="form.name" />
        </FormItem>
        <FormItem label="类型">
          <Select
            v-model:value="form.kind"
            :disabled="!!editId"
            :options="[
              { label: 'STCP', value: 'stcp' },
              { label: 'SUDP', value: 'sudp' },
              { label: 'XTCP', value: 'xtcp' },
            ]"
          />
        </FormItem>
        <FormItem label="本地监听地址">
          <Input v-model:value="form.bind_addr" placeholder="127.0.0.1" />
        </FormItem>
        <FormItem label="本地监听端口">
          <InputNumber v-model:value="form.bind_port" :min="1" :max="65535" />
        </FormItem>
        <FormItem label="目标代理名" required>
          <Input v-model:value="form.target_proxy_name" placeholder="sdb" />
        </FormItem>
        <FormItem :label="editId ? '访问密钥(留空保持不变)' : '访问密钥'">
          <Input v-model:value="form.secret_key" />
        </FormItem>
        <FormItem label="压缩">
          <Checkbox v-model:checked="form.use_compression">
            启用 snappy 压缩
          </Checkbox>
        </FormItem>
      </Form>
    </Modal>
  </Page>
</template>
