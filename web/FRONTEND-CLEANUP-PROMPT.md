# 任务：清理 rabbithole 前端中与项目无关的模板残留（vue-vben-admin 演示代码）

你是一名前端工程师。仓库根目录：`/Users/yii/Documents/mycode/rabbithole`。
前端工作区在 `web/`（vue-vben-admin 裁剪版 monorepo，基线 commit `d27a5b8a`，唯一应用是 `apps/web-antd`；
背景见 `web/FRONTEND-NOTES.md`）。后端是 Rust（`crates/`），**本次任务只改前端，绝对不要动 Rust 代码与任何 API 契约**。

`web/` 是从 vue-vben-admin 模板裁剪而来的，模板自带的演示页面、演示数据、演示路由、品牌残留仍然留在应用里，
并且**真的显示在界面上**（侧边栏有「概览/分析页/工作台」，登录页有「忘记密码/注册/扫码登录/手机号登录/第三方登录」，
右上角有假通知红点，登录页页脚有 Vben 版权与 ICP 备案号，生产构建会加载百度统计脚本）。
请按下面的清单清理，分 3 个阶段执行，**每个阶段结束都要跑门禁命令**（见文末），全部通过才算完成。

**开工前先做一件小事（P0，2 分钟）**：修掉 `web/` 下 `pnpm install --frozen-lockfile` 的既有 exit 1 缺陷
（`pnpm-workspace.yaml` 的 `allowBuilds.lefthook` 是占位符字符串）。它是 CI frontend job 的第一步，
不修的话「门禁全绿」无从谈起。详见文末「既有缺陷」一节。

---

## 0. 先做一次现状核对（不要跳过）

在 `web/` 下执行以下检查，确认清单与当前代码一致；若某项结果与"预期"不符，说明代码已变，
跳过对应项并在最终报告里说明。

```bash
cd /Users/yii/Documents/mycode/rabbithole/web

# 1) 应用内所有页面/路由
ls apps/web-antd/src/views apps/web-antd/src/views/_core apps/web-antd/src/router/routes/modules

# 2) 逐项确认"孤儿页"（各自预期见下）
#    2a. demos / about 是彻底孤儿 —— 预期：无输出
grep -rn "views/demos\|demos/antd" apps/web-antd/src --include="*.ts" --include="*.vue" | grep -v "^apps/web-antd/src/views/demos"
grep -rn "_core/about" apps/web-antd/src --include="*.ts" --include="*.vue" | grep -v "^apps/web-antd/src/views/_core/about"
#    2b. profile 只被一个坏引用指向（按路由名）—— 预期：恰好 1 行，来自 layouts/basic.vue 的 router.push({name:'Profile'})
grep -rn "'Profile'" apps/web-antd/src --include="*.ts" --include="*.vue"
#    2c. dashboard 与 4 个登录变体页仍被"同样要删的路由文件"引用 —— 预期：命中 router/routes/modules/dashboard.ts 与 router/routes/core.ts
grep -rn "views/dashboard" apps/web-antd/src --include="*.ts" --include="*.vue" | grep -v "^apps/web-antd/src/views/dashboard"
grep -rn "code-login\|qrcode-login\|forget-password\|register" apps/web-antd/src/router --include="*.ts"

# 3) 确认 vxe adapter 无人 import（预期：无输出）
grep -rn "adapter/vxe-table" apps/web-antd/src

# 4) 确认 demos 命名空间无人使用（预期：无输出）
grep -rn "demos\." apps/web-antd/src --include="*.ts" --include="*.vue"

# 5) 确认 framework 层待删项零引用（预期：各自只命中定义处）
grep -rn "plugins/echarts" apps packages --include="*.ts" --include="*.vue" | grep -v node_modules   # 预期：仅 views/dashboard/analytics/*
grep -rn "<AccessControl" apps packages --include="*.vue" | grep -v node_modules                    # 预期：无输出
grep -rn "resetRoutes" apps packages --include="*.ts" | grep -v node_modules                        # 预期：仅 router/index.ts 自身
```

**已实测基线（用于对照）**：`views/demos`、`_core/about` 确认零外部引用；
`_core/profile` 仅剩 `layouts/basic.vue` 的坏引用；`dashboard` 与 4 个登录变体页仅被将删的路由文件引用。

另外记录两个基线事实，用于收尾对比：

- `pnpm run check:dep`（knip）**当前**就报出这些未使用依赖（改造前的既有问题，不是本次引入）：
  根 `package.json` 的 `sortablejs`、`@changesets/changelog-github`、`eslint`、`is-ci`、`lefthook`；
  `packages/effects/plugins` 的 `@tiptap/extension-document`、`@tiptap/extension-underline`；
  `internal/vite-config` 的 `resolve.exports`、`dayjs`；`internal/node-utils` 的 `find-up`。
  阶段 3 若删包，这个清单应相应变短。
- **`apps/web-antd/dist/` 是「当前未清理源码」的构建产物**（构建时间晚于 src 最后改动，不是陈旧构建）：
  其中包含 `js/demos-*.js`（模板演示页 + `demos.json` 文案）、`js/vben-*.js`（`@vben/constants` 的
  `doc.vben.pro`/`github.com/vbenjs` 常量）、`js/about-*.js`、`js/profile-*.js`、`js/analytics-*.js`、
  `js/workspace-*.js`；`index.html` 标题与加载页文字仍是 "Vben Admin Antd"，百度统计脚本仍在，
  入口 JS 带 "Vben Admin" 版权头。README 的生产模式要求把 `[web] static_dir` 指向该目录
  （当前 `deploy/rh-server.toml` 里该行是注释状态，所以是否正在被服务取决于部署配置）。
  **本次改造结束后必须重新 `pnpm run build:antd` 覆盖 dist**（`VITE_ARCHIVER=true` 会同步产出 `dist.zip`），
  这是本任务的实际交付物之一。
  补充机制：`src/router/access.ts` 用 `import.meta.glob('../views/**/*.vue')` 收集所有视图，
  所以**只要视图文件还在磁盘上，即使没有任何路由引用它，构建也会为它产出 chunk**——
  删文件才是真正让演示代码离开产物（以及让下载体积变小）的办法。

---

## 阶段 1：删除无关页面、路由、演示数据与品牌残留（必做，收益最高）

### 1.1 删除文件（应用层，`web/apps/web-antd/src/`）

