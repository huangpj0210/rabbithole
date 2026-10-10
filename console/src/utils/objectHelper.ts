/**
 * 对象/数组处理工具类
 */

// ==================== 对象操作 ====================

/**
 * 深拷贝
 */
export function deepClone<T>(obj: T): T {
  if (obj === null || typeof obj !== 'object') {
    return obj;
  }

  if (obj instanceof Date) {
    return new Date(obj.getTime()) as unknown as T;
  }

  if (obj instanceof RegExp) {
    return new RegExp(obj) as unknown as T;
  }

  if (Array.isArray(obj)) {
    return obj.map((item) => deepClone(item)) as unknown as T;
  }

  const cloned = {} as T;
  for (const key in obj) {
    if (Object.prototype.hasOwnProperty.call(obj, key)) {
      cloned[key] = deepClone(obj[key]);
    }
  }
  return cloned;
}

/**
 * 浅拷贝
 */
export function shallowClone<T>(obj: T): T {
  if (obj === null || typeof obj !== 'object') {
    return obj;
  }

  if (Array.isArray(obj)) {
    return [...obj] as unknown as T;
  }

  return { ...obj };
}

/**
 * 深度合并对象
 */
export function merge<T extends Record<string, unknown>>(...objects: Partial<T>[]): T {
  const result = {} as T;

  for (const obj of objects) {
    if (!obj) continue;

    for (const key in obj) {
      if (Object.prototype.hasOwnProperty.call(obj, key)) {
        const value = obj[key];
        const existing = result[key as keyof T];

        if (isPlainObject(value) && isPlainObject(existing)) {
          result[key as keyof T] = merge(existing as any, value as any) as T[keyof T];
        } else {
          result[key as keyof T] = value as T[keyof T];
        }
      }
    }
  }

  return result;
}

/**
 * 选择对象的指定属性
 */
export function pick<T extends Record<string, unknown>, K extends keyof T>(obj: T, keys: K[]): Pick<T, K> {
  const result = {} as Pick<T, K>;
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(obj, key)) {
      result[key] = obj[key];
    }
  }
  return result;
}

/**
 * 排除对象的指定属性
 */
export function omit<T extends Record<string, unknown>, K extends keyof T>(obj: T, keys: K[]): Omit<T, K> {
  const result = { ...obj };
  for (const key of keys) {
    delete result[key];
  }
  return result as Omit<T, K>;
}

/**
 * 获取对象的所有键
 */
export function keys<T extends Record<string, unknown>>(obj: T): (keyof T)[] {
  return Object.keys(obj) as (keyof T)[];
}

/**
 * 获取对象的所有值
 */
export function values<T extends Record<string, unknown>>(obj: T): T[keyof T][] {
  return Object.values(obj) as T[keyof T][];
}

/**
 * 获取对象的键值对数组
 */
export function entries<T extends Record<string, unknown>>(obj: T): [keyof T, T[keyof T]][] {
  return Object.entries(obj) as [keyof T, T[keyof T]][];
}

/**
 * 从键值对数组创建对象
 */
export function fromEntries<K extends string, V>(entries: [K, V][]): Record<K, V> {
  return Object.fromEntries(entries) as Record<K, V>;
}

/**
 * 判断是否是纯对象
 */
export function isPlainObject(value: unknown): value is Record<string, unknown> {
  return Object.prototype.toString.call(value) === '[object Object]';
}

/**
 * 判断对象是否为空
 */
export function isEmptyObject(obj: Record<string, unknown>): boolean {
  return Object.keys(obj).length === 0;
}

/**
 * 根据路径获取对象的值
 * @example get(obj, 'a.b.c') 或 get(obj, ['a', 'b', 'c'])
 */
export function get<T = unknown>(obj: Record<string, unknown>, path: string | string[], defaultValue?: T): T | undefined {
  const keys = Array.isArray(path) ? path : path.split('.');
  let result: unknown = obj;

  for (const key of keys) {
    if (result === null || result === undefined) {
      return defaultValue;
    }
    result = (result as Record<string, unknown>)[key];
  }

  return (result === undefined ? defaultValue : result) as T | undefined;
}

/**
 * 根据路径设置对象的值
 */
export function set(obj: Record<string, unknown>, path: string | string[], value: unknown): void {
  const keys = Array.isArray(path) ? path : path.split('.');
  let current = obj;

  for (let i = 0; i < keys.length - 1; i++) {
    const key = keys[i]!;
    if (!(key in current) || current[key] === null || typeof current[key] !== 'object') {
      current[key] = {};
    }
    current = current[key] as Record<string, unknown>;
  }

  const lastKey = keys[keys.length - 1]!;
  current[lastKey] = value;
}

/**
 * 检查对象是否包含指定路径
 */
