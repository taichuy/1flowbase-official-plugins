# 太初y 日历排盘 · Taichuy Calendar

独立 runtime extension，版本 **0.1.5**，manifest v2。中文说明见本文；
[English documentation](README.en.md)。

最低宿主版本字段为 **0.6.1**，且必须使用**包含本次新契约的宿主构建**（H1 自动注册及通用
factory/host bindings/去重配套）。已经发布但不包含这些改动的旧 0.6.1 构建仍不支持，需升级代码；
不能只按相同版本号判断可用性。

从用户批准的太初y 网站源码迁移，而不是重写的简化日历：保留月/年历、黄历、出生信息、
真太阳时、紫微斗数桌面/移动排盘、八字/大运/流年/流月与复制交互。不使用 iframe，不含旧站导航、
热榜、登录、接口服务、在线地理服务或后端预测。传统文化历法数据仅供文化兴趣与参考，
不构成命运判断。迁移原文、源哈希和适配边界见 [PROVENANCE](migration/PROVENANCE.md)
与 [inventory](migration/inventory.json)。

## 六个入口

组件 source 统一为 `@taichuy/taichuy-calendar`，公开 exports：

| Export | 功能 |
| --- | --- |
| `Calendar` | 可独立使用的原站月/年历，选择年月、今天、仅内存签到 |
| `Almanac` | 完整黄历：宜忌、纳音、冲煞、值神、时辰、建除、星宿、复制 |
| `BirthInfo` | 原出生日期/时间/省市区/经纬度/真太阳时/性别表单 |
| `ZiWei` | 原紫微斗数排盘、宫位交互、三方四正、运限切换、复制 |
| `Bazi` | 原八字、藏干十神、五行、八组大运、流年/流月展开、复制 |
| `CalendarPage` | 完整联动页面：黄历、表单与两种排盘；桌面右侧仅月历，移动月历在原位置 |

```tsx
import { CalendarPage } from '@taichuy/taichuy-calendar';
export default function Block() {
  return <CalendarPage />;
}
```

独立入口自己创建 React Context。完整页使用同一 provider；黄历日期与出生日期按源站设计分离，
更改月历不会改出生日期；出生日期、时间、地点、性别与真太阳时会实时驱动排盘。
这些入口作为分离的宿主 block 挂载时各自独立，不在全局变量中共享状态或 React。

所有入口支持 `types/index.d.ts` 中的可选初值：`initialDate`、`initialBirthDate`、
`initialTime`（HH:mm[:ss]）、`initialGender`、`initialLongitude`、`initialLatitude`、
`initialUseTrueSolarTime`、`locale`（zh/en）。初值只用于初始化；交互后以组件自身状态为准。
日期/天文校正沿用源站浏览器本地时区语义，不擅自改为 UTC 或出生地时区。
英文 locale 保留原站已有的副标题/引文/免责声明译文；传统图表和表单标签仍为中文。

## 宿主模块 ABI

`_assets/ui/browser.js` 是一个 ESM，**只导出** `createFrontendModule(hostModules)`。
宿主先解析 manifest 所有 `binding: host`，再把冻结的 ESM namespace map 传入 factory。
factory 返回上述六个组件。源码不从 blob import bare dependency，也不通过 globalThis 找 React。

```js
export function createFrontendModule(hostModules) {
  const module = { exports: {} };
  const exports = module.exports;
  const require = (source) => {
    if (!Object.hasOwn(hostModules, source)) throw new Error('Missing frontend host module: ' + source);
    return { ...hostModules[source], __esModule: true };
  };
  // esbuild 的 CJS bundle body；classic JSX transform，不需要 react/jsx-runtime。
  return module.exports;
}
```

`hostModules[source]` 是 namespace，不是 CommonJS 的 module.exports。其 `default` 需要显式提供；
以上适配创建新对象，既不修改 frozen namespace，也不会让 dayjs 的 default 变成整个 namespace。
精确 source/version/exports 在 [module-contract](scripts/module-contract.mjs)，全部 external：