| 路径 | 为什么无关 |
|---|---|
| `views/dashboard/`（整个目录，analytics 6 个文件 + workspace 1 个） | 模板「分析页/工作台」演示，全是假数据；被 `router/routes/modules/dashboard.ts` 挂进侧边栏，且 analytics 的 `meta.affixTab: true` 会让登录后标签栏**固定一个「分析页」标签** |
| `views/demos/`（整个目录） | 模板 Ant Design Vue 组件演示页（其路由文件上游已删，页面已无引用） |
| `views/_core/about/` | 模板「关于」页（上游路由 `vben.ts` 已删，页面已无引用） |
| `views/_core/profile/`（整个目录，5 个文件） | 模板「个人中心」；路由已随上游 `vben.ts` 删除，`layouts/basic.vue` 里 `router.push({ name: 'Profile' })` 是**坏引用**（点击静默失败） |
| `views/_core/authentication/code-login.vue` | 手机号验证码登录演示页，提交处理是空函数（`void values`），后端无对应接口 |
| `views/_core/authentication/qrcode-login.vue` | 扫码登录演示页，同上 |
| `views/_core/authentication/forget-password.vue` | 忘记密码演示页，同上 |
| `views/_core/authentication/register.vue` | 注册演示页，同上 |
| `views/_core/fallback/coming-soon.vue` | 演示兜底页，全仓无引用 |
| `views/_core/fallback/offline.vue` | 演示兜底页，全仓无引用 |
| `views/_core/fallback/internal-error.vue` | 演示兜底页，全仓无引用 |
| `router/routes/modules/dashboard.ts` | 只为上面的 dashboard 演示页服务 |
| `locales/langs/zh-CN/demos.json`、`locales/langs/en-US/demos.json` | `demos` 命名空间在删除路由后无任何消费者 |
| `adapter/vxe-table.ts` | 无人 import（`grep -rn "adapter/vxe-table"` 为空）；vxe 表格在本项目页面中从未使用 |
| `CHANGELOG.md` | 上游 vben 的变更日志，与本项目无关 |
| `src/locales/README.md`、`src/views/_core/README.md`（可选） | 上游框架说明文档 |
| `packages/effects/common-ui/src/ui/fallback/icons/warning.svg`（阶段 3 候选） | 全仓零引用（`fallback.vue` 只 import 5 个 `.vue` 图标），孤儿资产 |

**必须保留**（不要误删）：
- `views/_core/fallback/not-found.vue`（404 兜底，`router/routes/core.ts` 引用）
- `views/_core/fallback/forbidden.vue`（403，`router/access.ts` 的 `forbiddenComponent` 引用）
- `views/_core/authentication/login.vue`（本项目登录页）
- `views/rabbithole/**`（本项目全部业务页：overview/clients/services/visitors/settings）

保留的两个 fallback 页组件名仍带模板痕迹（`defineOptions({ name: 'Fallback404Demo' })`、`Fallback403Demo`），
可顺手改名为 `FallbackNotFound` / `FallbackForbidden`——纯内部名，无引用方，改不改不影响功能。

### 1.1b 收益量化（已实测，用于排优先级）

删除上面这些文件后，构建产物会显著变小。以当前 dist（4.3 MB，其中 `js/` 4.1 MB、217 个 chunk）为基线，
已确认可直接消失的部分：

| 内容 | 证据 | 量级 |
|---|---|---|
| **echarts 库** | `dist/js/use-echarts-*.js` 674 KB，内含 `zrender`；唯一引用者是 `views/dashboard/analytics/*`（`grep -rn "plugins/echarts"` 除 dashboard 外无命中） | **674 KB ≈ js 总量的 16%** |
| vben 的 16 个内联 SVG 图标（含 4 个 20–32 KB 头像） | `packages/icons/src/svg/load.ts` 用 `import.meta.glob('./icons/**', {eager:true, query:'?raw'})` **无条件全量加载**，与是否被引用无关 | `src-DQ7FrfW7.js` 128 KB（含全部 svg） |
| 演示页/演示组件 chunk | `demos-*.js`、`analytics-*.js`、`workspace-*.js`、`about-*.js`、`profile-*.js`、`code-login-*.js`、`qrcode-login-*.js`、`register-*.js`、`forget-password-*.js`、`password-setting-*.js`、`antd-*.js`（demos 页）等 | 数十个 chunk |
| zh-TW 语言包 | `common-BdmafBf4.js`(0.9 KB)、`authentication-DaSE31EI.js`(3.4 KB)、`preferences-D4_rDnaP.js`(7.7 KB)、`ui-9JFOdkA_.js`(4.4 KB)，合计约 16.5 KB（内容为繁体文案，运行时永远不会被加载） | ~16 KB |

所以阶段 1 不只是"清理门面"，是实打实的产物体积优化——**但要记住"删文件才有效"**：
只改路由/偏好不会让这些 chunk 消失（见 §0 的 `import.meta.glob` 机制说明）。改完务必重新构建并对比
`du -sh dist/js`。

**关于 vben CSS 类名（`vben-menu`/`vben-scrollbar` 等 122 处 scss 引用 + 运行时 `useNamespace('vben')`）**：
这是框架的 BEM 命名空间（`packages/@core/base/shared/src/constants/globals.ts` 的 `DEFAULT_NAMESPACE = 'vben'`），
贯穿 `packages/@core/ui-kit/**` 与 `packages/effects/**` 的样式与模板。**本次任务不要改**——
改它要动几十个文件且极易漏，属于上游框架的命名约定，不影响用户体验。若确需改名，单独立项。

### 1.2 修改路由

`src/router/routes/core.ts`：删除 `Authentication` 路由下 `code-login`、`qrcode-login`、`forget-password`、`register`
四条子路由（连同它们的 `$t('page.auth.xxx')` title），只保留 `login`。

### 1.3 修登录页（关闭并移除演示入口）

`src/views/_core/authentication/login.vue`：
1. 给 `<AuthenticationLogin>` 显式传参，关闭演示入口（组件默认全是 `true`，所以当前登录页真的在渲染这些死链）：
   ```html
   <AuthenticationLogin
     :form-schema="formSchema"
     :loading="authStore.loginLoading"
     :show-code-login="false"
     :show-qrcode-login="false"
     :show-forget-password="false"
     :show-register="false"
     :show-third-party-login="false"
     @submit="authStore.authLogin"
   />
   ```
   （`showRememberMe` 是真实功能，保留默认值即可。）
2. 删除死代码：`MOCK_USER_OPTIONS` 常量，以及 `username` 字段里引用不存在字段 `selectAccount` 的
   `dependencies.trigger / triggerFields`（`selectAccount` 字段本身在上游就被删了，这段 trigger 永远不触发）；
   随后清理不再使用的 `BasicOption` import。
   （`login.vue` 已相对上游删掉了 `VbenSelect` 快速选号字段与 `SliderCaptcha` 滑块验证码，
   所以 `authentication.selectAccount` / `authentication.verifyRequiredTip` 两个 locale key
   在本仓也只剩核心包里的定义、无消费者——见阶段 3 的 locale 清理项。）

### 1.4 修基础布局（假通知 + Vben 品牌菜单）

`src/layouts/basic.vue`：
1. 删除硬编码演示通知：`notifications` 数组（6 条假数据：「收到了 14 份新周报」等）以及只服务于它的
   `showDot`、`handleNoticeClear`、`markRead`、`remove`、`handleMakeAll`、`viewAll`、`handleClick`、`navigateTo`，
   和模板里的 `<template #notification>...</template>` 整块。
2. `menus` 计算属性：删除「个人中心」（`Profile` 路由已不存在）、「文档」（`VBEN_DOC_URL`）、「GitHub」
   （`VBEN_GITHUB_URL`）、「问答」（`VBEN_GITHUB_URL/issues`）四项——全部是 vben 品牌/坏链接。
   清理后 `menus` 变空数组，直接传 `:menus="[]"` 或移除该 prop 即可。
   ⚠️ **注意**：`UserDropdown` 的下拉里**默认并没有退出登录项**——`preferences.widget.logoutButtonPosition`
   默认是 `'header'`，所以退出按钮在**顶栏**（`header.vue` 的 `logout-btn` 插槽）。下拉里的退出项
   只在 `logoutButtonPosition === 'user-dropdown'` 时出现。所以删掉 `menus` 的四个品牌项后，
   用户下拉会只剩「偏好设置」（若 `preferencesButtonPosition` 解析到 `user-dropdown`）或完全空；
   **退出登录始终在顶栏可用，不会因此丢失**——但要在浏览器冒烟里确认这一点（见文末冒烟第 4 条）。
