/**
 * 工具类统一导出
 * 注意：部分模块有同名导出函数，通过 default 导出的模块对象访问
 */

// Storage 工具
export { storage, sessionStorage } from './storage';

// 日期处理
export { default as dateHelper } from './dateHelper';

// 字符串处理
export { default as stringHelper } from './stringHelper';

// 数字处理
export { default as numberHelper } from './numberHelper';

// 对象/数组处理
export { default as objectHelper } from './objectHelper';

// 验证工具
export { default as validation } from './validation';

// URL 处理
export { default as urlHelper } from './urlHelper';

// 加密工具 (需要安装 crypto-js)

// ==================== 通用工具函数 ====================

/**
 * 防抖函数
 */
export function debounce<T extends (...args: unknown[]) => unknown>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return function (this: unknown, ...args: Parameters<T>) {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      fn.apply(this, args);
    }, delay);
  };
}

/**
 * 节流函数
 */
export function throttle<T extends (...args: unknown[]) => unknown>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let lastTime = 0;
  return function (this: unknown, ...args: Parameters<T>) {
    const now = Date.now();
    if (now - lastTime >= delay) {
      lastTime = now;
      fn.apply(this, args);
    }
  };
}

/**
 * 延迟执行
 */
export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * 复制文本到剪贴板
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard) {
      await navigator.clipboard.writeText(text);
      return true;
    }
    // 降级方案
    const textarea = document.createElement('textarea');
    textarea.value = text;
    textarea.style.position = 'fixed';
    textarea.style.opacity = '0';
    document.body.appendChild(textarea);
    textarea.select();
    const result = document.execCommand('copy');
    document.body.removeChild(textarea);
    return result;
  } catch {
    return false;
  }
}

/**
 * 从剪贴板读取文本
 */
export async function readFromClipboard(): Promise<string> {
  try {
    if (navigator.clipboard) {
      return await navigator.clipboard.readText();
    }
    return '';
  } catch {
    return '';
  }
}

/**
 * 获取浏览器类型
 */
export function getBrowserType(): 'chrome' | 'safari' | 'firefox' | 'edge' | 'ie' | 'opera' | 'unknown' {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('edg')) return 'edge';
  if (ua.includes('opr') || ua.includes('opera')) return 'opera';
  if (ua.includes('chrome')) return 'chrome';
  if (ua.includes('safari')) return 'safari';
  if (ua.includes('firefox')) return 'firefox';
  if (ua.includes('trident') || ua.includes('msie')) return 'ie';
  return 'unknown';
}

/**
 * 是否是移动设备
 */
export function isMobile(): boolean {
  return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
}

/**
 * 是否是 iOS 设备
 */
export function isIOS(): boolean {
  return /iPhone|iPad|iPod/i.test(navigator.userAgent);
}

/**
 * 是否是 Android 设备
 */
export function isAndroid(): boolean {
  return /Android/i.test(navigator.userAgent);
}

/**
 * 获取设备像素比
 */
export function getDevicePixelRatio(): number {
  return window.devicePixelRatio || 1;
}

/**
 * 判断元素是否在视口内
 */
export function isInViewport(element: HTMLElement): boolean {
  const rect = element.getBoundingClientRect();
  return (
    rect.top >= 0 &&
    rect.left >= 0 &&
    rect.bottom <= (window.innerHeight || document.documentElement.clientHeight) &&
    rect.right <= (window.innerWidth || document.documentElement.clientWidth)
  );
}

/**
 * 滚动到元素位置
 */
export function scrollToElement(
  elementOrSelector: HTMLElement | string,
  options?: ScrollIntoViewOptions
): void {
  const element =
    typeof elementOrSelector === 'string'
      ? document.querySelector(elementOrSelector)
      : elementOrSelector;

  if (element) {
    element.scrollIntoView({
      behavior: 'smooth',
      block: 'start',
      ...options,
    });
  }
}

/**
 * 滚动到页面顶部
 */
export function scrollToTop(smooth = true): void {
  window.scrollTo({
    top: 0,
    behavior: smooth ? 'smooth' : 'auto',
  });
}

/**
 * 获取滚动位置
 */
export function getScrollPosition(): { x: number; y: number } {
  return {
    x: window.pageXOffset || document.documentElement.scrollLeft,
    y: window.pageYOffset || document.documentElement.scrollTop,
  };
}

/**
 * 请求全屏
 */
export function requestFullScreen(element?: HTMLElement): void {
  const el = element || document.documentElement;
  if (el.requestFullscreen) {
    el.requestFullscreen();
  } else if ((el as any).webkitRequestFullscreen) {
    (el as any).webkitRequestFullscreen();
  } else if ((el as any).mozRequestFullScreen) {
    (el as any).mozRequestFullScreen();
  } else if ((el as any).msRequestFullscreen) {
    (el as any).msRequestFullscreen();
  }
}

/**
 * 退出全屏
 */
export function exitFullScreen(): void {
  if (document.exitFullscreen) {
    document.exitFullscreen();
  } else if ((document as any).webkitExitFullscreen) {
    (document as any).webkitExitFullscreen();
  } else if ((document as any).mozCancelFullScreen) {
    (document as any).mozCancelFullScreen();
  } else if ((document as any).msExitFullscreen) {
    (document as any).msExitFullscreen();
  }
}

/**
 * 是否是全屏状态
 */
export function isFullScreen(): boolean {
  return !!(
    document.fullscreenElement ||
    (document as any).webkitFullscreenElement ||
    (document as any).mozFullScreenElement ||
    (document as any).msFullscreenElement
  );
}
