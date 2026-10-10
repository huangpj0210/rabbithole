/**
 * URL/路由处理工具类
 */

// ==================== 查询参数处理 ====================

/**
 * 获取 URL 查询参数对象
 * @param url URL 字符串，默认使用当前页面 URL
 */
export function getQueryParams(url?: string): Record<string, string> {
  const searchStr = url ? new URL(url, window.location.origin).search : window.location.search;
  const params = new URLSearchParams(searchStr);
  const result: Record<string, string> = {};

  params.forEach((value, key) => {
    result[key] = value;
  });

  return result;
}

/**
 * 获取单个查询参数
 */
export function getQueryParam(key: string, url?: string): string | null {
  const params = getQueryParams(url);
  return params[key] ?? null;
}

/**
 * 设置查询参数到 URL
 * @param params 参数对象
 * @param url 基础 URL，默认使用当前页面 URL
 */
export function setQueryParams(params: Record<string, string | number | boolean | undefined | null>, url?: string): string {
  const baseUrl = url || window.location.href.split('?')[0];
  const currentParams = getQueryParams(url);

  // 合并参数
  Object.entries(params).forEach(([key, value]) => {
    if (value === undefined || value === null || value === '') {
      delete currentParams[key];
    } else {
      currentParams[key] = String(value);
    }
  });

  const searchStr = encodeParams(currentParams);
  return searchStr ? `${baseUrl ?? ''}?${searchStr}` : (baseUrl ?? '');
}

/**
 * 移除查询参数
 */
export function removeQueryParam(key: string | string[], url?: string): string {
  const keys = Array.isArray(key) ? key : [key];
  const params = getQueryParams(url);

  keys.forEach((k) => {
    delete params[k];
  });

  const baseUrl = (url || window.location.href).split('?')[0] ?? '';
  const searchStr = encodeParams(params);
  return searchStr ? `${baseUrl}?${searchStr}` : baseUrl;
}

// ==================== 参数编码解码 ====================

/**
 * 对象编码为查询字符串
 */
export function encodeParams(params: Record<string, string | number | boolean | undefined | null>): string {
  const searchParams = new URLSearchParams();

  Object.entries(params).forEach(([key, value]) => {
    if (value !== undefined && value !== null && value !== '') {
      searchParams.append(key, String(value));
    }
  });

  return searchParams.toString();
}

/**
 * 查询字符串解码为对象
 */
export function decodeParams(queryString: string): Record<string, string> {
  const params = new URLSearchParams(queryString);
  const result: Record<string, string> = {};

  params.forEach((value, key) => {
    result[key] = value;
  });

  return result;
}

// ==================== URL 解析 ====================

export interface ParsedUrl {
  protocol: string;
  host: string;
  hostname: string;
  port: string;
  pathname: string;
  search: string;
  hash: string;
  origin: string;
  href: string;
  params: Record<string, string>;
}

/**
 * 解析 URL
 */
export function parseUrl(url: string): ParsedUrl {
  try {
    const urlObj = new URL(url, window.location.origin);
    return {
      protocol: urlObj.protocol,
      host: urlObj.host,
      hostname: urlObj.hostname,
      port: urlObj.port,
      pathname: urlObj.pathname,
      search: urlObj.search,
      hash: urlObj.hash,
      origin: urlObj.origin,
      href: urlObj.href,
      params: getQueryParams(url),
    };
  } catch {
    return {
      protocol: '',
      host: '',
      hostname: '',
      port: '',
      pathname: url,
      search: '',
      hash: '',
      origin: '',
      href: url,
      params: {},
    };
  }
}

/**
 * 构建 URL
 * @param base 基础 URL
 * @param params 查询参数
 */
export function buildUrl(base: string, params?: Record<string, string | number | boolean | undefined | null>): string {
  if (!params || Object.keys(params).length === 0) {
    return base;
  }

  const searchStr = encodeParams(params);
  const separator = base.includes('?') ? '&' : '?';
  return searchStr ? `${base}${separator}${searchStr}` : base;
}

/**
 * 拼接 URL 路径
 */
export function joinUrl(...parts: string[]): string {
  return parts
    .map((part, index) => {
      if (index === 0) {
        return part.replace(/\/+$/, '');
      }
      return part.replace(/^\/+|\/+$/g, '');
    })
    .filter(Boolean)
    .join('/');
}

// ==================== Hash 处理 ====================

/**
 * 获取 Hash 值（不包含 #）
 */
export function getHash(): string {
  return window.location.hash.slice(1);
}

/**
 * 设置 Hash 值
 */
export function setHash(hash: string): void {
  window.location.hash = hash;
}

