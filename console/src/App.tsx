import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useEffect } from 'react';
import { RouterProvider } from 'react-router';

import { Toaster } from '@/components/ui/sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { watchSystemTheme } from '@/lib/theme';
import { router } from '@/router';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
      staleTime: 10_000,
    },
  },
});

export function App() {
  // 主题模式为「跟随系统」时,系统切换要跟着变
  useEffect(() => watchSystemTheme(), []);

  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={200}>
        <RouterProvider router={router} />
        {/* Toaster 挂在根部、与路由无关:登录页等无布局页面也能收到拦截器报错(R33 的根因) */}
        <Toaster />
      </TooltipProvider>
    </QueryClientProvider>
  );
}
