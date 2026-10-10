/** 展示格式化工具。 */
export function fmtBytes(n: number | null | undefined): string {
  const v = n ?? 0;
  if (v > 1024 * 1024 * 1024) return `${(v / 1024 ** 3).toFixed(2)} GB`;
  if (v > 1024 * 1024) return `${(v / 1024 ** 2).toFixed(2)} MB`;
  if (v > 1024) return `${(v / 1024).toFixed(1)} KB`;
  return `${v} B`;
}

export function fmtTime(ts: number | null | undefined): string {
  if (!ts) return '-';
  return new Date(ts).toLocaleString('zh-CN', { hour12: false });
}
