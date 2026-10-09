# rabbithole 前端基座笔记（vue-vben-admin）

本目录是 vue-vben-admin 的裁剪版 monorepo，仅保留 `apps/web-antd` 一个应用，作为 rabbithole（Rust 后端 + 本前端）的管理端基座。本文件记录来源、裁剪依据、环境偏离与构建验证结果。

## 1. 来源与版本

- 上游仓库：<https://github.com/vbenjs/vue-vben-admin>
- 基线 commit：`d27a5b8a0c2972a10d583a7050fcb889766ff83f`（main，2026-10-07，commit message: `fix(layout): 取消固定后二级菜单消失问题 (#8429)`）
- 复制方式：完整 clone 后 checkout 该 commit，`git rev-parse HEAD` 校验一致，删除 `.git` 后复制裁剪到本目录（`/tmp/vben-clone` 为临时工作副本）。

## 2. 实际使用的工具链版本与偏离

| 项 | 上游要求（根 package.json） | 本机实际 | 说明 |
| --- | --- | --- | --- |
| Node | `^22.18.0 \|\| ^24.12.0`（engines），`.node-version` 为 24.16.0 | v26.10.0 | **偏离**。`.npmrc` 未开启 `engine-strict`，pnpm 仅打印 `[WARN] Unsupported engine`，不阻断 install/build，实际构建成功。未降低 Node 版本。 |
| pnpm | engines `>=11.0.0`；`packageManager: "pnpm@11.16.0"` | 全局 pnpm 11.9.0，**install 实际以 pnpm v11.16.0 执行** | 无实质偏离。Node 26 已不捆绑 corepack（`corepack not found`），但 pnpm ≥10 的 manage-package-manager-versions 机制会读取 `packageManager` 字段并自动切换到 11.16.0（install 日志末尾 `Done in 25.3s using pnpm v11.16.0`）。 |
| 包源 | — | `registry=https://registry.npmmirror.com`（沿用上游 `.npmrc`） | 网络可用，未调整。 |

其他兼容性记录：

- 首次 `pnpm install` 曾失败（exit 1），原因是复制 `scripts/` 时目录嵌套（`scripts/scripts/`），导致 workspace 里找不到 `@vben/turbo-run` / `@vben/vsh`；修正目录结构后重装成功。
- **lefthook 事件（重要偏离）**：`pnpm-workspace.yaml` 上游的 `allowBuilds` 允许 lefthook 运行构建脚本，其 postinstall 会在 install 时自动执行 `lefthook install`，向**父仓库**（rabbithole 的 `.git`）写入 `.git/hooks/prepare-commit-msg`、`.git/info/lefthook.checksum` 并在仓库根生成 `lefthook.yml` 模板。已做两件事：
  1. 清理上述三处文件，`rabbithole/.git` 已恢复原状（hooks 仅剩 `*.sample`，info 仅剩 `exclude`）；
  2. 删除根 `package.json` 的 `prepare` 脚本，并从 `pnpm-workspace.yaml` 的 `allowBuilds` 中移除 `lefthook`，未来重新 `pnpm install` 不会再污染父仓库。

## 3. 裁剪清单与依据

上游该 commit 的 `apps/` 实际包含 6 个应用：`backend-mock`、`web-antd`、 `web-antdv-next`、`web-ele`、`web-naive`、`web-tdesign`（比早期版本多出 `web-antdv-next` 与 `web-tdesign`）。

### 保留

