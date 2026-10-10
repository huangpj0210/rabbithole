/**
 * 时间处理工具类
 * 基于 dayjs 实现
 */
import dayjs, { type Dayjs, type ManipulateType } from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';
import 'dayjs/locale/zh-cn';

// 启用相对时间插件和中文
dayjs.extend(relativeTime);
dayjs.locale('zh-cn');

export type DateInput = string | number | Date | Dayjs | null | undefined;

// ==================== 格式化 ====================

/**
 * 格式化日期
 * @param date 日期
 * @param format 格式，默认 'YYYY-MM-DD HH:mm:ss'
 */
export function formatDate(date: DateInput, format = 'YYYY-MM-DD HH:mm:ss'): string {
  if (!date) return '';
  return dayjs(date).format(format);
}

/**
 * 格式化为日期（不含时间）
 */
export function formatDateOnly(date: DateInput): string {
  return formatDate(date, 'YYYY-MM-DD');
}

/**
 * 格式化为时间（不含日期）
 */
export function formatTimeOnly(date: DateInput): string {
  return formatDate(date, 'HH:mm:ss');
}

/**
 * 格式化为中文日期
 */
export function formatDateCN(date: DateInput): string {
  return formatDate(date, 'YYYY年MM月DD日');
}

/**
 * 格式化为中文日期时间
 */
export function formatDateTimeCN(date: DateInput): string {
  return formatDate(date, 'YYYY年MM月DD日 HH:mm:ss');
}

// ==================== 解析 ====================

/**
 * 解析日期字符串
 * @param dateStr 日期字符串
 * @param format 格式
 */
export function parseDate(dateStr: string, format?: string): Date | null {
  if (!dateStr) return null;
  const parsed = format ? dayjs(dateStr, format) : dayjs(dateStr);
  return parsed.isValid() ? parsed.toDate() : null;
}

/**
 * 获取时间戳（秒）
 */
export function getTimestamp(date?: DateInput): number {
  return dayjs(date).unix();
}

/**
 * 获取时间戳（毫秒）
 */
export function getTimestampMs(date?: DateInput): number {
  return dayjs(date).valueOf();
}

// ==================== 相对时间 ====================

/**
 * 获取相对时间 (如 "2小时前", "3天后")
 */
export function getRelativeTime(date: DateInput): string {
  if (!date) return '';
  return dayjs(date).fromNow();
}

/**
 * 获取距离指定时间的相对描述
 */
export function getRelativeTimeTo(date: DateInput, compareDate: DateInput): string {
  if (!date || !compareDate) return '';
  return dayjs(date).from(dayjs(compareDate));
}

/**
 * 获取友好的时间显示
 * 今天显示时间，昨天显示"昨天 HH:mm"，其他显示完整日期
 */
export function getFriendlyTime(date: DateInput): string {
  if (!date) return '';
  const d = dayjs(date);
  const now = dayjs();

  if (d.isSame(now, 'day')) {
    return d.format('HH:mm');
  } else if (d.isSame(now.subtract(1, 'day'), 'day')) {
    return `昨天 ${d.format('HH:mm')}`;
  } else if (d.isSame(now, 'year')) {
    return d.format('MM-DD HH:mm');
  } else {
    return d.format('YYYY-MM-DD HH:mm');
  }
}

// ==================== 时间操作 ====================

/**
 * 时间加减
 * @param date 日期
 * @param amount 数量
 * @param unit 单位 (day, month, year, hour, minute, second, week)
 */
export function addTime(date: DateInput, amount: number, unit: ManipulateType = 'day'): Date {
  return dayjs(date).add(amount, unit).toDate();
}

/**
 * 时间减少
 */
export function subtractTime(date: DateInput, amount: number, unit: ManipulateType = 'day'): Date {
  return dayjs(date).subtract(amount, unit).toDate();
}

/**
 * 获取开始时间
 * @param unit 单位 (day, month, year, week)
 */
export function startOf(date: DateInput, unit: ManipulateType = 'day'): Date {
  return dayjs(date).startOf(unit).toDate();
}

/**
 * 获取结束时间
 */
export function endOf(date: DateInput, unit: ManipulateType = 'day'): Date {
  return dayjs(date).endOf(unit).toDate();
}

// ==================== 日期计算 ====================

/**
 * 计算两个日期之间的天数差
 */
export function getDaysBetween(date1: DateInput, date2: DateInput): number {
  return Math.abs(dayjs(date1).diff(dayjs(date2), 'day'));
}

/**
 * 计算两个日期之间的差值
 * @param unit 单位 (day, month, year, hour, minute, second)
 */
export function getDateDiff(date1: DateInput, date2: DateInput, unit: ManipulateType = 'day'): number {
  return dayjs(date1).diff(dayjs(date2), unit);
}

/**
 * 计算年龄
 */
export function getAge(birthday: DateInput): number {
  if (!birthday) return 0;
  return dayjs().diff(dayjs(birthday), 'year');
}

