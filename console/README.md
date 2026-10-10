# rabbithole 控制台(console/)

rabbithole 管理台前端,基于 [ant-design-admin-v6](https://github.com/arvinaij/ant-design-admin-v6)(MIT)模板裁剪:
**Umi Max 4 + React 19 + Ant Design v6 + ProComponents**,数据层 TanStack Query + axios + zustand,图表 recharts。

## 开发

```bash
pnpm install          # Node >= 20, pnpm(packageManager 已声明,corepack 可自动切换)
pnpm dev              # http://localhost:8000,/api 代理到 127.0.0.1:8080(deploy 示例 web 端口,config/proxy.ts 可改)
pnpm typecheck        # tsc --noEmit
pnpm build            # 清空 dist 后 max build(防旧 hash chunk 混载,勿跳过清理)
pnpm preview          # 本地预览生产构建
```

后端联调:`target/debug/rh-server --config <你的 server.toml>`(web 端口默认 8080,改 proxy.ts 指向它)。

## 被 rh-server 托管的两种方式

1. **内嵌(推荐,单二进制)**:`pnpm build` 后重编 `cargo build -p rh-server`。
   `build.rs` 双路径检测:**优先 `console/dist`**,回退旧 `web/apps/web-antd/dist`;
   构建日志出现 `embedding web console from .../console/dist` 即内嵌成功。
   运行时未配置 `[web].static_dir` 时自动启用内嵌 UI(SPA 深链回退 200、未知 `/api/*` JSON 404)。
2. **static_dir 显式指定**:`server.toml` 中 `[web] static_dir = "/path/to/console/dist"`,
   显式配置优先于内嵌。

注意:debug 构建的 rust-embed 从 `console/dist` **运行时读盘**,改 dist 无需重编;
release 构建编译期嵌入,改 dist 后需重编。

## 目录结构

```
config/            Umi 配置:routes.ts(§8.0 八菜单 + 旧路由重定向)、
                   defaultSettings.ts(ProLayout)、proxy.ts(dev 代理)
src/
  api/             request.ts(axios 封装:envelope code===0 取 data、业务错弹
                   中文 message、HTTP 401 清 token 回跳登录)+ rabbithole.ts
                   (业务接口,契约与后端 §7 零变更)+ auth.ts(登录链路)
  stores/auth.ts   zustand:token/username,localStorage 持久化
  wrappers/auth    路由级登录守卫(注意:umi rr6 模式 wrapper 须渲染 <Outlet/>)
  layouts 由模板 plugin-layout(app.tsx 运行时配置)+ TopHeader/TagsView 提供
  components/      ProxyListPage(参数化类型页)、ProxyFormModal(§8.1 字段矩阵
                   + 插件子表单)、VisitorsPanel(访问器)、SecretModal(一次性
                   密钥 + rh-client.toml 接入模板)、HeaderPairsInput、feedback
  pages/           overview(卡片+流量速率图+事件)/ clients / domains / tcp /
                   udp / secret(规则+访问器双 tab)/ certs(M3 容错)/ settings
```

## 已知约定

- antd v6 表单统一走 `initialValues + form.submit() + onFinish`;不要用
  `setFieldsValue(未注册字段)` + `validateFields()` 组合(在本表单规模下
  validateFields 会挂起,详见 ProxyFormModal 演进史)。
- Modal 表单每次打开以 `key` 强制重挂载,避免 rc-select/rc-input-number 显示缓存。
- build 脚本内建 `rm -rf dist`:增量构建残留旧 hash chunk 会被浏览器混载,
  导致 React 事件委托失效一类极难排查的问题。
- 依赖后端未落地字段的 UI 均按容错降级:计量/连接列仅在 API 返回时渲染,
  证书页在 M3 前显示空态,配额字段提交后由后端忽略。
- 模板裁剪记录:demo 页面、mock、PWA、多余 locale(仅留 zh-CN/en-US)、
  plots/L7 地图、jest/husky/commitlint 均已移除;LICENSE 保留(MIT)。
