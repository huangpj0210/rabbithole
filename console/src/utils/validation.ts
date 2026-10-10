/**
 * 验证处理工具类
 */

// ==================== 常用正则表达式 ====================

export const PATTERNS = {
  // 邮箱
  EMAIL: /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/,
  // 手机号（中国大陆）
  PHONE: /^1[3-9]\d{9}$/,
  // 座机号
  TELEPHONE: /^0\d{2,3}-?\d{7,8}$/,
  // URL
  URL: /^(https?:\/\/)?([\da-z.-]+)\.([a-z.]{2,6})([/\w .-]*)*\/?$/,
  // 身份证号（18位）
  ID_CARD: /^[1-9]\d{5}(19|20)\d{2}(0[1-9]|1[0-2])(0[1-9]|[12]\d|3[01])\d{3}[\dXx]$/,
  // 邮政编码
  POSTAL_CODE: /^[1-9]\d{5}$/,
  // 整数
  INTEGER: /^-?\d+$/,
  // 正整数
  POSITIVE_INTEGER: /^[1-9]\d*$/,
  // 非负整数
  NON_NEGATIVE_INTEGER: /^\d+$/,
  // 小数
  DECIMAL: /^-?\d+(\.\d+)?$/,
  // 正小数
  POSITIVE_DECIMAL: /^\d+(\.\d+)?$/,
  // 中文
  CHINESE: /^[\u4e00-\u9fa5]+$/,
  // 英文
  ENGLISH: /^[a-zA-Z]+$/,
  // 字母数字
  ALPHANUMERIC: /^[a-zA-Z0-9]+$/,
  // 用户名（字母开头，允许字母数字下划线，4-16位）
  USERNAME: /^[a-zA-Z][a-zA-Z0-9_]{3,15}$/,
  // 密码（至少8位，包含字母和数字）
  PASSWORD: /^(?=.*[a-zA-Z])(?=.*\d)[a-zA-Z\d!@#$%^&*()_+\-=[\]{};':"\\|,.<>/?]{8,}$/,
  // 强密码（至少8位，包含大小写字母、数字和特殊字符）
  STRONG_PASSWORD: /^(?=.*[a-z])(?=.*[A-Z])(?=.*\d)(?=.*[!@#$%^&*()_+\-=[\]{};':"\\|,.<>/?]).{8,}$/,
  // 银行卡号
  BANK_CARD: /^[1-9]\d{15,18}$/,
  // 车牌号
  LICENSE_PLATE: /^[京津沪渝冀豫云辽黑湘皖鲁新苏浙赣鄂桂甘晋蒙陕吉闽贵粤青藏川宁琼使领][A-Z][A-HJ-NP-Z0-9]{4,5}[A-HJ-NP-Z0-9挂学警港澳]$/,
  // IPv4
  IPV4: /^((25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(25[0-5]|2[0-4]\d|[01]?\d\d?)$/,
  // MAC地址
  MAC: /^([0-9A-Fa-f]{2}[:-]){5}([0-9A-Fa-f]{2})$/,
  // 版本号
  VERSION: /^\d+(\.\d+){0,2}$/,
  // 日期 YYYY-MM-DD
  DATE: /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/,
  // 时间 HH:mm:ss
  TIME: /^([01]\d|2[0-3]):([0-5]\d):([0-5]\d)$/,
  // 日期时间
  DATETIME: /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])\s([01]\d|2[0-3]):([0-5]\d):([0-5]\d)$/,
};

// ==================== 验证函数 ====================

/**
 * 验证邮箱
 */
export function isEmail(value: string): boolean {
  if (!value) return false;
  return PATTERNS.EMAIL.test(value);
}

/**
 * 验证手机号
 */
export function isPhone(value: string): boolean {
  if (!value) return false;
  return PATTERNS.PHONE.test(value);
}

/**
 * 验证座机号
 */
export function isTelephone(value: string): boolean {
  if (!value) return false;
  return PATTERNS.TELEPHONE.test(value);
}

/**
 * 验证 URL
 */
export function isUrl(value: string): boolean {
  if (!value) return false;
  return PATTERNS.URL.test(value);
}

/**
 * 验证身份证号
 */
export function isIdCard(value: string): boolean {
  if (!value) return false;
  if (!PATTERNS.ID_CARD.test(value)) return false;

  // 验证校验码
  const weights = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
  const checkCodes = ['1', '0', 'X', '9', '8', '7', '6', '5', '4', '3', '2'];

  let sum = 0;
  for (let i = 0; i < 17; i++) {
    sum += parseInt(value[i]!) * weights[i]!;
  }

  const checkCode = checkCodes[sum % 11]!;
  return value[17]!.toUpperCase() === checkCode;
}

/**
 * 验证邮政编码
 */
export function isPostalCode(value: string): boolean {
  if (!value) return false;
  return PATTERNS.POSTAL_CODE.test(value);
}

/**
 * 验证密码强度
 * @returns 0: 无效, 1: 弱, 2: 中, 3: 强
 */
export function getPasswordStrength(value: string): number {
  if (!value || value.length < 6) return 0;

  let strength = 0;

  // 包含小写字母
  if (/[a-z]/.test(value)) strength++;
  // 包含大写字母
  if (/[A-Z]/.test(value)) strength++;
  // 包含数字
  if (/\d/.test(value)) strength++;
  // 包含特殊字符
  if (/[!@#$%^&*()_+\-=[\]{};':"\\|,.<>/?]/.test(value)) strength++;
  // 长度大于10
  if (value.length >= 10) strength++;

  if (strength <= 2) return 1; // 弱
  if (strength <= 3) return 2; // 中
  return 3; // 强
}

/**
 * 验证密码是否符合要求
 */
export function isPassword(value: string, minLength = 8): boolean {
  if (!value || value.length < minLength) return false;
  return PATTERNS.PASSWORD.test(value);
}

/**
 * 验证强密码
 */
export function isStrongPassword(value: string): boolean {
  if (!value) return false;
  return PATTERNS.STRONG_PASSWORD.test(value);
}

/**
 * 验证用户名
 */
export function isUsername(value: string): boolean {
  if (!value) return false;
  return PATTERNS.USERNAME.test(value);
}

/**
 * 验证银行卡号
 */
export function isBankCard(value: string): boolean {
  if (!value) return false;
  return PATTERNS.BANK_CARD.test(value.replace(/\s/g, ''));
}

/**
 * 验证车牌号
 */
export function isLicensePlate(value: string): boolean {
  if (!value) return false;
  return PATTERNS.LICENSE_PLATE.test(value);
}

/**
 * 验证 IPv4
 */
export function isIPv4(value: string): boolean {
  if (!value) return false;
  return PATTERNS.IPV4.test(value);
}

/**
 * 验证 MAC 地址
 */
export function isMAC(value: string): boolean {
  if (!value) return false;
  return PATTERNS.MAC.test(value);
}

/**
 * 验证是否全是中文
 */
export function isChinese(value: string): boolean {
  if (!value) return false;
  return PATTERNS.CHINESE.test(value);
}

/**
 * 验证是否全是英文
 */
export function isEnglish(value: string): boolean {
  if (!value) return false;
  return PATTERNS.ENGLISH.test(value);
}

/**
 * 验证是否只包含字母和数字
 */
export function isAlphanumeric(value: string): boolean {
  if (!value) return false;
  return PATTERNS.ALPHANUMERIC.test(value);
}

/**
 * 验证整数
 */
export function isInteger(value: string | number): boolean {
  if (value === '' || value === null || value === undefined) return false;
  return PATTERNS.INTEGER.test(String(value));
}

/**
 * 验证正整数
 */
export function isPositiveInteger(value: string | number): boolean {
  if (value === '' || value === null || value === undefined) return false;
  return PATTERNS.POSITIVE_INTEGER.test(String(value));
}

/**
 * 验证小数
 */
export function isDecimal(value: string | number): boolean {
  if (value === '' || value === null || value === undefined) return false;
  return PATTERNS.DECIMAL.test(String(value));
}

/**
 * 验证日期格式 (YYYY-MM-DD)
 */
export function isDate(value: string): boolean {
  if (!value) return false;
  return PATTERNS.DATE.test(value);
}

/**
 * 验证时间格式 (HH:mm:ss)
 */
export function isTime(value: string): boolean {
  if (!value) return false;
  return PATTERNS.TIME.test(value);
}

/**
 * 验证日期时间格式
 */
export function isDateTime(value: string): boolean {
  if (!value) return false;
  return PATTERNS.DATETIME.test(value);
}

// ==================== 类型判断 ====================

/**
 * 判断是否为空值
 */
export function isEmpty(value: unknown): boolean {
  if (value === null || value === undefined) return true;
  if (typeof value === 'string') return value.trim() === '';
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === 'object') return Object.keys(value).length === 0;
  return false;
}

/**
 * 判断是否不为空
 */
export function isNotEmpty(value: unknown): boolean {
  return !isEmpty(value);
}

/**
 * 判断类型
 */
export function isType(value: unknown, type: string): boolean {
  const typeString = Object.prototype.toString.call(value);
  return typeString === `[object ${type}]`;
}

/**
 * 是否是字符串
 */
export function isString(value: unknown): value is string {
  return typeof value === 'string';
}

/**
 * 是否是数字
 */
export function isNumber(value: unknown): value is number {
  return typeof value === 'number' && !isNaN(value) && isFinite(value);
}

/**
 * 是否是布尔值
 */
export function isBoolean(value: unknown): value is boolean {
  return typeof value === 'boolean';
}

/**
 * 是否是数组
 */
export function isArray(value: unknown): value is unknown[] {
  return Array.isArray(value);
}

/**
 * 是否是对象
 */
export function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/**
 * 是否是函数
 */
export function isFunction(value: unknown): value is (...args: unknown[]) => unknown {
  return typeof value === 'function';
}

/**
 * 是否是 undefined
 */
export function isUndefined(value: unknown): value is undefined {
  return value === undefined;
}

/**
 * 是否是 null
 */
export function isNull(value: unknown): value is null {
  return value === null;
}

/**
 * 是否是 Date 对象
 */
export function isDate_(value: unknown): value is Date {
  return value instanceof Date && !isNaN(value.getTime());
}

/**
 * 是否是正则表达式
 */
export function isRegExp(value: unknown): value is RegExp {
  return value instanceof RegExp;
}

/**
 * 是否是 Promise
 */
export function isPromise(value: unknown): value is Promise<unknown> {
  return value instanceof Promise || (isObject(value) && isFunction((value as any).then) && isFunction((value as any).catch));
}

// ==================== 自定义验证 ====================

/**
 * 创建验证器
 */
export function createValidator(pattern: RegExp, message = '格式不正确') {
  return (value: string) => {
    if (!value) return true;
    return pattern.test(value) ? true : message;
  };
}

/**
 * 验证字符串长度范围
 */
export function isLengthBetween(value: string, min: number, max: number): boolean {
  if (!value) return min === 0;
  return value.length >= min && value.length <= max;
}

/**
 * 验证数值范围
 */
export function isNumberBetween(value: number, min: number, max: number): boolean {
  return value >= min && value <= max;
}

// 默认导出
export default {
  PATTERNS,
  isEmail,
  isPhone,
  isTelephone,
  isUrl,
  isIdCard,
  isPostalCode,
  getPasswordStrength,
  isPassword,
  isStrongPassword,
  isUsername,
  isBankCard,
  isLicensePlate,
  isIPv4,
  isMAC,
  isChinese,
  isEnglish,
  isAlphanumeric,
  isInteger,
  isPositiveInteger,
  isDecimal,
  isDate,
  isTime,
  isDateTime,
  isEmpty,
  isNotEmpty,
  isType,
  isString,
  isNumber,
  isBoolean,
  isArray,
  isObject,
  isFunction,
  isUndefined,
  isNull,
  isDate_,
  isRegExp,
  isPromise,
  createValidator,
  isLengthBetween,
  isNumberBetween,
};
