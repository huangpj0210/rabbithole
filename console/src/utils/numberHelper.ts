/**
 * 数字处理工具类
 */

// ==================== 格式化 ====================

/**
 * 格式化数字（添加千分位分隔符）
 * @param num 数字
 * @param decimals 小数位数，默认 2
 */
export function formatNumber(num: number | string, decimals = 2): string {
  if (num === null || num === undefined || num === '') return '';
  const n = typeof num === 'string' ? parseFloat(num) : num;
  if (isNaN(n)) return '';
  return n.toLocaleString('zh-CN', {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

/**
 * 格式化整数（添加千分位分隔符，无小数）
 */
export function formatInteger(num: number | string): string {
  if (num === null || num === undefined || num === '') return '';
  const n = typeof num === 'string' ? parseInt(num, 10) : Math.floor(num);
  if (isNaN(n)) return '';
  return n.toLocaleString('zh-CN');
}

/**
 * 货币格式化
 * @param num 数字
 * @param currency 货币符号，默认 '¥'
 * @param decimals 小数位数，默认 2
 */
export function formatCurrency(num: number | string, currency = '¥', decimals = 2): string {
  if (num === null || num === undefined || num === '') return '';
  const n = typeof num === 'string' ? parseFloat(num) : num;
  if (isNaN(n)) return '';
  const formatted = Math.abs(n).toLocaleString('zh-CN', {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  return n < 0 ? `-${currency}${formatted}` : `${currency}${formatted}`;
}

/**
 * 简化数字显示 (1000 -> 1K, 1000000 -> 1M)
 * @param num 数字
 * @param decimals 小数位数，默认 1
 */
export function abbreviateNumber(num: number | string, decimals = 1): string {
  if (num === null || num === undefined || num === '') return '';
  const n = typeof num === 'string' ? parseFloat(num) : num;
  if (isNaN(n)) return '';

  const absNum = Math.abs(n);
  const sign = n < 0 ? '-' : '';

  if (absNum >= 1e12) {
    return sign + (absNum / 1e12).toFixed(decimals) + 'T';
  } else if (absNum >= 1e9) {
    return sign + (absNum / 1e9).toFixed(decimals) + 'B';
  } else if (absNum >= 1e6) {
    return sign + (absNum / 1e6).toFixed(decimals) + 'M';
  } else if (absNum >= 1e4) {
    return sign + (absNum / 1e4).toFixed(decimals) + 'W';
  } else if (absNum >= 1e3) {
    return sign + (absNum / 1e3).toFixed(decimals) + 'K';
  }
  return String(n);
}

/**
 * 中文数字简化显示 (10000 -> 1万, 100000000 -> 1亿)
 */
export function abbreviateNumberCN(num: number | string, decimals = 1): string {
  if (num === null || num === undefined || num === '') return '';
  const n = typeof num === 'string' ? parseFloat(num) : num;
  if (isNaN(n)) return '';

  const absNum = Math.abs(n);
  const sign = n < 0 ? '-' : '';

  if (absNum >= 1e8) {
    return sign + (absNum / 1e8).toFixed(decimals) + '亿';
  } else if (absNum >= 1e4) {
    return sign + (absNum / 1e4).toFixed(decimals) + '万';
  }
  return String(n);
}

// ==================== 百分比 ====================

/**
 * 计算百分比
 * @param num 分子
 * @param total 分母
 * @param decimals 小数位数，默认 2
 */
export function percentage(num: number, total: number, decimals = 2): number {
  if (!total || total === 0) return 0;
  return toFixed((num / total) * 100, decimals);
}

/**
 * 格式化百分比显示
 */
export function formatPercentage(num: number, total: number, decimals = 2): string {
  return percentage(num, total, decimals) + '%';
}

/**
 * 数字转百分比显示
 * @param num 0-1 之间的小数
 */
export function toPercentage(num: number, decimals = 2): string {
  if (num === null || num === undefined) return '';
  return toFixed(num * 100, decimals) + '%';
}

// ==================== 精度处理 ====================

/**
 * 精确到指定小数位数（四舍五入）
 */
export function toFixed(num: number, decimals = 2): number {
  if (num === null || num === undefined || isNaN(num)) return 0;
  const factor = Math.pow(10, decimals);
  return Math.round(num * factor) / factor;
}

/**
 * 向上取整到指定小数位
 */
export function roundUp(num: number, decimals = 0): number {
  if (num === null || num === undefined || isNaN(num)) return 0;
  const factor = Math.pow(10, decimals);
  return Math.ceil(num * factor) / factor;
}

/**
 * 向下取整到指定小数位
 */
export function roundDown(num: number, decimals = 0): number {
  if (num === null || num === undefined || isNaN(num)) return 0;
  const factor = Math.pow(10, decimals);
  return Math.floor(num * factor) / factor;
}

/**
 * 四舍五入到指定小数位
 */
export function round(num: number, decimals = 0): number {
  return toFixed(num, decimals);
}

// ==================== 随机数 ====================

/**
 * 生成随机整数
 * @param min 最小值（包含）
 * @param max 最大值（包含）
 */
export function random(min: number, max: number): number {
  return Math.floor(Math.random() * (max - min + 1)) + min;
}

/**
 * 生成随机浮点数
 */
export function randomFloat(min: number, max: number, decimals = 2): number {
  const num = Math.random() * (max - min) + min;
  return toFixed(num, decimals);
}

/**
 * 生成随机 ID
 */
export function randomId(): string {
  return Math.random().toString(36).substring(2, 15) + Math.random().toString(36).substring(2, 15);
}

// ==================== 字节转换 ====================

/**
 * 字节转换为可读格式
 * @param bytes 字节数
 * @param decimals 小数位数，默认 2
 */
export function bytesToSize(bytes: number, decimals = 2): string {
  if (bytes === 0) return '0 B';
  if (!bytes || bytes < 0) return '';

  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return parseFloat((bytes / Math.pow(k, i)).toFixed(decimals)) + ' ' + sizes[i];
}

/**
 * 大小字符串转换为字节数
 * @param sizeStr 大小字符串，如 "1.5MB", "2GB"
 */
export function sizeToBytes(sizeStr: string): number {
  if (!sizeStr) return 0;

  const units: Record<string, number> = {
    B: 1,
    KB: 1024,
    MB: 1024 * 1024,
    GB: 1024 * 1024 * 1024,
    TB: 1024 * 1024 * 1024 * 1024,
  };

  const match = sizeStr.toUpperCase().match(/^([\d.]+)\s*(B|KB|MB|GB|TB)?$/);
  if (!match || !match[1]) return 0;

  const num = parseFloat(match[1]);
  const unit = match[2] ?? 'B';

  return Math.round(num * (units[unit] || 1));
}

// ==================== 范围处理 ====================

/**
 * 限制数字在指定范围内
 */
export function clamp(num: number, min: number, max: number): number {
  return Math.min(Math.max(num, min), max);
}

/**
 * 判断数字是否在范围内
 */
export function inRange(num: number, min: number, max: number): boolean {
  return num >= min && num <= max;
}

// ==================== 安全运算（解决浮点数精度问题）====================

/**
 * 获取数字的小数位数
 */
function getDecimalLength(num: number): number {
  const str = String(num);
  const dotIndex = str.indexOf('.');
  return dotIndex === -1 ? 0 : str.length - dotIndex - 1;
}

/**
 * 将数字转为整数（去除小数点）
 */
function toInt(num: number): number {
  return Number(String(num).replace('.', ''));
}

/**
 * 安全加法 (解决 0.1 + 0.2 !== 0.3 的问题)
 * @example add(0.1, 0.2) // 0.3
 */
export function add(a: number, b: number): number {
  const maxDecimal = Math.max(getDecimalLength(a), getDecimalLength(b));
  const factor = Math.pow(10, maxDecimal);
  return (Math.round(a * factor) + Math.round(b * factor)) / factor;
}

/**
 * 安全减法
 * @example subtract(0.3, 0.1) // 0.2
 */
export function subtract(a: number, b: number): number {
  const maxDecimal = Math.max(getDecimalLength(a), getDecimalLength(b));
  const factor = Math.pow(10, maxDecimal);
  return (Math.round(a * factor) - Math.round(b * factor)) / factor;
}

/**
 * 安全乘法
 * @example multiply(0.1, 0.2) // 0.02
 */
export function multiply(a: number, b: number): number {
  const decimalA = getDecimalLength(a);
  const decimalB = getDecimalLength(b);
  const factor = Math.pow(10, decimalA + decimalB);
  return (toInt(a) * toInt(b)) / factor;
}

/**
 * 安全除法
 * @example divide(0.3, 0.1) // 3
 */
export function divide(a: number, b: number): number {
  if (b === 0) {
    console.warn('divide: 除数不能为0');
    return 0;
  }
  const decimalA = getDecimalLength(a);
  const decimalB = getDecimalLength(b);
  const factor = Math.pow(10, decimalB - decimalA);
  return (toInt(a) / toInt(b)) * factor;
}

/**
 * 安全取模/求余
 * @example modulo(0.3, 0.1) // 0
 */
export function modulo(a: number, b: number): number {
  if (b === 0) {
    console.warn('modulo: 除数不能为0');
    return 0;
  }
  const maxDecimal = Math.max(getDecimalLength(a), getDecimalLength(b));
  const factor = Math.pow(10, maxDecimal);
  return (Math.round(a * factor) % Math.round(b * factor)) / factor;
}

/**
 * 安全幂运算
 * @example pow(0.1, 2) // 0.01
 */
export function pow(base: number, exponent: number): number {
  const decimal = getDecimalLength(base);
  if (decimal === 0 || exponent < 0) {
    return Math.pow(base, exponent);
  }
  const factor = Math.pow(10, decimal * exponent);
  return Math.pow(toInt(base), exponent) / factor;
}

/**
 * 链式安全计算
 * @example calc(0.1).add(0.2).multiply(10).value() // 3
 */
export function calc(initial: number) {
  let result = initial;

  const chain = {
    add(n: number) {
      result = add(result, n);
      return chain;
    },
    subtract(n: number) {
      result = subtract(result, n);
      return chain;
    },
    multiply(n: number) {
      result = multiply(result, n);
      return chain;
    },
    divide(n: number) {
      result = divide(result, n);
      return chain;
    },
    modulo(n: number) {
      result = modulo(result, n);
      return chain;
    },
    pow(n: number) {
      result = pow(result, n);
      return chain;
    },
    round(decimals = 0) {
      result = toFixed(result, decimals);
      return chain;
    },
    value() {
      return result;
    },
  };

  return chain;
}

/**
 * 安全求和（数组）
 * @example safeSum([0.1, 0.2, 0.3]) // 0.6
 */
export function safeSum(numbers: number[]): number {
  return numbers.reduce((acc, num) => add(acc, num || 0), 0);
}

/**
 * 安全平均值
 * @example safeAverage([0.1, 0.2, 0.3]) // 0.2
 */
export function safeAverage(numbers: number[]): number {
  if (!numbers.length) return 0;
  return divide(safeSum(numbers), numbers.length);
}

// ==================== 数学运算 ====================

/**
 * 求和
 */
export function sum(numbers: number[]): number {
  return numbers.reduce((acc, num) => acc + (num || 0), 0);
}

/**
 * 平均值
 */
export function average(numbers: number[]): number {
  if (!numbers.length) return 0;
  return sum(numbers) / numbers.length;
}

/**
 * 最大值
 */
export function max(numbers: number[]): number {
  if (!numbers.length) return 0;
  return Math.max(...numbers);
}

/**
 * 最小值
 */
export function min(numbers: number[]): number {
  if (!numbers.length) return 0;
  return Math.min(...numbers);
}

// ==================== 判断 ====================

/**
 * 是否是数字
 */
export function isNumber(value: unknown): value is number {
  return typeof value === 'number' && !isNaN(value) && isFinite(value);
}

/**
 * 是否是整数
 */
export function isInteger(value: unknown): boolean {
  return isNumber(value) && Number.isInteger(value);
}

/**
 * 是否是正数
 */
export function isPositive(value: number): boolean {
  return isNumber(value) && value > 0;
}

/**
 * 是否是负数
 */
export function isNegative(value: number): boolean {
  return isNumber(value) && value < 0;
}

/**
 * 是否是偶数
 */
export function isEven(value: number): boolean {
  return isInteger(value) && value % 2 === 0;
}

/**
 * 是否是奇数
 */
export function isOdd(value: number): boolean {
  return isInteger(value) && value % 2 !== 0;
}

// ==================== 转换 ====================

/**
 * 安全转换为数字
 */
export function toNumber(value: unknown, defaultValue = 0): number {
  if (value === null || value === undefined || value === '') return defaultValue;
  const num = Number(value);
  return isNaN(num) ? defaultValue : num;
}

/**
 * 安全转换为整数
 */
export function toInteger(value: unknown, defaultValue = 0): number {
  const num = toNumber(value, defaultValue);
  return Math.floor(num);
}

// 默认导出
export default {
  // 格式化
  formatNumber,
  formatInteger,
  formatCurrency,
  abbreviateNumber,
  abbreviateNumberCN,
  // 百分比
  percentage,
  formatPercentage,
  toPercentage,
  // 精度处理
  toFixed,
  roundUp,
  roundDown,
  round,
  // 随机数
  random,
  randomFloat,
  randomId,
  // 字节转换
  bytesToSize,
  sizeToBytes,
  // 范围处理
  clamp,
  inRange,
  // 安全运算
  add,
  subtract,
  multiply,
  divide,
  modulo,
  pow,
  calc,
  safeSum,
  safeAverage,
  // 数学运算
  sum,
  average,
  max,
  min,
  // 判断
  isNumber,
  isInteger,
  isPositive,
  isNegative,
  isEven,
  isOdd,
  // 转换
  toNumber,
  toInteger,
};