| 路径 | 依据 |
| --- | --- |
| `package.json`（根） | workspace 根，`devDependencies` 引用 `@vben/turbo-run`、`@vben/vsh`、`@vben/vite-config` 等 workspace 包 |
| `pnpm-workspace.yaml` | workspace 定义 + catalog 版本表（全部依赖版本走 catalog），已编辑（见下） |
| `pnpm-lock.yaml` | 锁定依赖；install 时 pnpm 自动移除了被删应用的 importers（lockfile 已随之更新） |
| `turbo.json` | `pnpm build` = `cross-env NODE_OPTIONS=--max-old-space-size=8192 turbo build`；turbo `^2.10.13`（catalog）；其中 `@vben/backend-mock#build` 任务定义保留（包已删，turbo 忽略） |
| `packages/**` 全部 | `@core/*`、`effects/*`（access/common-ui/hooks/layouts/plugins/request）、`constants`、`icons`、`locales`、`preferences`、`stores`、`styles`、`types`、`utils`，均为 web-antd 的 `workspace:*` 运行时依赖 |
| `internal/**` 全部 | `vite-config`、`node-utils`、`tailwind-config`、`tsconfig`、`lint-configs/*`；前三者参与构建，lint-configs 被根 devDependencies 引用且带 `stub`（postinstall 构建） |
| `scripts/turbo-run/**` | 根 `dev`/`preview` 脚本使用（`@vben/turbo-run`） |
| `scripts/vsh/**` | 根 `lint`/`format`/`check:*`/`publint` 脚本使用（`@vben/vsh`） |
| `scripts/clean.mjs`、`scripts/deploy/` | 根 `clean`、`build:docker` 脚本引用 |
| `apps/web-antd/**` | 目标应用 |
| `.npmrc`、`.editorconfig`、`.gitignore`、`.node-version`、`.browserslistrc`、`vitest.config.ts` | 根级必要/辅助文件；`.gitignore` 全为相对模式，作为 web 子目录 ignore 直接适用，不含上级规则 |

### 删除（未复制）

| 路径 | 说明 |
| --- | --- |
| `apps/web-ele`、`apps/web-naive`、`apps/web-antdv-next`、`apps/web-tdesign` | 其他 UI 框架演示应用，非目标 |
| `apps/backend-mock` | 内置 nitro mock 服务；rabbithole 后端为真实 Rust 服务。web-antd 的 package.json 不依赖它，仅 dev 代理指向其端口，已一并改造（见第 5 节）。即使误开 mock 插件，插件找不到 `@vben/backend-mock` 包也会跳过，不会崩溃 |
| `playground`、`docs` | 演示与文档站，已从 `pnpm-workspace.yaml` 的 packages 中移除对应两行 |
| `.github`、`.changeset`、`.vscode`、`lefthook.yml`、`cspell.json`、lint 配置（`eslint.config.mjs`/`oxlint.config.ts`/`oxfmt.config.ts`/`stylelint.config.mjs`）、README*、code-workspace、tea/gitpod 等 | CI/发布/编辑器/文档类文件，与安装构建无关 |

### 对上游文件的具体修改

1. `pnpm-workspace.yaml`：packages 移除 `- docs`、`- playground` 两行； `allowBuilds` 移除 `lefthook: true`（见第 2 节）。
2. 根 `package.json`：删除 `prepare` 脚本（lefthook install，防止写父仓库）；删除指向已删应用的脚本 `build:docs/ele/naive/tdesign/play`、 `dev:docs/ele/naive/tdesign/play/antdv-next`。其余（含 `postinstall: pnpm -r run --if-present stub`、`preinstall: npx only-allow pnpm`）原样保留。
3. `apps/web-antd` 配置见下节。

## 4. apps/web-antd 配置调整

- `.env.development`：`VITE_NITRO_MOCK=true` → `false`（关闭内置 Nitro mock）； `VITE_GLOB_API_URL=/api` 保持不变；端口 5666。
- `.env.production`：`VITE_GLOB_API_URL` 由 `https://mock-napi.vben.pro/api` 改为 `/api`（生产由反向代理转发到 Rust 后端）。`VITE_ARCHIVER=true` 使 build 额外产出 `dist.zip`。
- `vite.config.ts`：dev 代理由 `http://localhost:5320/api`（backend-mock，带 rewrite 去掉 `/api` 前缀）改为 `target: 'http://127.0.0.1:8080'` 并移除 rewrite —— 前端 `/api/xxx` 原样转发到后端 `/api/xxx`，与 `VITE_GLOB_API_URL=/api` 一致。**若后端路由不带 `/api` 前缀，需恢复 `rewrite: (path) => path.replace(/^\/api/, '')` 或调整 target。**
- mock 开关机制：`internal/vite-config/src/utils/env.ts` 读取 `VITE_NITRO_MOCK`（`getBoolean` 仅认 `'true'`），经 `loadApplicationPlugins` 的 `nitroMock` 选项启用 `viteNitroMockPlugin`（仅 dev 生效，build 不受影响）。

