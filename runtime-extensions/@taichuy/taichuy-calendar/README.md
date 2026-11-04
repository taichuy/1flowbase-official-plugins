# 太初y 日历排盘 · Taichuy Calendar

独立 runtime extension，版本 **0.1.15**，manifest v2。中文说明见本文；
[English documentation](README.en.md)。

最低宿主版本为 **0.6.2**；宿主旧版 0.6.1 不具备本版本的执行预算和混合贡献契约。
正式目录使用既有 minimum_host_version 检查，不应绕过低版本警告强行启用。
构建时 Cargo SDK git rev 与云打包 checkout 绑定同一个明确宿主 SHA；不是把宿主 main/dev 当作固定契约。

从用户批准的太初y 网站源码迁移，而不是重写的简化日历：保留月/年历、黄历、出生信息、
真太阳时、紫微斗数桌面/移动排盘、八字/大运/流年/流月与复制交互。不使用 iframe，不含旧站导航、
热榜、登录、接口服务、在线地理服务或后端预测。新增后端接口仅复用同一批 npm 本地计算算法，不提供预测服务。传统文化历法数据仅供文化兴趣与参考，
不构成命运判断。迁移原文、源哈希和适配边界见 [PROVENANCE](migration/PROVENANCE.md)
与 [inventory](migration/inventory.json)。

## 公共组件与组合入口

组件 source 统一为 `@taichuy/taichuy-calendar`，公开 exports：

| Export | 功能 |
| --- | --- |
| `Calendar` | 可独立使用的原站月/年历，选择年月、今天、仅内存签到 |
| `Almanac` | 完整黄历：宜忌、纳音、冲煞、值神、时辰、建除、星宿、复制 |
| `BirthInfo` | 原出生日期/时间/省市区/经纬度/真太阳时/性别表单 |
| `ZiWei` | 原紫微斗数排盘、宫位交互、三方四正、运限切换、复制 |
| `Bazi` | 原八字、藏干十神、五行、八组大运、流年/流月展开、复制 |
| `CalendarPage` | 旧版整页兼容导出；新页面使用下方显式组合模板 |

```tsx
import { CalendarProvider, Calendar, Almanac, BirthInfo, ZiWei, Bazi } from '@taichuy/taichuy-calendar';
export default function Block() {
  return <CalendarProvider>
    <Calendar /><Almanac /><BirthInfo /><ZiWei /><Bazi />
  </CalendarProvider>;
}
```

推荐从 `examples/composed-page.tsx` 开始：响应式列布局、Tab、锚点滚动均在可编辑源码中，
不是 `<CalendarPage />` 黑盒。`CalendarProvider` 为同一树下组件共享状态；不同 Provider 与
不同宿主区块互相隔离。日历联动黄历，出生日期/时间/地点/性别/真太阳时联动两个排盘；
黄历日期不改变出生日期。无 Provider 的五个独立组件仍可单独使用。

- `useCalendarDate()`：读写日历日期；`useBirthInfo()`：读写出生信息与排盘状态。
- 更底层的受控组件：`LunarInfoCard({date})`、`ZiWeiCalculator({date,time,gender,locationInfo})`、
  `BaziCalculator`（同上）、`LocationSelector({value,onChange})`。置于 Provider 内以复用主题/语言。
- `Calendar({onAlmanac})` 可把黄历按钮连接到自定义页面锚点。
- `CalendarPage` 导出及 contribution 暂时保留兼容；不再作为新页面的推荐组合入口。
- 可选初始值见 `types/index.d.ts`。它们只初始化状态，不是受控 props；出生时间使用浏览器本地
  民用时间语义，不自动转换为出生地时区。`locale` 仅调整原站引语/免责声明，传统术语保留中文。

### MCP 可编辑的 AI 解读助手

`ZiWei`、`Bazi` 及其底层 Calculator 支持 `renderAIHelper(data)`，替换整个助手区域。
`examples/composed-page.tsx` 在页面源码内定义助手的文案、按钮、布局及数据展示，MCP 可直接编辑该源码。
插件继续负责排盘和运限选择，实时传入与原复制按钮相同的完整 `natalText`；紫微还提供
`horoscopeText`、`viewDate`、`copyHoroscope()`，八字提供 `selectedDaYunIndex`，两者均有 `copyNatal()`。
修改出生信息或运限后使用当前数据；不传 renderer 时保持原助手，返回 `null` 时仅省略助手。

