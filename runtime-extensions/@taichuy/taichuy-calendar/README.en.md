# Taichuy Calendar · 太初y 日历排盘

Independent **0.1.18** runtime extension, manifest v2. [中文说明](README.md).
Minimum host version is **0.6.2**, **with a build containing the new contracts**:
H1 component projection plus factory/host-binding/deduplication support. An older
published 0.6.1 build without those changes is NOT sufficient; upgrade the host
code rather than relying on the version number alone.
This is a migration of the user-approved original site, not a simplified rewrite.
It preserves the lunar calendar, almanac, birth form, astronomical true-solar-time
correction, desktop/mobile ZiWei chart, Bazi luck tables and clipboard actions.
There is no iframe, old-site navigation/hot list/login, prediction service,
persistence or online geocoding. Traditional calendrical data is for cultural
interest/reference, not a claim about fate. See [provenance](migration/PROVENANCE.md)
and the source SHA-256 [inventory](migration/inventory.json).

## Public components

The five panels and the legacy compatibility page share source `@taichuy/taichuy-calendar`:

| Export | Original functionality |
| --- | --- |
| `Calendar` | Independent lunar month/year calendar, header selection and return to today |
| `Almanac` | Full almanac, auspicious/inauspicious items, hourly luck, stars and clipboard |
| `BirthInfo` | Date/time, province/city/district, coordinates, true solar time and gender |
| `ZiWei` | Twelve palaces, focus/connected palaces, mobile details, horoscope stepping and copy |
| `Bazi` | Pillars, hidden stems/ten gods, five elements, eight decades, years/months and copy |
| `CalendarPage` | Legacy compatibility export; new pages use the explicit composition below |

```tsx
import { CalendarProvider, Calendar, Almanac, BirthInfo, ZiWei, Bazi } from '@taichuy/taichuy-calendar';
export default function Block() {
  return <CalendarProvider>
    <Calendar /><Almanac /><BirthInfo /><ZiWei /><Bazi />
  </CalendarProvider>;
}
```

Start with `examples/composed-page.tsx`: responsive layout, tabs and scroll targets live
in editable JSX, not a hidden `<CalendarPage />`. One `CalendarProvider` links sibling
components; separate providers and separate host blocks remain isolated. Calendar
selection updates the almanac without changing birth date. Birth date/time/location,
gender and true-solar-time edits update both charts. The five standalone components
still supply their own boundary when no provider is present.

`useCalendarDate()` exposes calendar state; `useBirthInfo()` exposes birth/chart state.
Controlled primitives are also available: `LunarInfoCard({date})`, `ZiWeiCalculator`
and `BaziCalculator` with `{date,time,gender,locationInfo}`, and
`LocationSelector({value,onChange})`. Put them inside a provider for theme and locale.
The calendar action bar only offers Today: it resets the selected date and month view and updates the almanac. Sign-in and redundant almanac navigation are removed.
`CalendarPage` and its contribution remain for compatibility, not as the recommended
new-page entry. Initialization props in `types/index.d.ts` initialize rather than
control state. Civil times retain browser-local semantics, not birth-location timezone
conversion; locale changes the original quotation/disclaimer, not traditional terms.

### Import the existing business packages

This release exposes **existing dependencies to authorized low-code imports**; it adds
no new dependencies or downloads. These sources are scoped to the plugin's authorized
`code_modules`, not global host npm availability or unrestricted npm imports.

| Source | Version | Browser surface |
|---|---|---|
| `lunar-typescript` | 1.8.6 | Upstream public runtime exports and types |
| `tyme4ts` | 1.3.9 | Upstream public runtime exports and types |
| `iztro` | 2.5.3 | `astro`, `star`, `util`, `data` and types |
| `astronomy-engine` | 2.1.19 | Public browser-compatible exports and types |
| `china-division` | 2.7.0 | Province/city/district subset: `provinces,cities,areas,pc,pcC,pca,pcaC` and default; no Node fs/path, street/village data or arbitrary JSON subpaths |

```tsx
import { Solar } from 'lunar-typescript';
import { SolarDay } from 'tyme4ts';
import { astro } from 'iztro';
import { SunPosition } from 'astronomy-engine';
import { pcaC } from 'china-division';
export default function CustomCalendarSummary() {
  return <div>{Solar.fromYmd(2026, 11, 1).getLunar().toString()} · {SolarDay.fromYmd(2026, 11, 1).toString()} · {pcaC[0].name}</div>;
}
```

