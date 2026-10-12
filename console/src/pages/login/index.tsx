import { zodResolver } from '@hookform/resolvers/zod';
import { Loader2, Lock, User } from 'lucide-react';
import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { useNavigate, useSearchParams } from 'react-router';
import { z } from 'zod';

import { codesApi, loginApi, userInfoApi } from '@/api/auth';
import { safeRedirect } from '@/api/request';
import { BrandMark } from '@/components/brand-mark';
import { Button } from '@/components/ui/button';
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { feedbackSuccess } from '@/lib/feedback';
import { HOME_PATH, useAuthStore } from '@/stores/auth';

const schema = z.object({
  username: z.string().min(1, '请输入用户名'),
  password: z.string().min(1, '请输入密码'),
});

type LoginValues = z.infer<typeof schema>;

/**
 * 登录页:全站唯一品牌位。
 * 表单在左、品牌面板在右(移动端品牌面板收起为顶部窄条)。
 * 账号/密码两个字段,无第三方/注册/忘记密码入口(契约见 design.md §7.1)。
 */
export default function LoginPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const token = useAuthStore((state) => state.token);

  const form = useForm<LoginValues>({
    resolver: zodResolver(schema),
    defaultValues: { username: '', password: '' },
  });

  // 已登录访问登录页 → 回控制台
  useEffect(() => {
    if (token) {
      navigate(HOME_PATH, { replace: true });
    }
  }, [token, navigate]);

  const onSubmit = async (values: LoginValues) => {
    try {
      const { accessToken } = await loginApi(values);
      useAuthStore.getState().setAuth(accessToken, values.username);
      // 登录后并行请求 user/info + auth/codes(codes 预期 [],失败不阻断)
      const [info] = await Promise.all([
        userInfoApi().catch(() => null),
        codesApi().catch(() => []),
      ]);
      if (info?.username) {
        useAuthStore.getState().setAuth(accessToken, info.username);
      }
      feedbackSuccess('登录成功');
      navigate(safeRedirect(searchParams.get('redirect')) || HOME_PATH, { replace: true });
    } catch {
      // 错误提示由响应拦截器统一弹出(sonner 挂在根部,登录页也收得到)
      // 焦点回到密码框,便于直接重输
      form.setFocus('password');
    }
  };

  return (
    <div className="grid min-h-svh lg:grid-cols-[1.6fr_1fr]">
      <div className="relative hidden flex-col justify-center gap-8 overflow-hidden bg-muted/40 p-16 lg:flex">
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 opacity-[0.35] [background-image:linear-gradient(to_right,var(--border)_1px,transparent_1px),linear-gradient(to_bottom,var(--border)_1px,transparent_1px)] [background-size:44px_44px]"
        />
        <div className="relative space-y-4">
          <div className="flex items-center gap-3">
            <BrandMark className="size-10" />
            <h1 className="text-4xl font-bold tracking-tight">rabbithole</h1>
          </div>
          <p className="max-w-md text-base leading-relaxed text-muted-foreground">
            自托管的端口转发与内网穿透平台。把公网的一个入口,变成通往内网每台机器的隧道。
          </p>
        </div>
        <div className="rh-mono relative text-[13px] leading-8 text-muted-foreground">
          <div>公网入口 0.0.0.0:20000 ──▶ 隧道 ──▶ 内网 10.0.0.1:22</div>
          <div>域名入口 app.example.com ──▶ 隧道 ──▶ 内网 10.0.0.2:8080</div>
          <div>密钥入口 visitor:6000 ──▶ 打洞直连 ──▶ 内网 10.0.0.3:5432</div>
        </div>
      </div>

      <div className="flex flex-col items-center justify-center gap-6 p-6">
        <div className="flex items-center gap-2 lg:hidden">
          <BrandMark className="size-8" />
          <span className="text-lg font-semibold">rabbithole</span>
        </div>

        <div className="w-full max-w-sm space-y-6">
          <div className="space-y-1.5 text-center lg:text-left">
            <h2 className="text-xl font-semibold tracking-tight">控制台</h2>
            <p className="text-sm text-muted-foreground">登录后管理客户端、隧道与证书</p>
          </div>

          <Form {...form}>
            <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
              <FormField
                control={form.control}
                name="username"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>用户名</FormLabel>
                    {/* FormControl 必须直接包住 input:它的 Slot 把 id/aria 挂到直接子元素上,
                        外面再套一层 div 会让 <label htmlFor> 指不到输入框 */}
                    <div className="relative">
                      <User
                        className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground"
                        aria-hidden="true"
                      />
                      <FormControl>
                        <Input
                          {...field}
                          autoComplete="username"
                          placeholder="用户名(默认 admin)"
                          className="pl-9"
                        />
                      </FormControl>
                    </div>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="password"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>密码</FormLabel>
                    <div className="relative">
                      <Lock
                        className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground"
                        aria-hidden="true"
                      />
                      <FormControl>
                        <Input
                          {...field}
                          type="password"
                          autoComplete="current-password"
                          placeholder="密码(首启随机密码见服务端日志)"
                          className="pl-9"
                        />
                      </FormControl>
                    </div>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <Button type="submit" className="w-full" disabled={form.formState.isSubmitting}>
                {form.formState.isSubmitting ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                ) : null}
                进入控制台
              </Button>
            </form>
          </Form>

          <p className="text-xs leading-relaxed text-muted-foreground">
            忘记密码?重启服务端并删除数据目录中的数据库可重新引导(数据将丢失)
          </p>
        </div>
      </div>
    </div>
  );
}
