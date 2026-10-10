/**
 * 字符串处理工具类
 */

// ==================== 空格处理 ====================

/**
 * 去除两端空格
 */
export function trim(str: string): string {
  if (!str) return '';
  return str.trim();
}

/**
 * 去除左侧空格
 */
export function trimStart(str: string): string {
  if (!str) return '';
  return str.trimStart();
}

/**
 * 去除右侧空格
 */
export function trimEnd(str: string): string {
  if (!str) return '';
  return str.trimEnd();
}

/**
 * 去除所有空格
 */
export function trimAll(str: string): string {
  if (!str) return '';
  return str.replace(/\s+/g, '');
}

// ==================== 大小写转换 ====================

/**
 * 首字母大写
 */
export function capitalize(str: string): string {
  if (!str) return '';
  return str.charAt(0).toUpperCase() + str.slice(1).toLowerCase();
}

/**
 * 每个单词首字母大写
 */
export function capitalizeWords(str: string): string {
  if (!str) return '';
  return str.replace(/\b\w/g, (char) => char.toUpperCase());
}

/**
 * 转换为大写
 */
export function toUpperCase(str: string): string {
  if (!str) return '';
  return str.toUpperCase();
}

/**
 * 转换为小写
 */
export function toLowerCase(str: string): string {
  if (!str) return '';
  return str.toLowerCase();
}

// ==================== 命名转换 ====================

/**
 * 转换为驼峰命名 (camelCase)
 */
export function camelCase(str: string): string {
  if (!str) return '';
  return str
    .replace(/[-_\s]+(.)?/g, (_, char) => (char ? char.toUpperCase() : ''))
    .replace(/^./, (char) => char.toLowerCase());
}

/**
 * 转换为帕斯卡命名 (PascalCase)
 */
export function pascalCase(str: string): string {
  if (!str) return '';
  return str
    .replace(/[-_\s]+(.)?/g, (_, char) => (char ? char.toUpperCase() : ''))
    .replace(/^./, (char) => char.toUpperCase());
}

/**
 * 转换为下划线命名 (snake_case)
 */
export function snakeCase(str: string): string {
  if (!str) return '';
  return str
    .replace(/([A-Z])/g, '_$1')
    .replace(/[-\s]+/g, '_')
    .replace(/^_/, '')
    .toLowerCase();
}

/**
 * 转换为大写下划线命名 (SCREAMING_SNAKE_CASE)
 */
export function screamingSnakeCase(str: string): string {
  return snakeCase(str).toUpperCase();
}

/**
 * 转换为短横线命名 (kebab-case)
 */
export function kebabCase(str: string): string {
  if (!str) return '';
  return str
    .replace(/([A-Z])/g, '-$1')
    .replace(/[_\s]+/g, '-')
    .replace(/^-/, '')
    .toLowerCase();
}

// ==================== 截断与填充 ====================

/**
 * 截断字符串
 * @param str 字符串
 * @param length 最大长度
 * @param suffix 后缀，默认 '...'
 */
export function truncate(str: string, length: number, suffix = '...'): string {
  if (!str) return '';
  if (str.length <= length) return str;
  return str.slice(0, length - suffix.length) + suffix;
}

/**
 * 截断字符串（按字符宽度，中文算2个字符）
 */
export function truncateByWidth(str: string, maxWidth: number, suffix = '...'): string {
  if (!str) return '';
  let width = 0;
  let i = 0;
  for (; i < str.length; i++) {
    const charWidth = str.charCodeAt(i) > 127 ? 2 : 1;
    if (width + charWidth > maxWidth - suffix.length) break;
    width += charWidth;
  }
  if (i >= str.length) return str;
  return str.slice(0, i) + suffix;
}

/**
 * 在字符串开头填充
 */
export function padStart(str: string, length: number, padStr = ' '): string {
  if (!str) str = '';
  return String(str).padStart(length, padStr);
}

/**
 * 在字符串末尾填充
 */
export function padEnd(str: string, length: number, padStr = ' '): string {
  if (!str) str = '';
  return String(str).padEnd(length, padStr);
}

/**
 * 重复字符串
 */
export function repeat(str: string, count: number): string {
  if (!str || count <= 0) return '';
  return str.repeat(count);
}

// ==================== 模板与替换 ====================

/**
 * 模板替换
 * @param template 模板字符串，如 "Hello, {name}!"
 * @param data 数据对象
 */
export function template(templateStr: string, data: Record<string, unknown>): string {
  if (!templateStr) return '';
  return templateStr.replace(/\{(\w+)\}/g, (_, key) => {
    return data[key] !== undefined ? String(data[key]) : `{${key}}`;
  });
}

/**
 * 替换所有匹配项
 */
export function replaceAll(str: string, search: string, replacement: string): string {
  if (!str) return '';
  return str.split(search).join(replacement);
}

/**
 * 移除指定字符串
 */
export function remove(str: string, search: string): string {
  return replaceAll(str, search, '');
}

// ==================== 分割与连接 ====================

/**
 * 分割字符串
 */