/**
 * Hash 转对象（支持 #key=value&key2=value2 格式）
 */
export function hashToObject(): Record<string, string> {
  const hash = getHash();
  if (!hash) return {};

  // 检查是否是路径格式还是参数格式
  if (hash.includes('=')) {
    return decodeParams(hash);
  }

  return { path: hash };
}

/**
 * 对象转 Hash
 */
export function objectToHash(obj: Record<string, string | number | boolean>): string {
  return encodeParams(obj);
}

// ==================== 路径处理 ====================

/**
 * 获取路径的文件名
 */
export function getFileName(path: string): string {
  return path.split('/').pop() || '';
}

/**
 * 获取路径的目录部分
 */
export function getDirName(path: string): string {
  const parts = path.split('/');
  parts.pop();
  return parts.join('/') || '/';
}

/**
 * 获取文件扩展名
 */
export function getExtension(path: string): string {
  const fileName = getFileName(path);
  const dotIndex = fileName.lastIndexOf('.');
  return dotIndex !== -1 ? fileName.slice(dotIndex + 1) : '';
}

/**
 * 移除扩展名
 */
export function removeExtension(path: string): string {
  const dotIndex = path.lastIndexOf('.');
  return dotIndex !== -1 ? path.slice(0, dotIndex) : path;
}

/**
 * 规范化路径
 */
export function normalizePath(path: string): string {
  return path
    .replace(/\\/g, '/')
    .replace(/\/+/g, '/')
    .replace(/\/$/, '');
}

// ==================== 判断函数 ====================

/**
 * 是否是绝对 URL
 */
export function isAbsoluteUrl(url: string): boolean {
  return /^[a-z][a-z\d+.-]*:/i.test(url);
}

/**
 * 是否是外部链接
 */
export function isExternalUrl(url: string): boolean {
  if (!isAbsoluteUrl(url)) return false;
  try {
    const urlObj = new URL(url);
    return urlObj.origin !== window.location.origin;
  } catch {
    return false;
  }
}

/**
 * 是否是相对路径
 */
export function isRelativePath(path: string): boolean {
  return !isAbsoluteUrl(path) && !path.startsWith('/');
}

/**
 * 是否是数据 URL
 */
export function isDataUrl(url: string): boolean {
  return url.startsWith('data:');
}

/**
 * 是否是 Blob URL
 */
export function isBlobUrl(url: string): boolean {
  return url.startsWith('blob:');
}

// ==================== 导航相关 ====================

/**
 * 打开新窗口
 */
export function openUrl(url: string, target: '_blank' | '_self' | '_parent' | '_top' = '_blank'): void {
  window.open(url, target);
}

/**
 * 跳转到 URL
 */
export function navigateTo(url: string): void {
  window.location.href = url;
}

/**
 * 重新加载页面
 */
export function reloadPage(forceReload = false): void {
  if (forceReload) {
    window.location.replace(window.location.href);
  } else {
    window.location.reload();
  }
}

/**
 * 返回上一页
 */
export function goBack(): void {
  window.history.back();
}

/**
 * 前进到下一页
 */
export function goForward(): void {
  window.history.forward();
}

// ==================== 下载相关 ====================

/**
 * 下载文件
 * @param url 文件 URL
 * @param fileName 文件名
 */
export function downloadFile(url: string, fileName?: string): void {
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName || getFileName(url);
  link.style.display = 'none';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
}

/**
 * 从 Blob 下载文件
 */
export function downloadBlob(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob);
  downloadFile(url, fileName);
  URL.revokeObjectURL(url);
}

/**
 * 下载文本内容
 */
export function downloadText(content: string, fileName: string, mimeType = 'text/plain'): void {
  const blob = new Blob([content], { type: mimeType });
  downloadBlob(blob, fileName);
}

/**
 * 下载 JSON
 */
export function downloadJson(data: unknown, fileName: string): void {
  const content = JSON.stringify(data, null, 2);
  downloadText(content, fileName, 'application/json');
}

// 默认导出
export default {
  getQueryParams,
  getQueryParam,
  setQueryParams,
  removeQueryParam,
  encodeParams,
  decodeParams,
  parseUrl,
  buildUrl,
  joinUrl,
  getHash,
  setHash,
  hashToObject,
  objectToHash,
  getFileName,
  getDirName,
  getExtension,
  removeExtension,
  normalizePath,
  isAbsoluteUrl,
  isExternalUrl,
  isRelativePath,
  isDataUrl,
  isBlobUrl,
  openUrl,
  navigateTo,
  reloadPage,
  goBack,
  goForward,
  downloadFile,
  downloadBlob,
  downloadText,
  downloadJson,
};