3. `<UserDropdown>` 删除 `description="ann.vben@gmail.com"` 与 `tag-text="Pro"`。
4. 清理因此不再使用的 import（`VBEN_DOC_URL`、`VBEN_GITHUB_URL`、`BookOpenText`、`CircleHelp`、`SvgGithubIcon`、
   `openWindow`、`NotificationItem`、`Notification` 等）——以 lint/typecheck 报错为准逐个删。
5. 两个「挂着但永不触发」的功能，二选一处理，不要两不管地留着：
   - `<template #lock-screen>` + `LockScreen`：只在 `preferences.widget.lockScreen` 为真且
     `accessStore.isLockScreen` 时渲染。若 1.5 里把 `widget.lockScreen` 关掉，就一并删掉这块模板与 import；
     若想保留锁屏功能，就保持默认 `true` 不动。
   - `<template #extra>` 里的 `AuthenticationLoginExpiredModal` + `LoginForm` import：
     只在 `preferences.app.loginExpiredMode === 'modal'` 时打开（见 `src/api/request.ts` 的 `doReAuthenticate`），
     默认值是 `'page'`（401 直接 logout 回登录页），弹窗永不出现。**要么**在 `src/preferences.ts` 显式设
     `app: { loginExpiredMode: 'modal' }`（推荐：会话过期就地重登，不丢当前页），**要么**删掉这块模板与
     `LoginForm` import。

### 1.5 品牌与配置残留

1. `src/preferences.ts`：
   - **删除 `preferencesExtension`**（`tenantMode/reportTitle/defaultTableSize/enableFormFullscreen` 是模板的
     「业务偏好」演示面板，本项目没有任何代码消费它们）；同步修改 `src/main.ts` 的 `initPreferences({...})`，
     去掉 `extension: preferencesExtension` 传参（`initPreferences` 允许不传）。
   - `copyright: appCopyrightPreferences` → 当前会在登录页渲染「Copyright © 2024 Vben 闽ICP备19024351号」并链接
     工信部。改为 `copyright: { enable: false }`，或替换为 rabbithole 自己的信息（`appCopyrightPreferences`
     来自 `@vben/preferences`，不要再 import 它）。
   - 追加 widget 开关，关闭与本项目无关的头部挂件（当前**真的显示**）：
     ```ts
     widget: {
       globalSearch: false,   // 全站菜单搜索，只有 5 个页面，无意义
       notification: false,   // 假通知铃铛（1.4 已删数据）
       timezone: false,       // 时区切换
       lockScreen: false,     // 锁屏（如你认为有用可保留，保留则 basic.vue 的 LockScreen 也别删）
     },
     ```
     说明：这些是框架偏好项，通过 override 关闭即可，**不要改 `packages/**` 的默认配置**。
     关闭 `notification` 后，1.4 中删除的 `#notification` slot 就不会有兜底铃铛出现。
   - 可选：`logo.source`（默认指向 `unpkg.com/@vbenjs/static-source` 的 vben logo）与 `app.defaultAvatar`
     （vben 头像 URL）换成自有资源，或设 `logo: { enable: false }`（侧边栏只显示应用名文字）。
2. `apps/web-antd/.env`：`VITE_APP_TITLE=Vben Admin Antd` → 改成项目名（如 `Rabbithole 控制台`）。
   该值决定浏览器标题、加载页文字、侧边栏 logo 旁文字。**不要改** `VITE_APP_NAMESPACE` 的语义
   （改了会清空用户本地偏好缓存，保持原值即可）。
   ⚠️ 同理，**不要改 `apps/web-antd/package.json` 的 `version`**：`main.ts` 的命名空间是
   `${VITE_APP_NAMESPACE}-${VITE_APP_VERSION}-${env}`，而 `VITE_APP_VERSION` 由
   `viteMetadataPlugin` 从该 package.json 的 `version` 注入（`inject-metadata.ts`）——
   改版本号会让所有用户的偏好/令牌缓存前缀变化（等同于强制重置）。改 `author/homepage` 是安全的。
3. `apps/web-antd/index.html`：
   - **删除百度统计脚本**（`hm.baidu.com/hm.js?b38e...` 那段 `<script>`；生产构建里 `_VBEN_ADMIN_PRO_APP_CONF_`
     确实存在，所以它真的会加载，自托管产品必须去掉）。
   - 更新 `<meta name="description">`、`keywords`、`author`（当前是 "A Modern Back-end Management System" /
     "Vben Admin Vue3 Vite" / "Vben"）。
   - 顶部 `window.__bootErrs` 调试采集脚本是此前排查「生产构建空白视图」时加的诊断代码；可一并删除，
     但**删完必须完成文末的构建+冒烟验证**；若想保守就保留并加一行注释说明用途。
4. `src/locales/langs/zh-CN/page.json`、`en-US/page.json`：删除 `dashboard` 整段与 `auth.profile`，
   只保留 `auth.login`（仍被登录路由 title 使用）。
5. `public/favicon.ico`：**当前是上游 vben 的图标**（与上游仓库字节一致），浏览器标签页显示的是它。
   换成项目自己的图标（或至少放一个自有占位图）。
6. `apps/web-antd/vite.config.ts`：在 `application: {}` 里加 `license: false`。
   原因：生产构建时 `viteLicensePlugin`（`internal/vite-config`）会把
   `Vben Admin / Author: vben / Copyright (C) 2024 Vben / Contact: ann.vben@gmail.com`
   版权头注入每个入口 chunk（已确认存在于当前 dist 的 `jse/*.js`），是发布物里的真实品牌残留。
   若不想动 vite.config，替代方案是把 **`apps/web-antd/package.json`** 的 `author/homepage/description`
   改成自有信息（该插件 `readPackageJSON(process.cwd())`，构建时 cwd 即应用目录，所以读的是它，
   不是 `web/package.json`）。注意 `license` 是 `internal/vite-config` 的现成开关，**不要改插件源码**。
7. `apps/web-antd/.env` 的 `VITE_APP_STORE_SECURE_KEY=please-replace-me-with-your-own-key`：
   这是 SecureLS 加密 pinia 持久化存储（生产环境 localStorage）的密钥，**当前是公开的占位值**，
   等于没有加密。换成随机值（`openssl rand -hex 32`）。注意：换 key 会让旧浏览器缓存的加密数据无法解密，
   需在发布说明里提示用户重新登录/清缓存（对内部工具可接受）。
8. 小项：`vite.config.ts` 的 `application` 里可加 `printInfoMap: {}`，去掉 dev server 启动时打印的
   `Vben Admin Docs: https://doc.vben.pro` 一行（dev-only，不影响产物）。

### 1.6 登录页品牌（logo / 文案 / 插画）