- `react` **19.2.5**、`antd` **6.6.2**、`dayjs` **1.11.20**、`@ant-design/icons` **6.3.4**。
- 保留原样式还需要宿主已有的 `antd-style` **4.1.0**、`clsx` **2.1.1**。
- 原站已有 `copy-to-clipboard@3.3.3` 与其依赖、Antd 中文 locale 元数据和 dayjs 中文 locale
  side effect 随插件 bundle；它们不是 host binding，不新增下载，不修改复制反馈。dayjs 核心仍 external。
- 不打包 React、ReactDOM 或上述宿主模块。原始所有 CSS-in-JS 样式代码在 bundle 内，
  由宿主 antd-style 挂载；没有被遗漏的静态 CSS，也不需要伪造空 browser.css。

五个批准的业务依赖均入 bundle，另包含上述原站已有支持代码；源码安装中版本不符立即停止：
`lunar-typescript@1.8.6`、`tyme4ts@1.3.9`、`iztro@2.5.3`、
`astronomy-engine@2.1.19`、`china-division@2.7.0`。其实际传递依赖也由 bundle 隔离，
不要求宿主提供。完整许可证含 Astronomy Engine 源头的完整 MIT `@preserve` 声明。

## 离线构建与发布装配

不运行 npm/pnpm install、npx 或联网下载。构建复用已安装的工具/依赖；需要时显式设置：

```bash
export TAICHUY_HOST_ROOT=/home/taichuy/git/1flowbase
export TAICHUY_SOURCE_ROOT=/home/taichuy/git/taichu-y-web-master
# 宿主不能直接 resolve esbuild 时，指向已有 esbuild/lib/main.js；脚本不会安装它。
export ESBUILD_MODULE=/absolute/path/to/existing/esbuild/lib/main.js
node scripts/build.mjs
# 以上生成真实 browser.js、manifest.yaml、完整 licenses 与 bundle-contract.json。
# 仅 Root 集中构建时执行以下步骤；D1 源码包不执行 Cargo：
node scripts/build-runtime.mjs
node scripts/check-package.mjs
```

运行目录为本插件目录。`build-runtime.mjs` 使用 offline/locked Cargo，可选
`CARGO_BUILD_TARGET`/`CARGO_TARGET_DIR`。本插件没有 build.rs，不在 Cargo build 过程中隐式运行 npm。
Rust binary 无依赖，仅满足现有宿主 packager 的 runtime.entry 要求；收到任何运行时操作都拒绝，
不是业务 stub/后端服务。实际入口全部前端执行；按当前 v2 schema，每个 frontend payload 显式声明
一个 `declarative_only` execution binding，指向此 guard，只作声明归属，不启动后端操作。
包级和 block 权限均禁止网络、持久化、secrets；不申请 MCP、子进程、宿主数据或预测接口。
复制只发生在用户触发的本机剪贴板操作。

**发布必须提交生成资产。** 官方 provider-release 只执行 Cargo + 宿主 package，不会自动前端 build。
Root 必须把 `_assets/ui/browser.js`、`_assets/ui/THIRD_PARTY_LICENSES.txt`、`manifest.yaml`、
`migration/bundle-contract.json` 与源码一起装配提交后再发布；不要只提交脚本。
manifest 的 fetched asset 摘要必须对应真实 JS 字节，构建后修改源/契约需重新生成。
构建重复使用相同输入与 esbuild 版本；构建记录保存实际工具版本与依赖集合。
安装包由主仓宿主 CLI 的标准 package 命令制作，不是 npm 包。

## Root 集中 QA（D1 只写 fixture，不运行）

```bash
export TAICHUY_HOST_ROOT="${TAICHUY_HOST_ROOT:-/home/taichuy/git/1flowbase}"
export TAICHUY_EVIDENCE_ROOT="${TAICHUY_EVIDENCE_ROOT:-$TAICHUY_HOST_ROOT/tmp/test-governance/taichuy-calendar}"
export TAICHUY_CHROME_EXECUTABLE="${TAICHUY_CHROME_EXECUTABLE:-/usr/bin/google-chrome}"
node scripts/build-fixtures.mjs
TZ=Asia/Shanghai node --test "$TAICHUY_EVIDENCE_ROOT/compiled/algorithms.test.cjs"
node scripts/run-component-fixtures.mjs
```