export function has(obj: Record<string, unknown>, path: string | string[]): boolean {
  const keys = Array.isArray(path) ? path : path.split('.');
  let current: unknown = obj;

  for (const key of keys) {
    if (current === null || current === undefined || typeof current !== 'object') {
      return false;
    }
    if (!(key in current)) {
      return false;
    }
    current = (current as Record<string, unknown>)[key];
  }

  return true;
}

// ==================== 数组操作 ====================

/**
 * 数组扁平化
 * @param depth 扁平化深度，默认 1，Infinity 表示完全扁平化
 */
export function flatten<T>(arr: unknown[], depth = 1): T[] {
  if (depth === Infinity) {
    return arr.flat(Infinity) as T[];
  }
  return arr.flat(depth) as T[];
}

/**
 * 数组去重
 * @param key 根据指定属性去重（可选）
 */
export function unique<T>(arr: T[], key?: keyof T): T[] {
  if (!key) {
    return [...new Set(arr)];
  }

  const seen = new Set();
  return arr.filter((item) => {
    const val = item[key];
    if (seen.has(val)) {
      return false;
    }
    seen.add(val);
    return true;
  });
}

/**
 * 数组分组
 */
export function groupBy<T>(arr: T[], key: keyof T | ((item: T) => string | number)): Record<string, T[]> {
  return arr.reduce(
    (groups, item) => {
      const groupKey = typeof key === 'function' ? key(item) : String(item[key]);
      if (!groups[groupKey]) {
        groups[groupKey] = [];
      }
      groups[groupKey].push(item);
      return groups;
    },
    {} as Record<string, T[]>
  );
}

/**
 * 数组排序
 * @param key 排序的属性
 * @param order 排序方式 'asc' | 'desc'
 */
export function sortBy<T>(arr: T[], key: keyof T | ((item: T) => number | string), order: 'asc' | 'desc' = 'asc'): T[] {
  const sorted = [...arr].sort((a, b) => {
    const valA = typeof key === 'function' ? key(a) : a[key];
    const valB = typeof key === 'function' ? key(b) : b[key];

    if (valA < valB) return -1;
    if (valA > valB) return 1;
    return 0;
  });

  return order === 'desc' ? sorted.reverse() : sorted;
}

/**
 * 数组分块
 * @param size 每块的大小
 */
export function chunk<T>(arr: T[], size: number): T[][] {
  if (size <= 0) return [];

  const result: T[][] = [];
  for (let i = 0; i < arr.length; i += size) {
    result.push(arr.slice(i, i + size));
  }
  return result;
}

/**
 * 获取数组第一个元素
 */
export function first<T>(arr: T[]): T | undefined {
  return arr[0];
}

/**
 * 获取数组最后一个元素
 */
export function last<T>(arr: T[]): T | undefined {
  return arr[arr.length - 1];
}

/**
 * 查找元素索引
 */
export function findIndex<T>(arr: T[], predicate: (item: T) => boolean): number {
  return arr.findIndex(predicate);
}

/**
 * 查找元素
 */
export function find<T>(arr: T[], predicate: (item: T) => boolean): T | undefined {
  return arr.find(predicate);
}

/**
 * 移除数组中的指定元素
 */
export function remove<T>(arr: T[], predicate: (item: T) => boolean): T[] {
  return arr.filter((item) => !predicate(item));
}

/**
 * 插入元素到指定位置
 */
export function insert<T>(arr: T[], index: number, ...items: T[]): T[] {
  const result = [...arr];
  result.splice(index, 0, ...items);
  return result;
}

/**
 * 移动数组元素
 */
export function move<T>(arr: T[], from: number, to: number): T[] {
  const result = [...arr];
  const [item] = result.splice(from, 1) as [T];
  result.splice(to, 0, item);
  return result;
}

/**
 * 交换数组元素
 */
export function swap<T>(arr: T[], index1: number, index2: number): T[] {
  const result = [...arr];
  const temp = result[index1]!;
  result[index1] = result[index2]!;
  result[index2] = temp;
  return result;
}

/**
 * 随机打乱数组
 */
export function shuffle<T>(arr: T[]): T[] {
  const result = [...arr];
  for (let i = result.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    const temp = result[i]!;
    result[i] = result[j]!;
    result[j] = temp;
  }
  return result;
}

/**
 * 随机获取数组中的一个元素
 */
export function sample<T>(arr: T[]): T | undefined {
  if (!arr.length) return undefined;
  return arr[Math.floor(Math.random() * arr.length)];
}

/**
 * 随机获取数组中的多个元素
 */
export function sampleSize<T>(arr: T[], size: number): T[] {
  const shuffled = shuffle(arr);
  return shuffled.slice(0, size);
}

/**
 * 计算数组交集
 */
