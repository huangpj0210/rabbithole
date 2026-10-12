import { Info } from 'lucide-react';
import { useRef, useState } from 'react';

import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { fieldInfo } from '@/features/field-info';
import type { FieldInfoKey } from '@/features/field-info';
import { cn } from '@/lib/utils';

/**
 * 带说明的名词:名词 + ℹ 图标,悬停(桌面)或点按(触屏)弹出该项的功能说明,
 * 替代「括号补充说明」式文案 —— 界面只留名词本身。
 *
 * 说明文案不在本组件里,而是按 key 取自 @/features/field-info(旧版 89 条文案逐字平移)。
 * 不用 Radix Tooltip:它显式忽略 touch 指针,触屏点按不会弹出。
 */
export function InfoLabel({
  title,
  infoKey,
  info,
  iconOnly = false,
  className,
}: {
  title: React.ReactNode;
  infoKey?: FieldInfoKey;
  info?: React.ReactNode;
  /** 只渲染 ℹ 按钮(名词文字由外部已有元素承担时用,避免重复与非法嵌套)。 */
  iconOnly?: boolean;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  // 悬停已由指针事件自行开合;此时 Radix 的点击 toggle 不应把浮层关掉
  const hoveringRef = useRef(false);

  const content = infoKey ? fieldInfo(infoKey) : info;
  const label = typeof title === 'string' ? title : '该项';

  if (!content) {
    return iconOnly ? null : <span className={className}>{title}</span>;
  }

  return (
    <span className={cn('inline-flex items-center gap-1', className)}>
      {iconOnly ? null : <span>{title}</span>}
      <Popover
        open={open}
        onOpenChange={(next) => {
          if (hoveringRef.current) {
            return;
          }
          setOpen(next);
        }}
      >
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label={`${label} 说明`}
            // p-1.5 + -m-1.5:图标视觉尺寸不变,但命中区从 14×14 撑到 26×26,
            // 满足 WCAG 2.2 AA 的 24×24 最小目标(纯图标按钮在触屏上尤其需要)
            className="-m-1.5 inline-flex shrink-0 cursor-help items-center rounded-sm p-1.5 text-muted-foreground transition-colors hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
            onPointerEnter={(event) => {
              if (event.pointerType === 'touch') return;
              hoveringRef.current = true;
              setOpen(true);
            }}
            onPointerLeave={(event) => {
              if (event.pointerType === 'touch') return;
              hoveringRef.current = false;
              setOpen(false);
            }}
            // 刻意不在 onFocus 里自动弹出:Radix Dialog 打开时会自动聚焦首个可聚焦元素
            // (往往就是这个 ℹ 按钮),那样会一开弹窗就弹出第一个字段的说明浮层,
            // 并且第一次 Esc 只关掉浮层。键盘用户 Tab 到 ℹ 后按 Enter/Space 触发原生 click 即可。
            onBlur={() => {
              hoveringRef.current = false;
              setOpen(false);
            }}
          >
            <Info className="size-3.5" aria-hidden="true" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          side="top"
          align="start"
          className="max-w-72 text-xs leading-relaxed text-popover-foreground"
          onOpenAutoFocus={(event) => event.preventDefault()}
        >
          {content}
        </PopoverContent>
      </Popover>
    </span>
  );
}
