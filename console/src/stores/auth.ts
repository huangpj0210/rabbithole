import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/** 认证状态:仅 token + 用户名(单管理员控制台,无角色体系)。 */
export interface AuthState {
  token: null | string;
  username: null | string;
  homePath: string;
  setAuth: (token: string, username: string) => void;
  clear: () => void;
}

export const HOME_PATH = '/rabbithole/overview';

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      token: null,
      username: null,
      homePath: HOME_PATH,
      setAuth: (token, username) => set({ token, username }),
      clear: () => set({ token: null, username: null }),
    }),
    { name: 'rabbithole-console-auth' },
  ),
);
