import { MoreHorizontal } from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';

/**
 * 行操作收敛为「⋯」下拉:列表行不再平铺三个按钮。
 *
 * 触发器只在**悬停该行**或键盘聚焦时显现(小屏常显,避免触屏无 hover 就摸不到);
 * 打开状态下也保持可见。`group/row` 由 DataTable 的行提供。
 */
export function RowActionsMenu({
  label,
  children,
  className,
}: {
  /** 可访问名,例如「smoke-client 的操作」。 */
  label: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label={label}
          className={cn(
            'opacity-0 transition-opacity group-hover/row:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 max-md:opacity-100',
            className,
          )}
        >
          <MoreHorizontal className="size-4" aria-hidden="true" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