## 5. web-antd 目录结构要点

```
apps/web-antd/src/
├── api/
│   ├── request.ts        # requestClient / baseRequestClient（见下）
│   └── core/             # auth.ts / user.ts / menu.ts 接口定义
├── adapter/              # antd 组件适配（component/、form.ts、vxe-table.ts）
├── router/
│   ├── access.ts         # 动态路由/菜单生成入口
│   ├── guard.ts          # 路由守卫（登录态、权限）
│   └── routes/
│       ├── core.ts       # 登录等核心路由（免权限）
│       ├── index.ts      # 汇总：coreRoutes + modules glob + 404
│       └── modules/      # 业务路由模块（dashboard.ts/demos.ts/vben.ts）
├── store/auth.ts         # 登录/登出流程（authLogin/logout/fetchUserInfo）
├── views/
│   ├── _core/authentication/  # login.vue 等登录页面
│   └── dashboard/        # 业务页面示例
├── bootstrap.ts / main.ts / preferences.ts
```

- **requestClient**：`src/api/request.ts`。基于 `@vben/request` 的 `RequestClient`（axios 封装），baseURL 取 `VITE_GLOB_API_URL` （`useAppConfig`）；请求拦截器注入 `Authorization: Bearer <token>` 与 `Accept-Language`；响应拦截约定 `{ code, data }`、`successCode: 0`；401 时按 `preferences.app.enableRefreshToken` 走 `/auth/refresh`（withCredentials）刷新或重新登录。
- **登录流程**：`views/_core/authentication/login.vue` → `store/auth.ts#authLogin` → `api/core/auth.ts#loginApi` （`POST /api/auth/login`，返回 `{ accessToken }`）→ 并行拉取 `getUserInfoApi` / `getAccessCodesApi` → 跳转 `homePath`。
- **菜单/动态路由**：`router/access.ts#generateAccess` 调用 `@vben/access` 的 `generateAccessible(preferences.app.accessMode, …)`。默认 `accessMode: 'frontend'`（`packages/@core/preferences/src/config.ts`），菜单由本地 `router/routes/modules/*.ts` 生成；若要改后端下发菜单，将 `src/preferences.ts` 的 `app.accessMode` 覆盖为 `'backend'`， `getAllMenusApi`（`api/core/menu.ts`）需返回后端菜单树。

## 6. 安装与构建记录

| 命令（在 `web/` 下执行） | 退出码 | 结果 |
| --- | --- | --- |
| `pnpm install` | 1（首次） | 失败：scripts 目录嵌套导致 `ERR_PNPM_WORKSPACE_PKG_NOT_FOUND`，已修复 |
| `pnpm install` | **0** | 成功，`Done in 25.3s using pnpm v11.16.0`；postinstall stub 构建了 turbo-run/vsh/eslint-config/vite-config 等；有一条 peer dependencies 警告（不阻断） |
| `pnpm build` | **0** | 成功，turbo `11 successful, 11 total`，耗时 11.9s |