```tsx
<ZiWei renderAIHelper={(data) => <section>
  <h3>我的排盘助手</h3>
  <button onClick={data.copyNatal}>复制排盘</button>
  <pre>{data.natalText}</pre>
</section>} />
```

这些是浏览器渲染数据，不会自动保存为记录，也不使外部 MCP 能读取用户浏览器内存。
外部 Agent 需要计算数据时可通过已有托管服务 `bazi.calculate` / `ziwei.calculate`
以明确出生输入重新计算；页面实时选择和服务请求输入不是同一个持久状态。

### 直接使用现有业务包

下列五包是**已有依赖开放低代码 import**，版本不变、无新增下载。它们通过本插件的
`code_modules` 授权，不成为宿主全局 npm 包，也不开放任意 npm import。

| import source | 版本 | 开放范围 |
|---|---|---|
| `lunar-typescript` | 1.8.6 | 原包公共运行时导出及类型 |
| `tyme4ts` | 1.3.9 | 原包公共运行时导出及类型 |
| `iztro` | 2.5.3 | `astro`、`star`、`util`、`data` 及类型 |
| `astronomy-engine` | 2.1.19 | 浏览器可用公共导出及类型 |
| `china-division` | 2.7.0 | 浏览器省/市/区子集：`provinces,cities,areas,pc,pcC,pca,pcaC` 与 default；不含 Node fs/path、街道/村数据或任意 JSON 子路径 |

```tsx
import { Solar } from 'lunar-typescript';
import { SolarDay } from 'tyme4ts';
import { astro } from 'iztro';
import { SunPosition } from 'astronomy-engine';
import { pcaC } from 'china-division';
export default function CustomCalendarSummary() {
  const lunar = Solar.fromYmd(2026, 11, 1).getLunar();
  return <div>{lunar.toString()} · {SolarDay.fromYmd(2026, 11, 1).toString()} · {pcaC[0].name}</div>;
}
```

### 页面模板与组件管理

五个业务组件仍自动注册到 UI 组件管理；旧版整页记录仅为兼容保留，Provider/hooks/计算包不伪装成 UI 组件。
页面组合可保存/发布到宿主 JSX 编辑器的“页面模板”（底层为 managed code template），不重复注册五个组件为代码模板。
完整可移植页面包另由 `node scripts/build-page-template.mjs` 生成至
`applications-demo/@taichuy/calendar-composition` 与 `_assets/templates/calendar-composition.zip`，
通过现有应用模板导入/预览/安装流程使用。导入后创建新页面，页面/区块源码可继续编辑。
模板不复制用户已有页面 ID，不携带登录信息，安装时由宿主映射插件安装身份。
源文件生成不等于已发布官方签名云端模板。

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

## 0.1.6 — 原生区块样式前缀

Ant Design 局部覆写使用当前 `createStyles` 的 `prefixCls`，不再假定固定的 `ant`。
宿主使用隔离前缀时，原出生表单、卡片与日期弹层规则仍然生效。此次只适配选择器，
不改变历法算法、不覆盖宿主 Provider / portal 归属；原生表面需包含宿主样式 hooks。

## UI 注册边界（0.1.7）

六个 `block_contributions` 显式声明 `ui_registration: component`，只自动注册到
`/settings/ui-management/components`，不再作为官方条目出现在代码模板列表。
`code_template` 保留为组件完整挂载源码，不代表注册到代码模板。
底座负责根据声明进行注册、升级、只读归属和卸载清理；插件不直接写组件表。
需要支持该字段的底座源码；已发布旧版 0.6.1 不支持此契约。

## 0.1.7 — 可编辑组合与底层包导入

以 Provider + 多个公共组件替代新页面的整页黑盒；保留旧 CalendarPage。五个现有业务包可在区块 JSX 中按原 source 导入。iztro 类型只打包公开入口可达依赖；页面模板使用宿主支持的顶栏根路由。

## 0.1.8 — 独立日历栏

