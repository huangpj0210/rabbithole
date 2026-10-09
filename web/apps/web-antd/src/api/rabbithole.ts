/**
 * rabbithole 管理面业务 API(后端 envelope: {code:0,data,message})。
 */
import { requestClient } from '#/api/request';

export interface ClientRow {
  id: string;
  name: string;
  enabled: boolean;
  online: boolean;
  remark: string;
  created_at: number;
  last_online_at: null | number;
  client_version: string;
  hostname: string;
  os: string;
  expected_revision: number;
}

export interface ServiceRow {
  id: number;
  client_id: string;
  name: string;
  kind: string;
  enabled: boolean;
  bind_ip: string;
  bind_port: number;
  target_host: string;
  target_port: number;
  group: null | string;
  status: string;
  status_reason: string;
  healthy: boolean;
  pool_ready: number;
  plugin: null | string;
  spec: Record<string, any>;
}

export interface VisitorRow {
  id: number;
  client_id: string;
  name: string;
  kind: string;
  enabled: boolean;
  bind_addr: string;
  bind_port: number;
  target_proxy_name: string;
  use_compression: boolean;
  status: string;
}

export interface StatsOverview {
  clients_total: number;
  clients_online: number;
  services_total: number;
  services_running: number;
  conns_current: number;
  bytes_in: number;
  bytes_out: number;
}

export interface EventRow {
  id: number;
  ts: number;
  level: string;
  source: string;
  message: string;
  detail: string;
}

// ---------- clients ----------
export async function listClients() {
  return requestClient.get<ClientRow[]>('/clients');
}
export async function createClient(data: { name: string; remark?: string }) {
  return requestClient.post('/clients', data);
}
export async function updateClient(
  id: string,
  data: { name?: string; remark?: string; enabled?: boolean },
) {
  return requestClient.put(`/clients/${id}`, data);
}
export async function deleteClient(id: string) {
  return requestClient.delete(`/clients/${id}`);
}
export async function rotateClientSecret(id: string) {
  return requestClient.post<{ secret: string }>(`/clients/${id}/rotate-secret`);
}

// ---------- services ----------
export async function listServices(params?: { client_id?: string }) {
  return requestClient.get<ServiceRow[]>('/services', { params });
}
export async function createService(spec: Record<string, any>) {
  return requestClient.post('/services', spec);
}
export async function updateService(id: number, spec: Record<string, any>) {
  return requestClient.put(`/services/${id}`, spec);
}
export async function deleteService(id: number) {
  return requestClient.delete(`/services/${id}`);
}
export async function enableService(id: number) {
  return requestClient.post(`/services/${id}/enable`);
}
export async function disableService(id: number) {
  return requestClient.post(`/services/${id}/disable`);
}

// ---------- visitors ----------
export async function listVisitors(params?: { client_id?: string }) {
  return requestClient.get<VisitorRow[]>('/visitors', { params });
}
export async function createVisitor(spec: Record<string, any>) {
  return requestClient.post('/visitors', spec);
}
export async function updateVisitor(id: number, spec: Record<string, any>) {
  return requestClient.put(`/visitors/${id}`, spec);
}
export async function deleteVisitor(id: number) {
  return requestClient.delete(`/visitors/${id}`);
}
export async function enableVisitor(id: number) {
  return requestClient.post(`/visitors/${id}/enable`);
}
export async function disableVisitor(id: number) {
  return requestClient.post(`/visitors/${id}/disable`);
}

// ---------- stats / events ----------
export async function statsOverview() {
  return requestClient.get<StatsOverview>('/stats/overview');
}
export async function listEvents(params?: { limit?: number; level?: string }) {
  return requestClient.get<EventRow[]>('/events', { params });
}

/** 修改自身密码:成功后所有会话失效(后端轮换 JWT 密钥),需重新登录。 */
export async function changePassword(oldPassword: string, newPassword: string) {
  return requestClient.post('/auth/password', {
    new_password: newPassword,
    old_password: oldPassword,
  });
}