当前登录页（`authPageLayout: 'panel-right'` 默认）在**左/右侧面板**显示 vben 的插画
（`packages/effects/layouts/src/authentication/icons/slogan.vue`）与 vben 官网营销文案
（`authentication.pageTitle` = "开箱即用的大型中后台管理系统"、`pageDesc` = "工程化、高性能、跨组件库的前端模版"），
在**左上角**显示 `preferences.logo.source`（指向 `unpkg.com/@vbenjs/static-source` 的 vben logo 图片）。
注意：`apps/web-antd/src/layouts/auth.vue` 是**无条件**把 `preferences.logo.source` 传给 `AuthPageLayout` 的，
不受 `preferences.logo.enable` 控制，所以只改偏好开关删不掉它。处理：

1. `src/layouts/auth.vue`：删除 `logo`/`logoDark` 两个 computed 与对应 prop 传递（不再显示 vben logo），
   或改为传自有 logo 资源路径。
2. 新增 `src/locales/langs/zh-CN/authentication.json` 与 `en-US/authentication.json`，
   覆盖 `pageTitle`/`pageDesc`（应用语言包在核心包之后 `mergeLocaleMessage`，同名键会覆盖），
   改成 rabbithole 的定位文案；也可顺带覆盖 `loginSubtitle`。
3. 登录页插画：给 `AuthPageLayout` 传 `sloganImage`（自有图），或直接把偏好 `app.authPageLayout`
   覆盖为 `'panel-center'`（不渲染左侧插画面板，只留表单，最省事）。
4. 可选：偏好里 `logo: { enable: false }`（侧边栏只显示应用名文字）。若保留侧边栏 logo，
   务必把 `logo.source` 换成自有资源，不要再指向 unpkg 的 vben logo。
5. `preferences.app.defaultAvatar` 默认也是 unpkg 的 vben 头像 URL。`user_info` 接口的 `avatar` 是空串，
   `basic.vue` 里 `?? preferences.app.defaultAvatar` 对空串不生效（`??` 只挡 null/undefined），
   所以头像组件实际会走 reka-ui 的 fallback（显示用户名首字母）——但该 URL 仍留在产物里，
   且 `@vben/preferences` 的配置面板会展示它，建议一并换成自有资源。
6. 登录页文案补充：`layouts/auth.vue` 还传了 `authentication.pageDesc`（"工程化、高性能、跨组件库的前端模版"）
   与 `pageTitle`（"开箱即用的大型中后台管理系统"）作为左侧插画旁的宣传语——即上面第 2 步要覆盖的两个键；
   登录表单自身的副标题 `authentication.loginSubtitle`（"请输入您的账户信息以开始管理您的项目"）也可一并改。

### 1.7 运行时行为与死代码

1. `src/preferences.ts` 追加 `app: { enableCheckUpdates: false }`。
   原因：`enableCheckUpdates` 默认 `true`，`BasicLayout` 会挂 `CheckUpdates` 组件——
   每分钟对当前地址发 HEAD 请求比对 etag/last-modified，一旦变化弹「新版本可用」模态框。
   对自托管控制台这是模板行为（重新部署静态资源就会误报），建议关闭。
   （注：该组件对 `localhost`/`127.0.0.1` 会提前 return 不做检查，所以本地开发看不出来；
   一旦用域名/IP 访问就会生效——正好是真实部署场景。）
2. `copyright` 建议同时设 `settingShow: false`，避免「偏好设置」抽屉里出现让用户编辑 Vben 版权/ICP 的输入框。
3. **死代码：菜单下发链路**。`accessMode: 'frontend'` 下 `fetchMenuListAsync` 永远不会被调用
   （`generateRoutesByFrontend` 不消费它；后端 `/api/menu/all` 也只是返回 `[]` 的占位实现）。可删：
   `src/api/core/menu.ts` 整个文件、`src/api/core/index.ts` 里的 `export * from './menu'`、
   `src/router/access.ts` 里 `fetchMenuListAsync` 块（连同只为它服务的 `message.loading` 与
   `$t('common.loadingMenu')` 引用、`getAllMenusApi` import）。
   若想保留后端下发菜单的能力，则保留并在注释里写明「当前 accessMode=frontend 不走此路径」。
4. 可选死代码：`src/api/core/auth.ts` 的 `refreshTokenApi` 与 `src/api/request.ts` 的 `doRefreshToken`
   ——`enableRefreshToken` 默认 `false` 且后端没有 `/api/auth/refresh` 路由，链路永不触发。
   删除时保留 `authenticateResponseInterceptor` 的 401 分支（`doReAuthenticate` 是真实使用的）。
5. **顺带修正（设计文档已要求，与清理无关但一行）**：`src/api/core/auth.ts` 的 `logoutApi` 目前用
   `baseRequestClient`（**不注入 Authorization**），而后端 `POST /api/auth/logout` 要求 Bearer，
   无 Bearer 返回 401 → `store/auth.ts` 的 `logout()` 把异常吞掉 → **服务端审计事件 `logout: <user>` 实际不会记录**。
   `docs/design.md` §8.2 明确写了「`logoutApi` 显式附 `Authorization`」。
   修法：给该请求显式带 `Authorization: Bearer <token>`（从 `useAccessStore().accessToken` 取），
   或改用 `requestClient`。改完在浏览器验证退出后后端事件流出现 logout 审计。

### 阶段 1 门禁（必须全绿再进入阶段 2）

```bash
cd /Users/yii/Documents/mycode/rabbithole/web
pnpm run lint            # oxlint + oxfmt + eslint + stylelint，要求 exit 0
pnpm run test:unit       # vitest，要求 575 用例全通过（基线）
cd apps/web-antd && pnpm run typecheck && cd ../..
pnpm run build:antd      # 产出 apps/web-antd/dist
```

构建后自检产物已不含演示页（**chunk 名不等于文件名**，所以按内容而非文件名检查更可靠）：

```bash
cd apps/web-antd/dist
# 1) 演示页特征文案（来自被删的视图文件，构建后应全部为空）
grep -rl "组件使用演示" . ; grep -rl "总用户量" . ; grep -rl "基本设置" js/ ; grep -rl "Vben Admin Antd" . 
# 2) 品牌与统计残留（应全部为空）
grep -rl "baidu\|_hmt" . ; grep -rl "ann.vben@gmail.com" . ; grep -rl "doc.vben.pro" . ; grep -rl "unpkg.com/@vbenjs" .
# 3) 入口版权头（应不再出现 Vben Admin）
head -c 400 jse/*.js
```

---

## 阶段 2：裁剪 adapter 死注册（建议做，注意风险控制）

背景（已核实）：本项目业务页（`views/rabbithole/**`）**完全不使用** `src/adapter/component/index.ts` 注册的
antd 组件——它们直接 `import { ... } from 'ant-design-vue'`；登录页用的 `VbenInput`/`VbenInputPassword` 来自
`@vben-core/form-ui` 的**内置组件表**，与 adapter 无关。adapter 注册表目前唯一的真实消费者是
`@vben-core/popup-ui` 的 modal/drawer/alert 按钮（`globalShareState.getComponents()` 取
`DefaultButton`/`PrimaryButton`，取不到时有 `VbenButton` 兜底）。

