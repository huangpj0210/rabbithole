/**
 * 认证链路(design.md §8.2 契约):
 * login → accessToken → 并行 /user/info + /auth/codes → 应用;
 * logout 必须带 Bearer(经 requestClient 实例,拦截器自动注入);
 * HTTP 401 由拦截器统一清 token 跳登录。
 */
import { httpGet, httpPost, requestClient } from './request';

export interface LoginResult {
  accessToken: string;
}

export interface UserInfo {
  userId: string;
  username: string;
  realName: string;
  avatar: string;
  homePath: string;
  roles: string[];
}

export async function loginApi(data: { password: string; username: string }): Promise<LoginResult> {
  return httpPost<LoginResult>('/auth/login', data);
}

export async function userInfoApi(): Promise<UserInfo> {
  return httpGet<UserInfo>('/user/info');
}

/** 预期返回 [](静态路由模式也会调用,必须存在)。失败不阻断登录。 */
export async function codesApi(): Promise<string[]> {
  return httpGet<string[]>('/auth/codes');
}

/** 退出:显式走带 Bearer 注入的实例(后端校验通过才落 logout 审计事件)。 */
export async function logoutApi(): Promise<void> {
  await requestClient.post('/auth/logout');
}