// ==================== 日期判断 ====================

/**
 * 是否是今天
 */
export function isToday(date: DateInput): boolean {
  return dayjs(date).isSame(dayjs(), 'day');
}

/**
 * 是否是昨天
 */
export function isYesterday(date: DateInput): boolean {
  return dayjs(date).isSame(dayjs().subtract(1, 'day'), 'day');
}

/**
 * 是否是明天
 */
export function isTomorrow(date: DateInput): boolean {
  return dayjs(date).isSame(dayjs().add(1, 'day'), 'day');
}

/**
 * 是否是本周
 */
export function isThisWeek(date: DateInput): boolean {
  return dayjs(date).isSame(dayjs(), 'week');
}

/**
 * 是否是本月
 */
export function isThisMonth(date: DateInput): boolean {
  return dayjs(date).isSame(dayjs(), 'month');
}

/**
 * 是否是本年
 */
export function isThisYear(date: DateInput): boolean {
  return dayjs(date).isSame(dayjs(), 'year');
}

/**
 * 判断日期是否在某个范围内
 */
export function isBetween(date: DateInput, start: DateInput, end: DateInput): boolean {
  const d = dayjs(date);
  return d.isAfter(dayjs(start)) && d.isBefore(dayjs(end));
}

/**
 * 是否是过去的时间
 */
export function isPast(date: DateInput): boolean {
  return dayjs(date).isBefore(dayjs());
}

/**
 * 是否是将来的时间
 */
export function isFuture(date: DateInput): boolean {
  return dayjs(date).isAfter(dayjs());
}

/**
 * 是否是有效日期
 */
export function isValidDate(date: DateInput): boolean {
  return dayjs(date).isValid();
}

// ==================== 获取日期部分 ====================

/**
 * 获取年份
 */
export function getYear(date?: DateInput): number {
  return dayjs(date).year();
}

/**
 * 获取月份 (1-12)
 */
export function getMonth(date?: DateInput): number {
  return dayjs(date).month() + 1;
}

/**
 * 获取日期 (1-31)
 */
export function getDay(date?: DateInput): number {
  return dayjs(date).date();
}

/**
 * 获取星期几 (0-6, 0为周日)
 */
export function getWeekday(date?: DateInput): number {
  return dayjs(date).day();
}

/**
 * 获取星期几的中文名称
 */
export function getWeekdayCN(date?: DateInput): string {
  const weekdays = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'] as const;
  return weekdays[getWeekday(date)] ?? '周日';
}

/**
 * 获取小时 (0-23)
 */
export function getHour(date?: DateInput): number {
  return dayjs(date).hour();
}

/**
 * 获取分钟 (0-59)
 */
export function getMinute(date?: DateInput): number {
  return dayjs(date).minute();
}

/**
 * 获取秒 (0-59)
 */
export function getSecond(date?: DateInput): number {
  return dayjs(date).second();
}

// ==================== 特殊功能 ====================

/**
 * 获取某月的天数
 */
export function getDaysInMonth(date?: DateInput): number {
  return dayjs(date).daysInMonth();
}

/**
 * 获取今天的日期范围 [开始, 结束]
 */
export function getTodayRange(): [Date, Date] {
  const today = dayjs();
  return [today.startOf('day').toDate(), today.endOf('day').toDate()];
}

/**
 * 获取本周的日期范围
 */
export function getThisWeekRange(): [Date, Date] {
  const now = dayjs();
  return [now.startOf('week').toDate(), now.endOf('week').toDate()];
}

/**
 * 获取本月的日期范围
 */
export function getThisMonthRange(): [Date, Date] {
  const now = dayjs();
  return [now.startOf('month').toDate(), now.endOf('month').toDate()];
}

/**
 * 获取最近 N 天的日期范围
 */
export function getLastNDaysRange(n: number): [Date, Date] {
  const now = dayjs();
  return [now.subtract(n - 1, 'day').startOf('day').toDate(), now.endOf('day').toDate()];
}

// 默认导出
export default {
  formatDate,
  formatDateOnly,
  formatTimeOnly,
  formatDateCN,
  formatDateTimeCN,
  parseDate,
  getTimestamp,
  getTimestampMs,
  getRelativeTime,
  getRelativeTimeTo,
  getFriendlyTime,
  addTime,
  subtractTime,
  startOf,
  endOf,
  getDaysBetween,
  getDateDiff,
  getAge,
  isToday,
  isYesterday,
  isTomorrow,
  isThisWeek,
  isThisMonth,
  isThisYear,
  isBetween,
  isPast,
  isFuture,
  isValidDate,
  getYear,
  getMonth,
  getDay,
  getWeekday,
  getWeekdayCN,
  getHour,
  getMinute,
  getSecond,
  getDaysInMonth,
  getTodayRange,
  getThisWeekRange,
  getThisMonthRange,
  getLastNDaysRange,
};