做法（二选一，优先 A）：
- **A（保守，推荐）**：保持 `adapter/component/index.ts` 原样，只确认 `adapter/vxe-table.ts` 已删（阶段 1）。
  它不影响正确性，只是 bundle 里多些未被引用的懒加载 chunk。
- **B（激进）**：把 `adapter/component/index.ts` 裁剪为最小实现——只保留
  `DefaultButton`/`PrimaryButton`（antd Button 包装）与 `globalShareState.defineMessage({ copyPreferencesSuccess })`，
  删除 `withDefaultPlaceholder`/`Upload`/`VCropper`/`IconPicker`/`ApiComponent`/`ApiCascader`/`ApiSelect`/
  `ApiTreeSelect`/`AutoComplete`/`Cascader`/`Mentions`/`Rate`/`TimePicker` 等无消费者注册及其 `ComponentPropsMap`
  条目（约 700 行）。`adapter/form.ts` 的 `initSetupVbenForm` 保留（bootstrap 调用，为弹窗/表单提供规则与
  modelPropName 约定，删它要同步改 bootstrap，收益低）。
  裁完必须重跑阶段 1 门禁 + 浏览器冒烟：登录失效弹窗（`AuthenticationLoginExpiredModal`）与「退出登录」确认弹窗
  的按钮仍正常渲染、可点击。

**阶段 1/2 完成后的二阶死代码**（删除 `adapter/vxe-table.ts` 与 profile 页面之后才会出现的，一并清）：

- `src/adapter/form.ts` 的运行时导出 `useVbenForm`（只被已删的 `vxe-table.ts` 使用）与 `z` re-export：
  清理后全仓只剩 `initSetupVbenForm` 一个导出被 bootstrap 使用；`VbenFormSchema`/`VbenFormProps` 两个
  **类型**导出也随 profile 删除而失去消费者（`grep "from '#/adapter/form'"` 应为空）。可把该文件缩到
  `initSetupVbenForm` + 必要类型。
- `src/router/access.ts` 的 `pageMap`（`import.meta.glob('../views/**/*.vue')`）与 `layoutMap` 里的
  `IFrameView`：这两项只在 `accessMode: 'backend'` 时被消费（`generate-routes-backend.ts`），
  frontend 模式下完全不使用。若采纳 1.7-3 删除菜单下发链路，可一并删除 `pageMap`/`layoutMap`
  （`IFrameView` 在 `src/layouts/index.ts` 的导出也随之失去消费者，可删）。
  注意：删 `pageMap` 后，未被路由引用的 `.vue` 文件就不会再进构建图——这正是让阶段 1 的删除
  「真正生效于产物」的机制。
- `src/bootstrap.ts` 的 `registerLoadingDirective`（`v-loading`/`v-spinning`）：全仓（含 rabbithole 业务页）
  **没有任何地方使用**这两个指令；`registerAccessDirective` 的 `v-access` 同样零使用
  （P0 单角色 admin，无权限指令场景）。两者都是「注册了但没人用」，可删注册（连同 import）；
  若想保留以备后用，加注释说明。删了可少打包 `common-ui/es/loading` 与 `access/directive` 两块代码。
- `src/bootstrap.ts` 的 `initTippy` + `@vben/common-ui/es/tippy`：唯一 `v-tippy` 使用点是
  `packages/@core/ui-kit/shadcn-ui` 的 `collapsible-params-item.vue`（该组件自身无消费者，见阶段 3），
  其余 UI 的 tooltip 走 reka-ui 的 `VbenTooltip`。可删注册与依赖。
- **`apps/web-antd/package.json` 的 `@vben/plugins` 依赖**：阶段 1 删掉 `adapter/vxe-table.ts` 与
  `views/dashboard/analytics/*` 后，app 源码里只剩 `bootstrap.ts` 的 `import('@vben/plugins/motion')`
  一处引用（该 motion 插件本身全仓无 `v-motion` 使用）。若按上面删掉 MotionPlugin 注册，
  **这个依赖就可以从 app 的 `dependencies` 里移除**（属阶段 3 动作，需同步评估 `packages/effects/plugins` 的存留）。
- **`src/router/index.ts` 的 `resetRoutes` 导出**：`grep -rn "resetRoutes"` 除定义处外全仓零引用
  （上游为「切换角色重置路由」预留）。可删该行与 `resetStaticRoutes` import。
- **`packages/effects/access/src/access-control.vue`（`<AccessControl>`）与
  `packages/utils/src/helpers/get-popup-container.ts`**：两者全仓零使用
  （`getPopupContainer` 只有自身定义、无任何调用；`AccessControl` 只有 `src/index.ts` 的 export）。
  P0 单角色 admin 无权限组件场景。可删组件与该导出（`useAccess` 仍被 `directive.ts` 使用，保留）。

---

## 阶段 3：可选深清理（做之前先征求确认；默认不执行）

以下对象已确认**全仓无消费者**，但位于框架层（`web/packages/**`），删除收益是瘦身、风险是碰框架代码。
**如果要做，每删一项都要先 grep 验证再跑全量门禁**，并遵守两条硬规则：

1. **catalog 同步规则**：`pnpm-workspace.yaml` 的 `catalog:` 里凡是没有 package.json 消费的条目，
   会被 eslint-plugin-pnpm 的 `yaml-no-unused-catalog-item` 报错。删依赖必须同步删 catalog 条目（反之亦然）。
   **例外：`element-plus` 必须保留**——`packages/@core/ui-kit/form-ui/__tests__/form-field-node-name.test.ts`
   以深路径引用它（回归 issue #8214）。
2. 不要动 `packages/@core/**` 的核心能力（layout-ui / form-ui / popup-ui / shadcn-ui 的现役组件）。
3. **删源码必须连带处理它的测试文件**（否则 `pnpm run test:unit` 会红）。已确认存在测试、且随候选删除项一起失效的有：
   - `packages/effects/plugins/src/vxe-table/__tests__/use-vxe-grid.reactivity.test.ts`（2 例）→ 删 `vxe-table` 时一并删
   - `packages/effects/layouts/src/widgets/notification/__tests__/notification.test.ts`（1 例）→ 删 notification 时一并删
   - `packages/effects/common-ui/src/components/icon-picker/__tests__/icon-picker.test.ts`（4 例）→ 删 icon-picker 时一并删
   - `packages/effects/common-ui/src/components/api-component/__tests__/api-component.test.ts`（4 例）→ 删 api-component 时一并删
   - `packages/@core/ui-kit/shadcn-ui/src/components/collapsible/__tests__/collapsible.test.ts`（1 例）→ 删 collapsible 时一并删
   因此**阶段 3 之后的用例数会低于 575，这是预期的**——报告里要写明「删了哪些测试、为什么」，
   而不是把用例数下降当成失败。基线：78 文件 / 575 用例（已实测）。

候选清单：
- **`packages/effects/plugins/src/echarts/**`：优先级最高**——它是 674 KB 的 echarts 运行时（含 zrender），
  唯一消费者是阶段 1 已删的演示页。**注意**：光删 `views/dashboard/` 就已经让它离开产物（tree-shaking 生效），
  所以这一项**不是必须的**；但若想同时删掉 `packages/effects/plugins` 的 `echarts` 依赖与 catalog 条目，
  可一并处理。别为了删它而破坏阶段 1 已得的收益判断——先在阶段 1 后跑一次构建，确认 `use-echarts-*.js` 已消失。