### Templates and component management

The five components continue to register in UI component management; the legacy full-page
record remains only for compatibility. Providers, hooks and calculation packages are not
registered as UI components. Save/publish the composed source through the JSX editor's
page-template chooser (a managed code template); do not duplicate component registrations.
`node scripts/build-page-template.mjs` also generates the portable full-page template in
`applications-demo/@taichuy/calendar-composition` and
`_assets/templates/calendar-composition.zip`. Use the existing application-template
preview/import/install flow to create an independently editable page. Stable sample IDs
are distinct from the user's page; installation identities are remapped by the host.
No session or credential data is included. Generating these files is not a signed cloud release.

## Frontend factory ABI

The ESM asset `_assets/ui/browser.js` exports only
`createFrontendModule(hostModules)`. The host first resolves manifest bindings
and passes frozen ESM namespaces keyed by their exact sources. The factory
returns the declared public components, provider, hooks and controlled primitives. It has no bare ES imports, global React
bridge, ReactDOM dependency or `react/jsx-runtime` external.

```js
export function createFrontendModule(hostModules) {
  const module = { exports: {} };
  const exports = module.exports;
  const require = (source) => {
    if (!Object.hasOwn(hostModules, source)) throw new Error('Missing frontend host module: ' + source);
    return { ...hostModules[source], __esModule: true };
  };
  // esbuild CJS body, compiled using classic JSX.
  return module.exports;
}
```

A host namespace is not the CJS module value. `.default` must be present where
[the contract](scripts/module-contract.mjs) requests it. Interop makes a new
object and never mutates the frozen namespace. This prevents a namespace object
being mistaken for the dayjs default function.

All of these existing host modules are external, with the locally inspected target versions:

- `react@19.2.5`, `antd@6.6.2`, `dayjs@1.11.20`, `@ant-design/icons@6.3.4`.
- To retain source styles: `antd-style@4.1.0`, `clsx@2.1.1`.
- Existing source-site `copy-to-clipboard@3.3.3` and its dependencies, Antd Chinese
  locale metadata and the dayjs Chinese locale side effect are bundled, NOT host
  bindings. No new download or clipboard feedback change is required; dayjs core
  remains external.

Original scoped CSS-in-JS objects live in the bundle and execute through host
antd-style. There is no missing static stylesheet and no artificial empty
browser.css. React, ReactDOM and every host package must not enter the release
bundle, except for the narrowly allowed Chinese locale metadata. The build
enforces this boundary.

The five approved business dependencies are bundled alongside the existing
support code described above:
`lunar-typescript@1.8.6`, `tyme4ts@1.3.9`, `iztro@2.5.3`,
`astronomy-engine@2.1.19`, `china-division@2.7.0`. Their needed transitive code is
bundled too, not expected from the host. All bundled licenses are retained,
including Astronomy Engine's full MIT notice from its `@preserve` source header.

## Offline build and release assembly

No install, npx, package download or implicit Cargo frontend build is performed.
Use existing local dependencies and an existing esbuild installation:

```bash
export TAICHUY_HOST_ROOT=/home/taichuy/git/1flowbase
export TAICHUY_SOURCE_ROOT=/home/taichuy/git/taichu-y-web-master
# Needed if esbuild cannot be resolved directly from an existing toolchain:
export ESBUILD_MODULE=/absolute/path/to/existing/esbuild/lib/main.js
node scripts/build.mjs
node scripts/build-page-template.mjs
# Root alone performs centralized runtime build and release readiness checking:
node scripts/build-runtime.mjs
node scripts/check-package.mjs
```

Run from this plugin directory. The runtime script supports `CARGO_BUILD_TARGET`
and `CARGO_TARGET_DIR`, and uses offline/locked Cargo. The dependency-free Rust
binary only satisfies the existing packager's runtime.entry requirement. It
rejects every operation, is not a prediction stub/service, and is not bound to
any business operation. All contributions execute in the frontend. Each frontend payload has an explicit
`declarative_only` execution binding pointing to the guard, as required by the
current v2 schema. This records payload ownership, not a backend operation.
Network, persistent storage and secrets are denied;
no host data, MCP or subprocess permissions are requested. Clipboard writes are
local and explicitly user-triggered.