export function intersection<T>(...arrays: T[][]): T[] {
  if (arrays.length === 0) return [];
  const first = arrays[0]!;
  if (arrays.length === 1) return [...first];

  return arrays.reduce((acc, arr) => acc.filter((item) => arr.includes(item)));
}

/**
 * 计算数组并集
 */
export function union<T>(...arrays: T[][]): T[] {
  return [...new Set(arrays.flat())];
}

/**
 * 计算数组差集 (在 arr1 中但不在 arr2 中)
 */
export function difference<T>(arr1: T[], arr2: T[]): T[] {
  const set2 = new Set(arr2);
  return arr1.filter((item) => !set2.has(item));
}

/**
 * 判断数组是否为空
 */
export function isEmptyArray(arr: unknown[]): boolean {
  return arr.length === 0;
}

/**
 * 数组求和
 */
export function sum(arr: number[]): number {
  return arr.reduce((acc, num) => acc + (num || 0), 0);
}

/**
 * 数组求平均值
 */
export function average(arr: number[]): number {
  if (!arr.length) return 0;
  return sum(arr) / arr.length;
}

/**
 * 根据属性求和
 */
export function sumBy<T>(arr: T[], key: keyof T | ((item: T) => number)): number {
  return arr.reduce((acc, item) => {
    const value = typeof key === 'function' ? key(item) : (item[key] as unknown as number);
    return acc + (value || 0);
  }, 0);
}

// ==================== 树形结构操作 ====================

export interface TreeNode {
  id: string | number;
  children?: TreeNode[];
  [key: string]: unknown;
}

/**
 * 数组转树形结构
 */
export function arrayToTree<T extends { id: string | number; parentId?: string | number | null }>(
  items: T[],
  options: { idKey?: string; parentIdKey?: string; childrenKey?: string } = {}
): (T & { children?: T[] })[] {
  const { idKey = 'id', parentIdKey = 'parentId', childrenKey = 'children' } = options;

  const map = new Map<string | number, T & { children?: T[] }>();
  const roots: (T & { children?: T[] })[] = [];

  // 先创建映射
  items.forEach((item) => {
    map.set(item[idKey as keyof T] as string | number, { ...item, [childrenKey]: [] });
  });

  // 构建树
  items.forEach((item) => {
    const node = map.get(item[idKey as keyof T] as string | number)!;
    const parentId = item[parentIdKey as keyof T] as string | number | null | undefined;

    if (parentId === null || parentId === undefined || !map.has(parentId)) {
      roots.push(node);
    } else {
      const parent = map.get(parentId)!;
      ((parent as Record<string, unknown>)[childrenKey] as T[]).push(node);
    }
  });

  return roots;
}

/**
 * 树形结构转数组
 */
export function treeToArray<T extends TreeNode>(tree: T[], childrenKey = 'children'): Omit<T, 'children'>[] {
  const result: Omit<T, 'children'>[] = [];

  function traverse(nodes: T[]) {
    nodes.forEach((node) => {
      const { [childrenKey]: children, ...rest } = node;
      result.push(rest as Omit<T, 'children'>);
      if (children && (children as T[]).length) {
        traverse(children as T[]);
      }
    });
  }

  traverse(tree);
  return result;
}

/**
 * 在树中查找节点
 */
export function findInTree<T extends TreeNode>(tree: T[], predicate: (node: T) => boolean, childrenKey = 'children'): T | undefined {
  for (const node of tree) {
    if (predicate(node)) {
      return node;
    }
    if (node[childrenKey] && (node[childrenKey] as T[]).length) {
      const found = findInTree(node[childrenKey] as T[], predicate, childrenKey);
      if (found) return found;
    }
  }
  return undefined;
}

/**
 * 过滤树形结构
 */
export function filterTree<T extends TreeNode>(tree: T[], predicate: (node: T) => boolean, childrenKey = 'children'): T[] {
  return tree
    .filter(predicate)
    .map((node) => {
      if (node[childrenKey] && (node[childrenKey] as T[]).length) {
        return {
          ...node,
          [childrenKey]: filterTree(node[childrenKey] as T[], predicate, childrenKey),
        };
      }
      return node;
    });
}

// 默认导出
export default {
  deepClone,
  shallowClone,
  merge,
  pick,
  omit,
  keys,
  values,
  entries,
  fromEntries,
  isPlainObject,
  isEmptyObject,
  get,
  set,
  has,
  flatten,
  unique,
  groupBy,
  sortBy,
  chunk,
  first,
  last,
  findIndex,
  find,
  remove,
  insert,
  move,
  swap,
  shuffle,
  sample,
  sampleSize,
  intersection,
  union,
  difference,
  isEmptyArray,
  sum,
  average,
  sumBy,
  arrayToTree,
  treeToArray,
  findInTree,
  filterTree,
};
