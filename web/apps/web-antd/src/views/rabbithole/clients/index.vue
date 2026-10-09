<script lang="ts" setup>
import type { ClientRow } from '#/api/rabbithole';

import { onMounted, ref } from 'vue';

import { Page } from '@vben/common-ui';

import {
  Button,
  Card,
  Form,
  FormItem,
  Input,
  message,
  Modal,
  Popconfirm,
  Switch,
  Table,
  Tag,
  Textarea,
} from 'ant-design-vue';

import {
  createClient,
  deleteClient,
  listClients,
  rotateClientSecret,
  updateClient,
} from '#/api/rabbithole';

const rows = ref<ClientRow[]>([]);
const loading = ref(false);
const modalOpen = ref(false);
const editId = ref<null | string>(null);
const form = ref({ name: '', remark: '' });
const secretModalOpen = ref(false);
const newSecret = ref('');

async function refresh() {
  loading.value = true;
  try {
    rows.value = await listClients();
  } finally {
    loading.value = false;
  }
}

function openCreate() {
  editId.value = null;
  form.value = { name: '', remark: '' };
  modalOpen.value = true;
}

function openEdit(row: ClientRow) {
  editId.value = row.id;
  form.value = { name: row.name, remark: row.remark };
  modalOpen.value = true;
}

async function submit() {
  if (!form.value.name) {
    message.warning('名称必填');
    return;
  }
  if (editId.value) {
    await updateClient(editId.value, form.value);
    message.success('已保存');
  } else {
    const r: any = await createClient(form.value);
    newSecret.value = r?.secret ?? '';
    secretModalOpen.value = true;
  }
  modalOpen.value = false;
  refresh();
}

async function toggle(row: ClientRow, enabled: boolean) {
  await updateClient(row.id, { enabled });
  refresh();
}

async function doRotate(row: ClientRow) {
  const r: any = await rotateClientSecret(row.id);
  newSecret.value = r?.secret ?? '';
  secretModalOpen.value = true;
}

async function doDelete(row: ClientRow) {
  await deleteClient(row.id);
  message.success('已删除');
  refresh();
}

function copySecret() {
  navigator.clipboard?.writeText(newSecret.value);
  message.success('已复制');
}

const columns = [
  { title: '名称', dataIndex: 'name', key: 'name' },
  { title: 'ID', dataIndex: 'id', key: 'id', width: 280 },
  { title: '在线', dataIndex: 'online', key: 'online', width: 80 },
  { title: '启用', dataIndex: 'enabled', key: 'enabled', width: 80 },
  { title: '版本/主机', key: 'meta', width: 200 },
  { title: '备注', dataIndex: 'remark', key: 'remark' },
  { title: '操作', key: 'action', width: 280 },
];

onMounted(refresh);
</script>

<template>
  <Page title="客户端" description="接入 rabbithole 的边缘客户端">
    <Card>
      <div style="margin-bottom: 12px">
        <Button type="primary" @click="openCreate">新建客户端</Button>
      </div>
      <Table
        :columns="columns"
        :data-source="rows"
        :loading="loading"
        row-key="id"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'online'">
            <Tag :color="record.online ? 'green' : 'default'">
              {{ record.online ? '在线' : '离线' }}
            </Tag>
          </template>
          <template v-else-if="column.key === 'enabled'">
            <Switch
              :checked="record.enabled"
              size="small"
              @change="(v: any) => toggle(record as ClientRow, !!v)"
            />
          </template>
          <template v-else-if="column.key === 'meta'">
            {{ record.client_version || '-' }} / {{ record.hostname || '-' }}
          </template>
          <template v-else-if="column.key === 'action'">
            <Button size="small" @click="openEdit(record as ClientRow)">
              编辑
            </Button>
            <Button
              size="small"
              style="margin-left: 8px"
              @click="doRotate(record as ClientRow)"
            >
              轮换密钥
            </Button>
            <Popconfirm
              title="确认删除该客户端及其全部规则?"
              @confirm="doDelete(record as ClientRow)"
            >
              <Button danger size="small" style="margin-left: 8px">删除</Button>
            </Popconfirm>
          </template>
        </template>
      </Table>
    </Card>

    <Modal
      v-model:open="modalOpen"
      :title="editId ? '编辑客户端' : '新建客户端'"
      @ok="submit"
    >
      <Form layout="vertical">
        <FormItem label="名称" required>
          <Input v-model:value="form.name" placeholder="edge-1" />
        </FormItem>
        <FormItem label="备注">
          <Textarea v-model:value="form.remark" :rows="2" />
        </FormItem>
      </Form>
    </Modal>

    <Modal
      v-model:open="secretModalOpen"
      title="客户端凭据(仅此一次展示)"
      @ok="secretModalOpen = false"
    >
      <p>请立即复制密钥并配置到客户端,关闭后无法再次查看:</p>
      <Input :value="newSecret" readonly />
      <Button style="margin-top: 8px" type="primary" @click="copySecret">
        复制
      </Button>
    </Modal>
  </Page>
</template>