export function split(str: string, separator: string | RegExp, limit?: number): string[] {
  if (!str) return [];
  return str.split(separator, limit);
}

/**
 * 连接字符串数组
 */
export function join(arr: string[], separator = ''): string {
  if (!arr || !arr.length) return '';
  return arr.join(separator);
}

/**
 * 按行分割
 */
export function splitLines(str: string): string[] {
  if (!str) return [];
  return str.split(/\r?\n/);
}

// ==================== 查找与判断 ====================

/**
 * 是否包含子字符串
 */
export function contains(str: string, search: string, ignoreCase = false): boolean {
  if (!str) return false;
  if (ignoreCase) {
    return str.toLowerCase().includes(search.toLowerCase());
  }
  return str.includes(search);
}

/**
 * 是否以指定字符串开头
 */
export function startsWith(str: string, search: string, ignoreCase = false): boolean {
  if (!str) return false;
  if (ignoreCase) {
    return str.toLowerCase().startsWith(search.toLowerCase());
  }
  return str.startsWith(search);
}

/**
 * 是否以指定字符串结尾
 */
export function endsWith(str: string, search: string, ignoreCase = false): boolean {
  if (!str) return false;
  if (ignoreCase) {
    return str.toLowerCase().endsWith(search.toLowerCase());
  }
  return str.endsWith(search);
}

/**
 * 是否为空字符串
 */
export function isEmpty(str: string | null | undefined): boolean {
  return str === null || str === undefined || str === '';
}

/**
 * 是否为空白字符串（空或只有空格）
 */
export function isBlank(str: string | null | undefined): boolean {
  return isEmpty(str) || trim(str || '') === '';
}

/**
 * 统计子字符串出现次数
 */
export function countOccurrences(str: string, search: string): number {
  if (!str || !search) return 0;
  return str.split(search).length - 1;
}

// ==================== 特殊处理 ====================

/**
 * 反转字符串
 */
export function reverse(str: string): string {
  if (!str) return '';
  return str.split('').reverse().join('');
}

/**
 * 获取字符串字节长度（中文算2字节）
 */
export function byteLength(str: string): number {
  if (!str) return 0;
  let length = 0;
  for (let i = 0; i < str.length; i++) {
    length += str.charCodeAt(i) > 127 ? 2 : 1;
  }
  return length;
}

/**
 * 生成随机字符串
 */
export function randomString(length = 8, chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'): string {
  let result = '';
  for (let i = 0; i < length; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return result;
}

/**
 * 生成 UUID
 */
export function uuid(): string {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

/**
 * 转义 HTML 特殊字符
 */
export function escapeHtml(str: string): string {
  if (!str) return '';
  const escapeMap: Record<string, string> = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;',
  };
  return str.replace(/[&<>"']/g, (char) => escapeMap[char] ?? char);
}

/**
 * 反转义 HTML 特殊字符
 */
export function unescapeHtml(str: string): string {
  if (!str) return '';
  const unescapeMap: Record<string, string> = {
    '&amp;': '&',
    '&lt;': '<',
    '&gt;': '>',
    '&quot;': '"',
    '&#39;': "'",
  };
  return str.replace(/&amp;|&lt;|&gt;|&quot;|&#39;/g, (entity) => unescapeMap[entity] ?? entity);
}

/**
 * 移除 HTML 标签
 */
export function stripHtml(str: string): string {
  if (!str) return '';
  return str.replace(/<[^>]*>/g, '');
}

/**
 * 隐藏手机号中间4位
 */
export function maskPhone(phone: string): string {
  if (!phone || phone.length < 7) return phone;
  return phone.replace(/(\d{3})\d{4}(\d+)/, '$1****$2');
}

/**
 * 隐藏邮箱用户名部分
 */
export function maskEmail(email: string): string {
  if (!email) return '';
  const parts = email.split('@');
  const name = parts[0];
  const domain = parts[1];
  if (!name || !domain) return email;
  if (name.length <= 2) {
    return `${name[0]}***@${domain}`;
  }
  return `${name[0]}***${name[name.length - 1]}@${domain}`;
}

/**
 * 隐藏身份证号中间部分
 */
export function maskIdCard(idCard: string): string {
  if (!idCard || idCard.length < 10) return idCard;
  return idCard.replace(/(\d{4})\d+(\d{4})/, '$1**********$2');
}

// 默认导出
export default {
  trim,
  trimStart,
  trimEnd,
  trimAll,
  capitalize,
  capitalizeWords,
  toUpperCase,
  toLowerCase,
  camelCase,
  pascalCase,
  snakeCase,
  screamingSnakeCase,
  kebabCase,
  truncate,
  truncateByWidth,
  padStart,
  padEnd,
  repeat,
  template,
  replaceAll,
  remove,
  split,
  join,
  splitLines,
  contains,
  startsWith,
  endsWith,
  isEmpty,
  isBlank,
  countOccurrences,
  reverse,
  byteLength,
  randomString,
  uuid,
  escapeHtml,
  unescapeHtml,
  stripHtml,
  maskPhone,
  maskEmail,
  maskIdCard,
};