以上命令从插件根目录执行；`npm run fixtures:prepare` / `npm run fixtures:components`
仍分别调用相同的准备 / 组件脚本。可用 `TAICHUY_EVIDENCE_ROOT` 指定取证目录；算法命令须使用同一目录。
准备脚本需要真实已生成的 browser.js 和已有宿主 React/Antd/Playwright，不安装依赖、不启动服务。
浏览器 runner 默认使用本机 `/usr/bin/google-chrome`；其他位置通过 `TAICHUY_CHROME_EXECUTABLE`
显式指定已有 Chrome/Chromium 可执行文件。路径无效即失败，不下载或依赖 Playwright headless_shell。
算法 fixture 在固定时区/时钟下，对比源哈希校验后从**原文件**截取的计算体，保留 6 个出生边界案例、
24 组真太阳时对照，以及固定 `2026-11-01` 的黄历、八字、紫微与提示词预期。
紫微序列化在实际值和源站参考值两侧同时排除 `astrolabe` / `_astrolabe` 反向引用及方法；
`FunctionalPalace._astrolabe` 指回所属星盘，不是额外业务数据，其余星曜、宫位、运限数据仍完整比较。
组件 fixture 用实际 factory + 冻结的 namespace 执行，缺少 dayjs 绑定必须拒绝；六入口均真实挂载。
fixture 不预加载 locale/clipboard，断言 factory 将中文 locale 注册到同一个宿主 dayjs 核心。
桌面/移动交互覆盖黄历与出生日期分离、日期/时间/性别/经纬与真太阳时联动、月/年/今天/签到、
紫微宫位/运限、八字大运/流年展开与全部复制路径，并阻断网络、保存截图/文本/报告。
QA-only host bundle 不进入安装资产，也不代表插件私带 React。
浏览器使用可见日历所属 Card 的作用域、Antd 两字按钮空格、黄历 hover 后复制、Cascader 的真实
combobox 与带图标前缀的复制按钮名称；不 force click、不跳过断言。桌面 / 移动端失败都会使报告失败，
并在 finally 中关闭 context / Chrome。原报告路径保持为 `$TAICHUY_EVIDENCE_ROOT/component-fixtures.json`。
PF1 只将 QA1 已证明等价的 fixture 修正纳回原命令；此修复未运行构建或行为测试，等待 Root 集中 QA2。
QA1 的临时 runner 通过不代表本次修正候选已通过，也不代表真实宿主页或系统剪贴板已验收。

**本源码 packet 未运行构建、测试、服务、Cargo 或安装。** Root 还需完成通用 host factory、
host bindings、重复模块去重与 UI-only block 自动注册契约，并集中验证安装/禁用/卸载、编辑器入口和
视觉/行为。fixtures 的存在不等于通过，原站截图不等于新插件截图；未完成前不得宣称云端可安装。

## 0.1.5 — 原生区块样式前缀

Ant Design 局部覆写使用当前 `createStyles` 的 `prefixCls`，不再假定固定的 `ant`。
宿主使用隔离前缀时，原出生表单、卡片与日期弹层规则仍然生效。此次只适配选择器，
不改变历法算法、不覆盖宿主 Provider / portal 归属；原生表面需包含宿主样式 hooks。

## UI 注册边界（0.1.5）

六个 `block_contributions` 显式声明 `ui_registration: component`，只自动注册到
`/settings/ui-management/components`，不再作为官方条目出现在代码模板列表。
`code_template` 保留为组件完整挂载源码，不代表注册到代码模板。
底座负责根据声明进行注册、升级、只读归属和卸载清理；插件不直接写组件表。
需要支持该字段的底座源码；已发布旧版 0.6.1 不支持此契约。
