const DEFAULT_CACHE_TIME = 60 * 60 * 24 * 7;

export const storage = {
  set(key: string, value: unknown, expire: number | null = DEFAULT_CACHE_TIME): void {
    const data = JSON.stringify({
      value,
      expire: expire !== null ? Date.now() + expire * 1000 : null,
    });
    localStorage.setItem(key, data);
  },

  get<T = unknown>(key: string, defaultValue: T | null = null): T | null {
    const item = localStorage.getItem(key);
    if (!item) return defaultValue;

    try {
      const data = JSON.parse(item);
      const { value, expire } = data;
      if (expire === null || expire >= Date.now()) {
        return value as T;
      }
      this.remove(key);
      return defaultValue;
    } catch {
      return defaultValue;
    }
  },

  remove(key: string): void {
    localStorage.removeItem(key);
  },

  clear(): void {
    localStorage.clear();
  },
};

export const sessionStorage = {
  set(key: string, value: unknown): void {
    const data = JSON.stringify(value);
    window.sessionStorage.setItem(key, data);
  },

  get<T = unknown>(key: string, defaultValue: T | null = null): T | null {
    const item = window.sessionStorage.getItem(key);
    if (!item) return defaultValue;

    try {
      return JSON.parse(item) as T;
    } catch {
      return defaultValue;
    }
  },

  remove(key: string): void {
    window.sessionStorage.removeItem(key);
  },

  clear(): void {
    window.sessionStorage.clear();
  },
};