**Generated assets must be committed before release.** provider-release builds
Cargo and invokes the host package CLI, but does not build the frontend. Root
must assemble and commit `_assets/ui/browser.js`,
`_assets/ui/THIRD_PARTY_LICENSES.txt`, `manifest.yaml`, and
`migration/bundle-contract.json` alongside the source. The manifest uses the
real JS SHA-256, never a placeholder. Rebuild whenever inputs/contracts change;
use the same inputs/tool version for reproducibility. Tool version and bundled
package names are recorded. Use the host's standard package command to create
`.1flowbasepkg`; this is not a separately installed npm package.

## Centralized Root QA (fixtures written, not run by D1)

```bash
export TAICHUY_HOST_ROOT="${TAICHUY_HOST_ROOT:-/home/taichuy/git/1flowbase}"
export TAICHUY_EVIDENCE_ROOT="${TAICHUY_EVIDENCE_ROOT:-$TAICHUY_HOST_ROOT/tmp/test-governance/taichuy-calendar}"
export TAICHUY_CHROME_EXECUTABLE="${TAICHUY_CHROME_EXECUTABLE:-/usr/bin/google-chrome}"
node scripts/build-fixtures.mjs
TZ=Asia/Shanghai node --test "$TAICHUY_EVIDENCE_ROOT/compiled/algorithms.test.cjs"
node scripts/run-component-fixtures.mjs
```

Run these commands from the plugin root. `npm run fixtures:prepare` and
`npm run fixtures:components` still invoke the same preparation and browser scripts.
`TAICHUY_EVIDENCE_ROOT` overrides the evidence location; use that same directory for
the algorithm command. Fixtures require a real built browser.js and existing host
React/Antd/Playwright. They start no server, install/download nothing, and block network.
The browser runner defaults to the existing `/usr/bin/google-chrome`. Set
`TAICHUY_CHROME_EXECUTABLE` to an existing Chrome/Chromium executable elsewhere.
An invalid path fails explicitly; no Playwright headless_shell is required or downloaded.
Algorithms compare to hash-checked bodies extracted from the **original source**,
with a fixed clock and Shanghai timezone: six birth-boundary cases, 24 true-solar-time
comparisons, and fixed `2026-11-01` almanac, Bazi, ZiWei and prompt expectations.
ZiWei serialization excludes `astrolabe` / `_astrolabe` reverse references and methods
on BOTH actual and reference values. `FunctionalPalace._astrolabe` points back to its
owning chart, not additional business data; all other star, palace and horoscope data
remain part of the full comparison.
The browser fixture executes the actual factory with frozen namespace bindings,
rejects a missing dayjs binding, mounts every export, and exercises desktop/mobile
calendar/form/chart/clipboard paths. It saves text, screenshots and a report.
The QA-only host bundle is not shipped and does not make the plugin privately
bundle React.
Browser interactions use the visible calendar's Card scope, Antd's Chinese button
spacing, hover-revealed almanac copy, the actual Cascader combobox, and icon-prefixed
copy-button names. No forced clicks or skipped assertions are used. Failures in either
viewport fail the report; contexts and Chrome close in finally. The report remains at
`$TAICHUY_EVIDENCE_ROOT/component-fixtures.json`.
PF1 integrates the QA1-proven equivalent fixture corrections into the original commands;
this repair has not run builds or behavioral tests and awaits centralized Root QA2.
The passing temporary QA1 runner is not a pass for this repaired candidate, installed
host-page integration, or the physical system clipboard.

This source packet has **not** run a build, Cargo, tests, server, browser or
installer. Root still owns generic factory loading, the additional host sources,
module deduplication and UI-only automatic component registration, followed by
centralized install/disable/uninstall and visual/interaction acceptance. Existing
source screenshots are inputs, not proof of the migrated plugin. Do not call
this source-only packet a cloud-installable release before Root's asset assembly.

## 0.1.6 — Native surface style prefixes

Scoped Ant Design overrides use the owning `createStyles` `prefixCls` instead of
assuming `ant`. This preserves the original form/card/picker rules when the host
uses isolated native prefixes; it does not change algorithms or override host
provider/portal ownership. The native surface must contain the host style hooks.

## UI registration boundary (0.1.7)