桌面组合保持左右分栏，右侧日历随左侧内容滚动时保持可见；矮窗口中日历栏可独立滚动，滚轮不传递到左侧。手机端保留单列日历。布局仍由 `examples/composed-page.tsx` 的可编辑源码负责，页面模板同步提供相同组合，未增加依赖或隐藏页面封装。

## 0.1.9 区块内布局比例

组合页使用 Ant Design `Flex`，以区块内容宽度为准，按「左侧留白 : 详情 : 间隔 : 日历 = 1 : 6 : 1 : 2」分配空间。区块宽度小于 1200px 时收起左留白并保留 280px 日历栏及 24px 间隔；不超过 768px 时改为单列日历。右栏继续保持 sticky 与独立滚动；可编辑组合和兼容整页、页面模板采用相同布局。

## 0.1.10 — 缩小留白并加宽日历

区块内比例调整为「左留白 : 详情 : 间隔 : 日历 = 0.3 : 6 : 0.3 : 3.4」，两处留白省出的宽度全部分配给日历。中等宽度仍采用相同比例，必要时为日历保留 280px 最小宽度；不超过 768px 继续单列。可编辑组合、兼容整页和页面模板同步更新，保留 sticky 与独立滚动。

## 后端计算接口（0.1.15）

后端通过正式 managed-service 贡献注册，由底座处理发现、安装/授权、调用者权限和进程生命周期。
插件是 system process_per_call 服务；原有 UI 绑定仍是 declarative_only，**仍须显式分配到目标工作区**且具备对应贡献授权，不因系统启用而对所有工作区显示。
运行时无需 Node/npm/pnpm 或 node_modules。npm/esbuild 只用于构建；Rust 共享 SDK 可选 javascript feature 内嵌已有 QuickJS。

所有路由为 `POST /api/console/managed-services/taichuy-calendar/` 加下列后缀；JSON body 见 scripts/runtime-contract.mjs，完整输出 schema 由 scripts/runtime-schemas.mjs 定义。

| contribution/interface | 后缀 | 业务范围 |
|---|---|---|
| taichuy-calendar.calendar.month | calendar/month | 月历及逐日农历/节日 |
| taichuy-calendar.calendar.convert | calendar/convert | 公农历双向转换（显式闰月） |
| taichuy-calendar.almanac.day | almanac/day | 原黄历、宜忌、时辰吉凶 |
| taichuy-calendar.birth.normalize | birth/normalize | 民用日期/时间、固定 offset、出生地规范化 |
| taichuy-calendar.time.true-solar | time/true-solar | 复用 astronomy-engine 真太阳时，秒精度 |
| taichuy-calendar.bazi.calculate | bazi/calculate | 原八字算法、八组大运/流年/流月、显式 as_of |
| taichuy-calendar.ziwei.calculate | ziwei/calculate | 原紫微算法、十二宫和显式时刻运限 |
| taichuy-calendar.regions.children | regions/children | china-division 的省市区子级查询 |

日期不统一收窄为 1901–2099。先校验真实公历日期，再由对应上游算法的实际范围约束；紫微使用 iztro 2.5.3 明确的 1900-01-31 至 2100-12-31（包括两端）。
输入 `civil_date`、`civil_time`、`utc_offset_minutes` 定义固定 offset，不猜出生地时区，不读机器默认 TZ；`as_of`/`as_of_time` 明确给出，不隐式用 now。
`UTC = civil-as-UTC - utc_offset_minutes * 60000`。秒精度真太阳时的 correction_seconds 与返回民用时间的算术差保持一致。

业务错误使用 `{ok:false,error:{code,message}}`；资产、协议或引擎错误使用原 CapabilityStdioResponse 的**外层**失败，不能用业务成功掩盖执行失败。
Node 参照和真实 Rust 接口须按相同版本包装比较，且独立日期/UTC 算术 oracle 先成立。方法与有限证明边界见 migration/runtime-contract.md。

### 接口权限与设置入口

日历插件不注册后台设置页。后端计算接口按稳定插件标识 `taichuy-calendar` 注册到权限管理的「插件」页签，可按角色设置启用、完全访问或自定义接口访问；前端组件继续通过受管接口调用计算能力。
