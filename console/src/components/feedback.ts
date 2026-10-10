/**
 * antd message 的模块级出口:axios 拦截器等 React 树外代码经此处弹提示,
 * 避免依赖 antd 静态方法(无法消费 App 上下文,主题/容器均不生效)。
 */
export interface FeedbackApi {
  error: (content: string) => void;
  success: (content: string) => void;
  warning: (content: string) => void;
}

let api: FeedbackApi | null = null;

export function bindFeedback(instance: FeedbackApi): void {
  api = instance;
}

export function feedbackError(content: string): void {
  if (api) {
    api.error(content);
  } else {
    console.error(content);
  }
}

export function feedbackSuccess(content: string): void {
  api?.success(content);
}

export function feedbackWarning(content: string): void {
  api?.warning(content);
}
