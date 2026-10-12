import { LogOut } from 'lucide-react';
import { useState } from 'react';
import { Outlet, useLocation, useNavigate } from 'react-router';

import { CommandPalette, CommandPaletteTrigger } from '@/components/command-palette';
import { AppSidebar } from '@/components/layout/app-sidebar';
import { ThemeToggle } from '@/components/theme-toggle';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbList,
  BreadcrumbPage,
} from '@/components/ui/breadcrumb';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Separator } from '@/components/ui/separator';
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar';
import { logoutApi } from '@/api/auth';
import { feedbackSuccess } from '@/lib/feedback';
import { NAV_GROUPS, ROUTES } from '@/router/paths';
import { useAuthStore } from '@/stores/auth';

function currentTitle(pathname: string): string {
  for (const group of NAV_GROUPS) {
    for (const item of group.items) {
      if (pathname === item.to || pathname.startsWith(`${item.to}/`)) {
        return item.title;
      }
    }
  }
  return '控制台';
}

function UserMenu() {
  const username = useAuthStore((state) => state.username);
  const navigate = useNavigate();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [pending, setPending] = useState(false);

  const doLogout = async () => {
    setPending(true);
    try {
      // 必须经带拦截器的实例:后端校验 Bearer 通过才落 logout 审计事件
      await logoutApi().catch(() => undefined);
    } finally {
      useAuthStore.getState().clear();
      setPending(false);
      setConfirmOpen(false);
      feedbackSuccess('已退出登录');
      navigate(ROUTES.login, { replace: true });
    }
  };

  const name = username ?? 'admin';

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon" aria-label={`账户:${name}`}>
            <Avatar className="size-7">
              <AvatarFallback className="text-xs">{name.slice(0, 1).toUpperCase()}</AvatarFallback>
            </Avatar>
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-44">
          <DropdownMenuLabel className="truncate">{name}</DropdownMenuLabel>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setConfirmOpen(true)}>
            <LogOut className="size-4" />
            退出登录
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <AlertDialog open={confirmOpen} onOpenChange={(next) => !pending && setConfirmOpen(next)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>退出登录?</AlertDialogTitle>
            <AlertDialogDescription>
              退出后当前会话立即失效,需要重新输入账号密码。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={pending}>取消</AlertDialogCancel>
            <AlertDialogAction
              disabled={pending}
              onClick={(event) => {
                event.preventDefault();
                void doLogout();
              }}
            >
              {pending ? '退出中…' : '退出登录'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

/** 应用外壳:shadcn sidebar(collapsible=icon)+ 顶栏;移动端自动抽屉。 */
export function ConsoleLayout() {
  const { pathname } = useLocation();

  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger className="-ml-1" />
          <Separator orientation="vertical" className="mr-1 h-4" />
          <Breadcrumb className="min-w-0">
            <BreadcrumbList>
              <BreadcrumbItem>
                <BreadcrumbPage className="truncate">{currentTitle(pathname)}</BreadcrumbPage>
              </BreadcrumbItem>
            </BreadcrumbList>
          </Breadcrumb>
          <div className="ml-auto flex items-center gap-1.5">
            <CommandPaletteTrigger />
            <ThemeToggle />
            <UserMenu />
          </div>
        </header>
        <div className="flex min-w-0 flex-1 flex-col gap-6 p-4 md:p-6">
          <Outlet />
        </div>
      </SidebarInset>
      <CommandPalette />
    </SidebarProvider>
  );
}