The six reusable component contributions explicitly declare `ui_registration: component`. They are
registered only in `/settings/ui-management/components`, not as official code templates.
`code_template` remains the component mounting source. A separate `CalendarComposition` contribution declares `ui_registration: code_template`, registering the complete editable block at `/settings/ui-management/code-templates`. Installation creates it when absent and upgrades update the same stable `taichuy-calendar / CalendarComposition` identity; repeated activation does not add duplicates. Its source is `examples/composed-page.tsx`, including the editable assistant and full calendar/almanac/birth/chart composition. Plugin templates and manual copies have separate identities; unrelated manual content is never matched by display name or overwritten.
The host owns registration, upgrade, read-only provenance and uninstall cleanup. The plugin never writes component tables.
A host source build supporting this field is required; the previously published 0.6.1 release does not support this contract.

## 0.1.7 — Editable composition and direct calculation imports

New pages compose a Provider and public components instead of a full-page wrapper; CalendarPage remains compatible. Five existing business packages are importable by their original source names. iztro declarations include only dependencies reachable from its public entry, and the portable template uses the host-supported topbar root route.

## 0.1.8 — Independent calendar rail

The desktop composition keeps a sticky right calendar beside the scrolling left panels. In short viewports the calendar rail scrolls independently without chaining wheel input to the left. Mobile keeps one inline calendar. Layout remains editable in `examples/composed-page.tsx` and is also shipped in the portable page template; no new dependency or opaque page wrapper is introduced.

## 0.1.9 Block-relative layout proportions

The composition uses Ant Design `Flex` with a leading-space : detail : gap : calendar ratio of **1 : 6 : 1 : 2**, measured within the block content width. Below 1200px it removes the leading space and keeps a 280px calendar rail with a 24px gap; at 768px or less it shows one inline calendar. The rail retains sticky positioning and independent overflow. The editable composition, compatibility page and portable template share this layout.

## 0.1.10 — Smaller gaps, wider calendar

The block-relative leading-space : detail : gap : calendar ratio is now **0.3 : 6 : 0.3 : 3.4**. Both reduced spaces are reassigned to the calendar. Medium blocks retain this ratio with a 280px minimum calendar width where necessary; blocks at 768px or less remain single-column. Editable composition, compatibility page and portable template stay aligned, with sticky positioning and independent scrolling preserved.

## Embedded computation API (0.1.18)

Requires host **0.6.2** (not an older 0.6.1 build). The existing official catalog minimum-host check and exact SDK git SHA pin are distinct: one checks installation compatibility, the other pairs release builds.
The eight managed-service POST operations are declared in scripts/runtime-contract.mjs; output DTOs in scripts/runtime-schemas.mjs. Node/npm are build-only; the published Rust executable runs the closed _assets/runtime/backend.js using the optional shared SDK JavaScript feature. No node_modules, private HTTP server, runtime package download, or Node compatibility layer.
Existing frontend components, low-code npm modules and editable page template remain available with explicit workspace assignment and contribution grants.
Civil time uses an explicit fixed utc_offset_minutes and explicit as_of; it never infers host TZ or a current business date. Only Ziwei adopts iztro's actual inclusive 1900-01-31 through 2100-12-31 bounds; other endpoints retain their own upstream constraints.
See migration/runtime-contract.md for the specification/implementation/verifier mapping and proof limitations.

### API permissions and settings navigation

The calendar plugin does not register an admin settings page. Its backend calculations are grouped under the stable plugin ID `taichuy-calendar` in the role permissions **Plugins** tab, with enabled, full-access and custom operation policies. Frontend components continue to call the managed calculation interfaces.

## MCP-editable interpretation assistant

`ZiWei`, `Bazi`, and their Calculator primitives accept `renderAIHelper(data)` to replace the entire assistant. The editable page in `examples/composed-page.tsx` owns its copy, buttons, layout, and data display, so MCP can edit the block source directly. The plugin keeps chart calculation and navigation and passes the complete `natalText` used by the original copy action, plus `copyNatal()`. Ziwei also provides `horoscopeText`, `viewDate`, and `copyHoroscope()`; Bazi provides `selectedDaYunIndex`. Birth and horoscope changes supply current data. Omit the prop for the original assistant, or return `null` to omit only the assistant.

These values are browser render data, not automatically persisted records or external MCP access to browser memory. An external agent can calculate chart data through the existing `bazi.calculate` / `ziwei.calculate` managed services with explicit birth inputs; that request does not share the browser's live selection.
