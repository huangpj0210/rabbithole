import { MoreHorizontal } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';

/** 鼠标离开后延迟关闭,给「从 ⋯ 移向菜单内容」留出时间,避免闪关。 */
const CLOSE_DELAY_MS = 180;

/**
 * 行操作的「⋯」:列表行超过 2 个动作时,主操作留在主位,其余收进这里。
 *
 * 交互:
 * - 触发器只在**悬停该行**或键盘聚焦时显现(小屏常显,触屏没有 hover);
 * - **悬停触发器即弹出**菜单,不用点(Radix 的 DropdownMenu 本身只支持点击,
 *   这里用受控 open + 指针事件实现;键盘仍可 Tab 到 ⋯ 后按 Enter/Space);
 * - 菜单内容也参与「离开才关」的判定,鼠标移进菜单不会把它关掉。
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
  const [open, setOpen] = useState(false);
  // 悬停已自行开合时,Radix 的点击 toggle 不应把它关掉
  const hoveringRef = useRef(false);
  const closeTimerRef = useRef<number | null>(null);

  const cancelClose = () => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  };

  const scheduleClose = () => {
    cancelClose();
    closeTimerRef.current = window.setTimeout(() => setOpen(false), CLOSE_DELAY_MS);
  };

  useEffect(() => cancelClose, []);

  return (
    <DropdownMenu
      open={open}
      onOpenChange={(next) => {
        if (hoveringRef.current) {
          return;
        }
        setOpen(next);
      }}
    >
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
          onPointerEnter={(event) => {
            if (event.pointerType === 'touch') return;
            hoveringRef.current = true;
            cancelClose();
            setOpen(true);
          }}
          onPointerLeave={(event) => {
            if (event.pointerType === 'touch') return;
            hoveringRef.current = false;
            scheduleClose();
          }}
        >
          <MoreHorizontal className="size-4" aria-hidden="true" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className="w-48"
        onPointerEnter={cancelClose}
        onPointerLeave={scheduleClose}
      >
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