- `packages/effects/plugins`：`src/tiptap/**`（全仓零引用）、
  `src/vxe-table/**`（仅已删的 `adapter/vxe-table.ts` 使用）、`src/motion/**`（`bootstrap.ts` 注册了
  `MotionPlugin` 但全仓无 `v-motion` 使用）。若全部删除：同步改 `packages/effects/plugins/package.json`
  （exports/deps）、`apps/web-antd/package.json`（移除 `@vben/plugins`）、`bootstrap.ts`（移除 MotionPlugin 注册）、
  `internal/vite-config`（`vxeTableLazyImport: true` 与 `vite-plugin-lazy-import` devDependency）、
  catalog（`@tiptap/*`、`echarts`、`vxe-pc-ui`、`vxe-table`、`@vueuse/motion`、`vite-plugin-lazy-import`）。
- `packages/effects/common-ui/src/components/`：`captcha/**`（3 个验证码组件，零引用）、`cropper/**`（仅被将删的
  adapter 使用）、`json-viewer/**`、`tree/**`、`count-to/**`、`ellipsis-text/**`、`col-page/**`、`resize/**`
  （仅框架自导出，无消费者）——需同步改 `components/index.ts` 与 `package.json`（`vue-json-pretty`、`json-bigint`、
  `@vueuse/integrations` 等依赖）。
- `packages/effects/common-ui/src/ui/`：`about/`、`profile/`、`dashboard/`（analysis+workbench）——阶段 1 后无消费者；
  同步改 `ui/index.ts`。
- `packages/effects/common-ui/src/ui/authentication/` 的演示子页：`code-login.vue`、`qrcode-login.vue`、
  `forget-password.vue`、`register.vue`、`third-party-login.vue`、`dingding-login.vue`——阶段 1 删掉应用侧页面、
  且 `login.vue` 传 `show-*-login=false` 后全部无消费者（注意 `login.vue` 对 `ThirdPartyLogin` 是**静态 import**，
  要一并删掉那行 import 才能让它离开产物）。同步改该目录 `index.ts`。
  连带可删：`packages/effects/hooks/src/use-app-config.ts` 里的 `auth.dingding` 默认配置、
  `packages/icons` 的 `SvgGoogleIcon`/`SvgQQChatIcon`/`SvgWeChatIcon`/`SvgDingDingIcon`、
  `packages/effects/common-ui/package.json` 的 `@vueuse/integrations`、`qrcode`、`@types/qrcode`。
- **「关掉偏好 ≠ 移出产物」**：`packages/effects/layouts/src/basic/header/header.vue` 与
  `basic/layout.vue` 对 `GlobalSearch`、`Notification`、`TimezoneButton`、`CheckUpdates`、`LockScreen` 都是
  **静态 import**，用偏好关掉只是不渲染，代码仍在 bundle 里。若目标是真瘦身，需要在 stage 3 里改这两个文件
  删掉对应 import/模板（并把 `widgets/global-search`、`widgets/notification`、`widgets/timezone`、
  `widgets/check-updates`、`widgets/lock-screen` 目录一并评估）；只做阶段 1 的话它们仍在产物中（可接受）。
- `packages/effects/layouts/src/authentication/icons/slogan.vue`：vben 的 4568 行登录插画，
  即使偏好切到 `panel-center` 不渲染，它仍被 `authentication.vue` 静态 import。真删需改该文件。
- `packages/@core/ui-kit/shadcn-ui`：`VbenTableAction`、`VbenDescriptions`、`VbenCollapsibleParams`、
  `VbenCountToAnimator` 无消费者（`components/index.ts` 里注释「给文档用」的那批 export）。
- `packages/effects/layouts/src/basic/copyright/copyright.vue` 的默认值 `companyName: 'Vben Admin'`
  （阶段 1 关掉 `copyright.enable` 后不会渲染；如需彻底，可改成空默认值）。
- `packages/icons/src/svg/`：`SvgAvatar1-4`、`SvgAntdvLogoIcon`、`SvgAntdvNextLogoIcon`、`SvgTDesignIcon`、
  `SvgBellIcon`、`SvgCakeIcon`、`SvgCardIcon`、`SvgDownloadIcon`、`SvgGoogleIcon`、`SvgQQChatIcon`、
  `SvgWeChatIcon`、`SvgDingDingIcon`（后 4 个仅被登录页第三方登录组件使用）及对应 `svg/icons/*.svg`。
- `packages/locales/src/langs/zh-TW/`：整目录（5 个 json）。`SupportedLanguages` 只声明 en-US/zh-CN，
  且全仓没有 module augmentation 注册 zh-TW，运行时永远不会加载它。**已实测确认它真的进了产物**
  （`dist/js/{common,authentication,preferences,ui}-*.js` 4 个 chunk 约 24 KB，含 `歡迎回來`/`登入成功` 等繁体文案），
  删除后这些 chunk 消失。同步可清理 `packages/@core/base/typings/src/languages.ts` 里那段
  「如何追加 zh-TW」的文档注释示例。
- `packages/styles/src/{ele,naive,antdv-next}/index.css`：这三个子路径导出分别服务于已删除的
  `apps/web-ele`、`apps/web-naive`、`apps/web-antdv-next`（本仓只剩 `web-antd`）。
  `grep "@vben/styles/"` 全仓只有 `@vben/styles/antd` 与 `@vben/styles/global` 被引用。
  可删这三个目录与 `packages/styles/package.json` 的对应 `exports` 条目。
- `packages/@core/base/shared/src/constants/vben.ts`：整个文件是 vben 品牌 URL 常量。
  阶段 1 删掉 `basic.vue` 的品牌菜单 + 阶段 3 删掉 `about.vue` 之后，**9 个常量全部零引用**
  （已核实：`VBEN_LOGO_URL`/`VBEN_ANTDV_NEXT_PREVIEW_URL`/`VBEN_ELE_PREVIEW_URL`/`VBEN_NAIVE_PREVIEW_URL`/
  `VBEN_ANT_PREVIEW_URL`/`VBEN_TD_PREVIEW_URL` 当前就已经零引用；`VBEN_GITHUB_URL`/`VBEN_DOC_URL`/
  `VBEN_PREVIEW_URL` 仅剩 `basic.vue` 与 `about.vue` 在用）。可删该文件 + `constants/index.ts` 里的
  `export * from './vben'`。
  注意**不要**连带删 `globals.ts` 里的 `DEFAULT_NAMESPACE`/CSS 变量常量（`--vben-*` 变量名与
  `__vben_main_content` 等 DOM id 是框架运行时契约，见上文 BEM 说明）。
- `packages/preferences/src/index.ts` 的 `appCopyrightPreferences` 常量：阶段 1 把 app 的
  `copyright` 改成 `{ enable: false }` 后，全仓只剩它的定义与 export（app 不再 import），
  可连同 `definePreferencesExtension`（阶段 1 删掉 `preferencesExtension` 后同样只剩定义/export）
  一并评估删除；注意保留 `defineOverridesPreferences`（`src/preferences.ts` 仍在用）。
