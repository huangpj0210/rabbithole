<script lang="ts" setup>
import type { EventRow, StatsOverview } from '#/api/rabbithole';

import { onMounted, onUnmounted, ref } from 'vue';

import { Page } from '@vben/common-ui';

import { Card, Col, Row, Statistic, Table, Tag } from 'ant-design-vue';

import { listEvents, statsOverview } from '#/api/rabbithole';

const stats = ref<null | StatsOverview>(null);
const events = ref<EventRow[]>([]);
let timer: any = null;

async function refresh() {
  try {
    [stats.value, events.value] = await Promise.all([
      statsOverview(),
      listEvents({ limit: 20 }),
    ]);
  } catch {
    /* 网络错误保持原值 */
  }
}

function fmtBytes(n: number) {
  if (n > 1024 * 1024 * 1024) return `${(n / 1024 ** 3).toFixed(2)} GB`;
  if (n > 1024 * 1024) return `${(n / 1024 ** 2).toFixed(2)} MB`;
  if (n > 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

function fmtTime(ts: number) {
  return new Date(ts).toLocaleString();
}

onMounted(() => {
  refresh();
  timer = setInterval(refresh, 5000);
});
onUnmounted(() => clearInterval(timer));

const eventColumns = [
  { title: '时间', dataIndex: 'ts', key: 'ts', width: 180 },
  { title: '级别', dataIndex: 'level', key: 'level', width: 80 },
  { title: '来源', dataIndex: 'source', key: 'source', width: 100 },
  { title: '消息', dataIndex: 'message', key: 'message' },
];
</script>

<template>
  <Page title="总览" description="rabbithole 服务端运行状态">
    <Row :gutter="16">
      <Col :span="6">
        <Card>
          <Statistic title="客户端" :value="stats?.clients_total ?? 0" />
        </Card>
      </Col>
      <Col :span="6">
        <Card>
          <Statistic
            title="在线客户端"
            :value="stats?.clients_online ?? 0"
            :value-style="{
              color: stats?.clients_online ? '#3f8600' : '#cf1322',
            }"
          />
        </Card>
      </Col>
      <Col :span="6">
        <Card>
          <Statistic title="运行中服务" :value="stats?.services_running ?? 0" />
        </Card>
      </Col>
      <Col :span="6">
        <Card>
          <Statistic title="当前连接" :value="stats?.conns_current ?? 0" />
        </Card>
      </Col>
    </Row>
    <Row :gutter="16" style="margin-top: 16px">
      <Col :span="12">
        <Card>
          <Statistic title="入站流量" :value="fmtBytes(stats?.bytes_in ?? 0)" />
        </Card>
      </Col>
      <Col :span="12">
        <Card>
          <Statistic
            title="出站流量"
            :value="fmtBytes(stats?.bytes_out ?? 0)"
          />
        </Card>
      </Col>
    </Row>
    <Card title="最近事件" style="margin-top: 16px">
      <Table
        :columns="eventColumns"
        :data-source="events"
        :pagination="{ pageSize: 10 }"
        row-key="id"
        size="small"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'ts'">
            {{ fmtTime(record.ts) }}
          </template>
          <template v-else-if="column.key === 'level'">
            <Tag
              :color="
                record.level === 'warn'
                  ? 'orange'
                  : record.level === 'error'
                    ? 'red'
                    : 'blue'
              "
            >
              {{ record.level }}
            </Tag>
          </template>
        </template>
      </Table>
    </Card>
  </Page>
</template>