`pnpm build` = `cross-env NODE_OPTIONS=--max-old-space-size=8192 turbo build`，构建全部带 `build` 脚本的 workspace 包（packages/* 库 + web-antd 应用）。

## 7. 构建产物

- 目录：`web/apps/web-antd/dist/`（`index.html`、`jse/` 入口、`js/` 分块、 `css/`、`_app-config-*.js`），约 4.1 MB
- 归档：`web/apps/web-antd/dist.zip`（约 1.2 MB，`VITE_ARCHIVER=true` 所致）

## 8. 常用命令（在 `web/` 下）

```bash
pnpm install        # 安装（pnpm 会自动切换到 packageManager 指定的 11.16.0）
pnpm dev:antd       # 开发（端口 5666，/api 代理到 127.0.0.1:8080）
pnpm build          # 构建（含全部依赖包）
pnpm build:antd     # 仅构建 web-antd（--filter）
pnpm check:type     # 类型检查（turbo run typecheck）
```

## 8. lint / vitest 补全(2026-10-09)

### 根因:裁剪时丢失上游根级 lint 配置

`pnpm run lint`(vsh lint = oxlint --type-aware + oxfmt --check + eslint + stylelint) 此前无法运行的真实根因是裁剪复制时丢失了上游五个根级配置文件。本轮从基线clone(/tmp/vben-clone,d27a5b8)原样恢复:

| 文件 | 作用 |
| --- | --- |
| `eslint.config.mjs` | `defineConfig()`(flat config 入口) |
| `stylelint.config.mjs` | `extends: ['@vben/stylelint-config']`(extends 在包内解析,无需全局 hoist) |
| `oxfmt.config.ts` | `@vben/oxfmt-config`(printWidth 80 等;**sortPackageJson:false** —— package.json 键序归 eslint 管,oxfmt 不排序) |
| `oxlint.config.ts` | `@vben/oxlint-config`(type-aware 355 规则) |
| `.stylelintignore` | dist/public/**tests** 等忽略 |

教训:oxfmt format-mode 在无配置时会按默认(且对 package.json 排序),与eslint jsonc/sort-keys 互相改写形成"格式打架";恢复上游配置(含sortPackageJson:false)后以 `eslint --fix` 定稿即收敛。

### pnpm 11 设置位置修正

- pnpm 11 不再读 package.json `pnpm` 字段(仅警告);`neverBuiltDependencies` 等设置移至 `pnpm-workspace.yaml`。
- `allowBuilds` 按上游恢复,但**剔除 lefthook**(裁剪决策不变:其 postinstall 会向父仓库 .git 写 hooks);代价是 install 末尾出现一条 ignored-builds 提示。
- 清理 16 项裁剪后无任何 package.json 消费的 catalog 条目(element-plus 除外, 见下),消除 eslint-plugin-pnpm `yaml-no-unused-catalog-item` 16 错误。

### element-plus 回归(精确 2.14.5)

上游 `form-field-node-name.test.ts` 经 `.pnpm/element-plus@2.14.5_vue@…` 深路径 import(上游全仓时该包由 docs/playground 消费)。本仓以精确版本 `element-plus: 2.14.5` 回归 catalog,并作为 `form-ui` devDependency (`catalog:`),深路径与 eslint pnpm 规则同时满足。

### 最终基线

- `pnpm run lint`:**exit 0**(oxlint 0/0;oxfmt、stylelint 全绿;eslint 0 错误, 16 警告均为上游框架测试文件 vue/one-component-per-file)。
- `pnpm run test:unit`:**78 文件 / 575 用例全通过**(1 例曾并行调度偶发失败, 单文件与整体复跑均过,未屏蔽)。
- `pnpm run check:type` 0 错误;`pnpm run build:antd` 通过;dist 经 rh-server static_dir 冒烟(index/深路由 200、/api 未授权 401)。
- 本轮 eslint/oxfmt --fix 的自动修复覆盖全仓(导入排序、package.json 键序、80 列格式等);业务代码另有 2 处手改(toggle if/else 改三元,oxlint 规则)。

### 2026-10-09(R13:设置页改密)

- 新增 `views/rabbithole/settings`(侧边栏「设置」):当前/新/确认三段 InputPassword,
  前端校验(≥8、两次一致),成功后 `authStore.logout()` 强制重登(后端已轮换会话密钥)。
- 修复:初版 FormItem `help` 恒渲染导致**空表单误显**灰色"两次输入的新密码不一致"
  (antd `help` 无条件展示、`validate-status` 只控色);改 `:help`/`:validate-status`
  仅 mismatch 时给值。浏览器实测:空=无提示、不一致=红字(h=22,color=error)+红框、
  一致=消失。
- 门禁复跑(设置页改动后):typecheck 0 错、lint exit 0、build 通过(dist 已更新)。
- vitest 上游 flake 证据补强:form-integration.test.ts 单文件连跑 5 次,3 次出现
  1~2 例**不同**用例失败(与上游 d27a5b8 字节级一致;本轮仅改 apps/web-antd 业务
  视图,不涉该包);全量 1/1/0 例失败,第三次 575/575 全绿。未屏蔽未跳过。
