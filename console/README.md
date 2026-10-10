# Ant Design Pro V6 Template

[English](#english) | [中文](#中文)

---

<a name="english"></a>

## English

An enhanced admin template based on [Ant Design Pro](https://github.com/ant-design/ant-design-pro), featuring multi-tab navigation, optimized layout, and rich utility functions.

### Features

- **Multi-Tab Navigation** - Browser-like tab management with right-click context menu (reload, close others, close left/right)
- **Custom Top Header** - Optimized header layout with collapsible sidebar button, language switcher, and user avatar
- **Latest Tech Stack** - React 19 + Ant Design 6 + Ant Design Pro Components 3 + TypeScript 5
- **Multiple Layouts** - Side menu, top navigation, and mixed layout modes
- **CSS-in-JS** - Modern styling with `antd-style` (createStyles)
- **Rich Utilities** - Comprehensive utility functions for storage, date, string, number, validation, URL, and crypto operations
- **Internationalization** - Built-in i18n support (Chinese/English)
- **Dark Theme** - Full dark mode support via SettingDrawer

### Tech Stack

| Technology | Version | Description |
|------------|---------|-------------|
| React | 19.x | UI Framework |
| Ant Design | 6.x | UI Component Library |
| ProComponents | 3.x | Pro Business Components |
| Umi Max | 4.x | Enterprise Framework |
| TypeScript | 5.x | Type Safety |
| antd-style | 3.x | CSS-in-JS Solution |

### Project Structure

```
├── config/                 # Umi configuration
│   ├── config.ts          # Main config
│   ├── routes.ts          # Route config
│   └── defaultSettings.ts # Layout settings
├── mock/                   # Mock data
├── public/                 # Static assets
├── src/
│   ├── components/        # Shared components
│   │   ├── Footer/        # Page footer
│   │   ├── Logo/          # Brand logo
│   │   ├── RightContent/  # Header right area
│   │   ├── TagsView/      # Multi-tab navigation
│   │   └── TopHeader/     # Custom top header
│   ├── locales/           # i18n files
│   ├── models/            # Global state (useModel)
│   │   └── tabsView.ts    # Tab state management
│   ├── pages/             # Page components
│   │   ├── dashboard/     # Dashboard pages
│   │   ├── form/          # Form pages
│   │   ├── list/          # List pages
│   │   ├── profile/       # Profile pages
│   │   ├── result/        # Result pages
│   │   ├── exception/     # Error pages
│   │   ├── account/       # Account pages
│   │   └── user/          # Auth pages
│   ├── services/          # API services
│   ├── utils/             # Utility functions
│   │   ├── storage.ts     # LocalStorage with expiration
│   │   ├── dateHelper.ts  # Date utilities (dayjs)
│   │   ├── stringHelper.ts# String utilities
│   │   ├── numberHelper.ts# Number utilities
│   │   ├── objectHelper.ts# Object/Array utilities
│   │   ├── validation.ts  # Validation utilities
│   │   ├── urlHelper.ts   # URL utilities
│   │   └── cryptoHelper.ts# Crypto utilities
│   ├── app.tsx            # Runtime config
│   └── global.less        # Global styles
└── package.json
```

### Getting Started

```bash
# Install dependencies
npm install

# Start development server
npm run dev

# Build for production
npm run build

# Type check
npm run tsc
```

### Configuration

Edit `config/defaultSettings.ts` to customize:

```typescript
const Settings = {
  navTheme: 'light',           // Theme: 'light' | 'realDark'
  colorPrimary: '#1890ff',     // Primary color
  layout: 'side',              // Layout: 'side' | 'top' | 'mix'
  multiTabs: true,             // Enable multi-tab navigation
  fixedHeader: true,           // Fixed header
  fixSiderbar: true,           // Fixed sidebar
  // ...
};
```

### Screenshots

![Dashboard Analysis](./docs/images/dashboard.png)
*Dashboard Page*

The template includes the following pages:

- **Dashboard** - Analysis, Monitor, Workplace
- **Form** - Basic Form, Step Form, Advanced Form
- **List** - Table List, Basic List, Card List, Search List
- **Profile** - Basic Profile, Advanced Profile
- **Result** - Success, Fail
- **Exception** - 403, 404, 500
- **Account** - Account Center, Account Settings

### Credits

This project is based on [Ant Design Pro](https://github.com/ant-design/ant-design-pro) by the Ant Design Team. Special thanks to:

- [Ant Design](https://ant.design/) - The world-class React UI library
- [Ant Design Pro](https://pro.ant.design/) - Out-of-box UI solution for enterprise applications
- [Ant Design Pro Components](https://procomponents.ant.design/) - Pro business components
- [Umi](https://umijs.org/) - Pluggable enterprise-level React application framework

### License

MIT

---

<a name="中文"></a>

## 中文

基于 [Ant Design Pro](https://github.com/ant-design/ant-design-pro) 的增强型后台管理模板，提供多标签页导航、优化布局和丰富的工具函数。

### 特性

- **多标签页导航** - 浏览器风格的标签管理，支持右键菜单（重新加载、关闭其他、关闭左侧/右侧）
- **自定义顶部栏** - 优化的顶部布局，包含侧边栏折叠按钮、语言切换器和用户头像
- **最新技术栈** - React 19 + Ant Design 6 + Ant Design Pro Components 3 + TypeScript 5
- **多种布局模式** - 侧边菜单、顶部导航、混合布局
- **CSS-in-JS** - 使用 `antd-style` 的现代样式方案（createStyles）
- **丰富的工具函数** - 包含存储、日期、字符串、数字、验证、URL、加密等全面的工具函数
- **国际化** - 内置中英文国际化支持
- **暗色主题** - 通过 SettingDrawer 完整支持暗色模式

### 技术栈

| 技术 | 版本 | 说明 |
|------|------|------|
| React | 19.x | UI 框架 |
| Ant Design | 6.x | UI 组件库 |
| ProComponents | 3.x | Pro 业务组件 |
| Umi Max | 4.x | 企业级框架 |
| TypeScript | 5.x | 类型安全 |
| antd-style | 3.x | CSS-in-JS 方案 |

### 项目结构

```
├── config/                 # Umi 配置
│   ├── config.ts          # 主配置
│   ├── routes.ts          # 路由配置
│   └── defaultSettings.ts # 布局设置
├── mock/                   # Mock 数据
├── public/                 # 静态资源
├── src/
│   ├── components/        # 共享组件
│   │   ├── Footer/        # 页脚
│   │   ├── Logo/          # 品牌 Logo
│   │   ├── RightContent/  # 顶部右侧区域
│   │   ├── TagsView/      # 多标签页导航
│   │   └── TopHeader/     # 自定义顶部栏
│   ├── locales/           # 国际化文件
│   ├── models/            # 全局状态 (useModel)
│   │   └── tabsView.ts    # 标签页状态管理
│   ├── pages/             # 页面组件
│   │   ├── dashboard/     # 仪表盘页面
│   │   ├── form/          # 表单页面
│   │   ├── list/          # 列表页面
│   │   ├── profile/       # 详情页面
│   │   ├── result/        # 结果页面
│   │   ├── exception/     # 异常页面
│   │   ├── account/       # 账户页面
│   │   └── user/          # 认证页面
│   ├── services/          # API 服务
│   ├── utils/             # 工具函数
│   │   ├── storage.ts     # 本地存储（支持过期时间）
│   │   ├── dateHelper.ts  # 日期工具（基于 dayjs）
│   │   ├── stringHelper.ts# 字符串工具
│   │   ├── numberHelper.ts# 数字工具
│   │   ├── objectHelper.ts# 对象/数组工具
│   │   ├── validation.ts  # 验证工具
│   │   ├── urlHelper.ts   # URL 工具
│   │   └── cryptoHelper.ts# 加密工具
│   ├── app.tsx            # 运行时配置
│   └── global.less        # 全局样式
└── package.json
```

### 快速开始

```bash
# 安装依赖
npm install

# 启动开发服务器
npm run dev

# 生产构建
npm run build

# 类型检查
npm run tsc
```

### 配置

编辑 `config/defaultSettings.ts` 进行自定义：

```typescript
const Settings = {
  navTheme: 'light',           // 主题: 'light' | 'realDark'
  colorPrimary: '#1890ff',     // 主色
  layout: 'side',              // 布局: 'side' | 'top' | 'mix'
  multiTabs: true,             // 启用多标签页导航
  fixedHeader: true,           // 固定顶部栏
  fixSiderbar: true,           // 固定侧边栏
  // ...
};
```

### 页面截图

模板包含以下页面：

- **仪表盘** - 分析页、监控页、工作台
- **表单页** - 基础表单、分步表单、高级表单
- **列表页** - 查询表格、标准列表、卡片列表、搜索列表
- **详情页** - 基础详情、高级详情
- **结果页** - 成功页、失败页
- **异常页** - 403、404、500
- **个人页** - 个人中心、个人设置

### 致谢

本项目基于 Ant Design 团队的 [Ant Design Pro](https://github.com/ant-design/ant-design-pro)。特别感谢：

- [Ant Design](https://ant.design/index-cn) - 世界级的 React UI 组件库
- [Ant Design Pro](https://pro.ant.design/zh-CN/) - 开箱即用的企业级 UI 解决方案
- [Ant Design Pro Components](https://procomponents.ant.design/) - Pro 业务组件
- [Umi](https://umijs.org/docs/introduce/introduce) - 可插拔的企业级 React 应用框架

### 许可证

MIT

---

## References | 参考资料

- Ant Design Pro: https://github.com/ant-design/ant-design-pro
- Ant Design: https://github.com/ant-design/ant-design
- Ant Design Pro Components: https://github.com/ant-design/pro-components
- Umi: https://github.com/umijs/umi
