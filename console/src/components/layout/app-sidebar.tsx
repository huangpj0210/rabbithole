import {
  ArrowRightLeft,
  Globe,
  KeyRound,
  LayoutDashboard,
  Radio,
  Server,
  Settings,
  ShieldCheck,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { NavLink, useLocation } from 'react-router';

import { BrandMark } from '@/components/brand-mark';
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from '@/components/ui/sidebar';
import { NAV_GROUPS, ROUTES } from '@/router/paths';

const ICONS: Record<string, LucideIcon> = {
  dashboard: LayoutDashboard,
  clients: Server,
  tcp: ArrowRightLeft,
  udp: Radio,
  domains: Globe,
  secret: KeyRound,
  certs: ShieldCheck,
  settings: Settings,
};

/** 侧栏:两组 + 设置,底部版本号;路径与旧版一致。 */
export function AppSidebar() {
  const { pathname } = useLocation();

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" asChild tooltip="rabbithole 控制台">
              {/*
                收起为图标态时只留品牌标记:文字块隐藏、按钮内容居中。
                size=lg 在收起态是 h-12 + p-0!,不隐藏文字会被 48px 的轨道裁切。
              */}
              <NavLink
                to={ROUTES.overview}
                className="group-data-[collapsible=icon]:justify-center"
              >
                {/*
                  尺寸容器:SidebarMenuButton 基础样式里的 [&>svg]:size-4 只作用于**直接子** svg,
                  直接放 BrandMark 会被压成 16px(展开态也偏小)。套一层 span 后按状态给尺寸:
                  展开态 28px;收起态按钮内框只有 16px(32 − p-2×2),故同步收到 16px。
                */}
                <span className="flex size-7 shrink-0 items-center justify-center group-data-[collapsible=icon]:size-4">
                  <BrandMark className="size-full" />
                </span>
                <div className="grid flex-1 text-left leading-tight group-data-[collapsible=icon]:hidden">
                  <span className="truncate font-semibold">rabbithole</span>
                  <span className="truncate text-xs text-muted-foreground">控制台</span>
                </div>
              </NavLink>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        {NAV_GROUPS.map((group, index) => (
          <SidebarGroup key={group.label ?? `group-${index}`}>
            {group.label ? <SidebarGroupLabel>{group.label}</SidebarGroupLabel> : null}
            <SidebarGroupContent>
              <SidebarMenu>
                {group.items.map((item) => {
                  const Icon = ICONS[item.icon] ?? LayoutDashboard;
                  const active = pathname === item.to || pathname.startsWith(`${item.to}/`);
                  return (
                    <SidebarMenuItem key={item.to}>
                      <SidebarMenuButton asChild isActive={active} tooltip={item.title}>
                        <NavLink to={item.to}>
                          <Icon className="size-4" />
                          <span>{item.title}</span>
                        </NavLink>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        ))}
      </SidebarContent>

      <SidebarFooter>
        <div className="px-2 pb-1 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
          rabbithole v{__APP_VERSION__}
        </div>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