- `packages/effects/hooks/src/use-design-tokens.ts`：421 行里只有 `useAntdDesignTokens`（第 10–76 行）被
  `apps/web-antd/src/app.vue` 使用；`useNaiveDesignTokens`（77–162）、`useElementPlusDesignTokens`（163–322）、
  `useTDesignDesignTokens`（323–421）分别服务于已删除的 `web-naive`/`web-ele`/`web-tdesign` 应用，全仓零引用。
  可删后三个函数（约 340 行）。
- `packages/@core/base/design` 的 `@iconify/json` 依赖：全仓没有任何代码消费它（`@iconify/tailwind4` 用的是
  在线 API，`@vben-core/icons` 用的是 `@iconify/vue`），只在 `scripts/vsh/src/check-dep/index.ts` 的
  `ignoreDependencies` 白名单里出现过——那是为了让 knip 别报它，说明上游自己也知道它没被直接 import。
  可删依赖 + catalog 条目 + 那条 ignore。
- **运行时 CDN 依赖（不是删除项，但要知情）**：路由菜单图标（`lucide:rabbit`/`lucide:gauge`/`carbon:workspace` 等）
  与 `MdiKeyboardEsc` 走 `@iconify/vue` 的**在线 API**（`packages/@core/base/icons/src/create-icon.ts` →
  `createIconifyIcon`）。产物里含 `api.iconify.design/collection?prefix=...`（已确认在
  `dist/js/bootstrap-*.js` 与 `dist/js/src-*.js`），即**离线/内网环境下这些图标会加载失败**。
  本次清理不要动它（改造成离线图标是另一个议题），但值得在报告里记一笔。
- `packages/locales/src/langs/*/profile.json`（`{updatePassword, updateBasicProfile}`）：只被
  `packages/effects/common-ui/src/ui/profile/*.vue` 读取，随 profile 删除一起失效（阶段 3）。
- `packages/locales/src/langs/*/{ui.json,preferences.json}` 中确认无消费者的键（如 `captcha`、`tiptap`、
  `preferences.antd.*`、`iconPicker`、`jsonViewer`、`crop`、`ui.widgets.checkUpdates*`/`lockScreen.*`/
  `timezone.*`/`search.*`（随对应挂件删除）、`ui.fallback.comingSoon`/`offline*`（随演示兜底页删除）等）
  ——可选，收益小，做的话逐键 grep 验证。
- 工具链残留（根级，可选）：`web/package.json` 的 `changeset`/`commit`(czg)/`check:cspell`/`test:benchmark`/
  `test:e2e` 脚本与其 devDependencies（cspell、@changesets/*、@vben/commitlint-config、lefthook、playwright、knip），
  这些工具在本仓没有配置（无 `.changeset/`、无 `cspell.json`）。删了要同步改 catalog。
  `scripts/deploy/`（vben 的 Dockerfile/nginx，构建目标是**已删除的 `playground`**，属坏引用）与
  `package.json` 的 `build:docker` 脚本、`turbo.json` 里 `@vben/backend-mock#build` 任务定义（已删应用）同理。
  ⚠️ knip 输出里有两类**误报，不要动**：根 devDependencies 的 `eslint`（`vsh lint` 以 CLI 方式调用它，
   靠 `publicHoistPattern` 提升，knip 看不到二进制调用）与 `lefthook`（按设计保持禁用，见 `FRONTEND-NOTES.md` 第 2 节）。
  真正可删的是 `is-ci`（全仓无引用）、`@changesets/changelog-github`、根 `dependencies.sortablejs`
  （`packages/@core/composables` 与 `packages/effects/layouts` 各自声明了它，根上这份是多余的）。
- **`internal/lint-configs/commitlint-config` 整个包**：它只被根 `package.json` 的
  `devDependencies["@vben/commitlint-config"]` 引用，而它的实际用途是给上游 `lefthook.yml` 的
  commit-msg 钩子提供配置——**本仓已删除 `lefthook.yml` 且禁用了 lefthook**（见 `FRONTEND-NOTES.md` 第 2 节），
  全仓也没有 `.commitlintrc`/`cz.config` 之类的配置指向它。即"声明了但没有任何触发路径"。
  删它需要同时删：根 `devDependencies` 的 `@vben/commitlint-config`、`package.json` 的 `commit`(czg) 脚本、
  该包目录、以及 catalog 里的 `@commitlint/cli`、`@commitlint/config-conventional`、
  `commitlint-plugin-function-rules`、`cz-git`、`czg`、`@commitlint/*` 与 `publicHoistPattern` 里的
  `@commitlint/*`、`czg` 两行。**注意**：`pnpm-lock.yaml` 与 `node_modules/.bin` 里仍有 `commitlint`/`czg`
  可执行文件（来自该包），删包后要重跑 `pnpm install` 让 lockfile 收敛。

---

## 硬性约束（违反即失败）

1. **不要动 Rust 后端**（`crates/`、`migrations/`、`tests/`、`deploy/*.toml`）。
2. **不要改 API 契约**：`api/request.ts`（envelope `code===0`、`data` 解包、401 处理）、`api/core/auth.ts`
   （`/auth/login`、`/auth/logout`、`/auth/codes`）、`api/core/user.ts`（`/user/info`）、`api/rabbithole.ts`
   （clients/services/visitors/stats/events/changePassword）、`store/auth.ts` 的登录/登出流程。
3. **不要改 `apps/web-antd/vite.config.ts` 的 `resolve.dedupe: ['vue','vue-router','pinia','@vue/devtools-api']`**
   （它修复过「登录后主界面空白」，删了会回归）与 dev proxy（`/api → http://127.0.0.1:8080`，无 rewrite）。
4. **不要改 `.env.development` / `.env.production` 的** `VITE_GLOB_API_URL=/api`、`VITE_NITRO_MOCK=false`、
   `VITE_ROUTER_HISTORY=hash`；产物目录必须仍是 `web/apps/web-antd/dist`（rh-server 的 `static_dir` 指向它）。
5. 保留 `router/guard.ts`、`router/access.ts` 框架流程；保留 `views/_core/fallback/not-found.vue` 与
   `forbidden.vue`；保留 `preferences.app.defaultHomePath: '/rabbithole/overview'`。
6. `packages/**`、`internal/**` 是框架基座，**阶段 1/2 不改**（阶段 3 例外，且须逐项验证）。
7. 每阶段结束后，工作区必须能通过全部门禁；**不要用 `--no-verify`、不要跳过测试、不要屏蔽规则**。
8. 若某个"删除项"经 grep 发现实际有引用（清单与代码不一致），**停下来在报告里说明，不要硬删**。

## 门禁命令（每个阶段结束都跑，全部 exit 0）

```bash
cd /Users/yii/Documents/mycode/rabbithole/web
pnpm run lint                                   # vsh lint：oxlint + oxfmt + eslint + stylelint
pnpm run test:unit                              # vitest：基线 78 文件 / 575 用例
pnpm run check:type                             # turbo run typecheck
pnpm run build:antd                             # 应用构建（产出 dist + dist.zip）
```

说明：仓库要求 pnpm 11.16.0（`packageManager` 字段会自动切换）；`test:unit` 曾有上游 flake
（`form-integration.test.ts` 偶发 1~2 例失败，与本次改动无关），若失败先单文件复跑确认。

### ⚠️ 改了依赖 / catalog 就必须同步 lockfile（否则 CI 必红）

`.github/workflows/ci.yml` 的 frontend job 跑的是 `pnpm install --frozen-lockfile`
（工作目录 `web/`），lockfile 是 `web/pnpm-lock.yaml`（`lockfileVersion: '9.0'`，已纳入版本控制）。
因此**只要动了任何 `package.json` 的依赖、或 `pnpm-workspace.yaml` 的 `catalog:`/`allowBuilds`**：

```bash
cd /Users/yii/Documents/mycode/rabbithole/web
pnpm install                     # 不带 --frozen-lockfile，让 lockfile 跟随更新
pnpm install --frozen-lockfile   # 复现 CI 的校验方式，必须 exit 0
```

`pnpm install` 会自动清理 lockfile 里失效的 importers/依赖项（上游裁剪时已发生过一次），
所以不要手改 `pnpm-lock.yaml`。若只改源码/删文件、没动依赖，则不需要重跑 install。

### ⚠️⚠️ 既有缺陷：`pnpm install --frozen-lockfile` 当前就 exit 1（必须先修，否则整条门禁不可信）

**实测证据**（2026-10-09，pnpm 11.16.0）：在 `web/` 下执行 `pnpm install --frozen-lockfile`，
安装本身成功、`Done in 5.3s using pnpm v11.16.0`，但进程 **exit 1**，末尾输出：

```
[ERR_PNPM_IGNORED_BUILDS] Ignored build scripts: lefthook@2.1.14
Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.
```

**根因**：`pnpm-workspace.yaml` 的 `allowBuilds` 段里，`lefthook` 的值是字符串占位符
`"set this to true or false"`（上游模板原样，本仓裁剪时照抄）。pnpm 11 把「非 `true` 的值」
一律当作「不允许构建」并计入 ignored-builds，而 `allowBuilds` 里出现被忽略项会让 install 以非零码退出。

**修法（二选一，推荐 A）**：
- **A（推荐）**：把该行改成显式 `lefthook: false`。语义与现状完全一致（本仓按 `FRONTEND-NOTES.md`
  第 2 节的裁剪决策**本来就不允许** lefthook 运行 postinstall——它会往父仓库 `.git` 写 hooks），
  且 `--frozen-lockfile` 立即变 exit 0。**已实测验证**：改成 `false` 后 `pnpm install --frozen-lockfile`
  → `EXIT=0`，`Done in 5.3s`；改回占位符字符串 → 立刻回到 `EXIT=1`。
  改完记得把 `pnpm-workspace.yaml` 的这条写进 `FRONTEND-NOTES.md`（那里记的是"剔除 lefthook"，
  但没记 pnpm 11 会因为占位符值而失败）。
- **B**：从 `allowBuilds` 整段删掉 `lefthook` 条目，并从 `catalog`/根 `devDependencies` 移除
  `lefthook`（`catalog: lefthook: ^2.1.14` + 根 `devDependencies.lefthook`）。更彻底，
  但要同步处理 catalog 与 lockfile（见上一节），且 `publicHoistPattern` 里也有一行 `- lefthook` 要删。

**为什么这条必须放在最前面做**：CI 的 frontend job 第一步就是 `pnpm install --frozen-lockfile`，
它现在会直接失败——也就是说**在修掉它之前，任何"CI 全绿"的说法都不成立**，本任务的
"门禁全绿"验收也失去意义。它同时解释了 `check:dep`(knip) 为何把 `lefthook` 报成 unused devDependency
（见阶段 3 的说明：那一条**不要**按普通未使用依赖处理，它是被有意禁用的）。

## 浏览器冒烟（阶段 1 完成后必做，阶段 2 做 B 方案时加做）

1. `cd web/apps/web-antd && pnpm dev`（端口 5666，`/api` 代理到本机 rh-server 8080；需后端在跑）。
2. 未登录访问深层路由 → 被守卫重定向到登录页；登录页**只有** 账号/密码/记住账号/登录按钮，
   无忘记密码、无注册、无扫码/手机号登录、无第三方图标、无页脚 Vben 版权与 ICP。
3. 登录后侧边栏只有「rabbithole」一组：总览 / 客户端 / 转发规则 / 访问器 / 设置，五个页面都能打开，
   数据来自 `/api/*`（可在 Network 面板确认）。
4. 右上角无通知红点、无 Vben 文档/GitHub/问答入口、用户下拉无「个人中心」与 ann.vben@gmail.com/Pro；
   **退出登录仍可用**（默认在顶栏右侧的退出图标按钮，点它 → 确认弹窗 → 回登录页）。
5. 访问不存在路径 → 404 兜底页；退出登录 → 回登录页（URL 不会层层叠加 redirect）。

## 交付要求

- 按阶段提交清晰的改动（或至少给出分阶段的文件变更清单：新增/删除/修改 各列路径）。
- 更新 `web/FRONTEND-NOTES.md`：追加一节「模板残留清理」，写明删了哪些页面/路由/依赖、为什么、
  以及门禁结果；如删除了框架包内容，同步更新其「裁剪清单」表。
  注意该文件 §5「web-antd 目录结构要点」里写 `modules/` 为 `dashboard.ts/demos.ts/vben.ts`，
  与实际（本次清理前只有 `dashboard.ts` + `rabbithole.ts`）不符，一并更正。
- 同步更新仓库根 `docs/acceptance.md`（前端质量门那一节，§2 与 §12/§13）：
  它记录了 `78 文件 / 575 用例` 与「16 警告均为上游 vben 框架测试文件」等基线数字；
  本次清理后若用例数/警告数变化（见阶段 3 的测试连带删除规则），要更新这些数字，
  否则文档会与实际不符。**这是交付物的一部分，不要漏。**
- 顺带修正文档漂移（可选）：
  - `docs/design.md` §8.3 的页面表还写着 `/dashboard`、`/clients`、`/services`、`/events`，
    与实际路由（`/rabbithole/overview|clients|services|visitors|settings`）不一致；
  - `docs/design.md` §7.1 的 `/api/user/info` 示例写 `homePath:"/dashboard"`，而后端实际返回
    `"/rabbithole/overview"`（`crates/rh-server/src/api.rs`）。两处都改成实际值。
- 可选：`apps/web-antd/src/layouts/auth.vue` 给 `AuthPageLayout` 传 `:toolbar="false"` 可去掉登录页右上角的
  「布局切换/配色/语言/主题」工具条（模板演示功能，与登录无关）；保留也无害。
- 可选：仓库根 `.gitignore` 第 11 行 `apps/web-antd/dist` 是相对**根目录**的路径（即
  `rabbithole/apps/web-antd/dist`，不存在），属死规则；产物实际由 `web/.gitignore` 的 `dist` 覆盖。
  删掉该行（属仓库根文件，改动前确认无其他用途）。
- **产物体积对比（建议做，很有说服力）**：改造前先记录基线
  （当前：`dist` 总计 4.3 MB，`js/` 4.1 MB / 217 个 chunk；其中 `use-echarts-*.js` 674 KB），
  改造后重新构建再记录一次，在报告里给出前后对比。
- 最终报告：每个阶段的门禁命令输出结论（exit code + 用例数），以及浏览器冒烟的逐条结果；
  任何未能完成的项要明确说明原因，不要含糊带过。
