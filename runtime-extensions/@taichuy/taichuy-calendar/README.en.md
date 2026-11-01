# Taichuy Calendar · 太初y 日历排盘

Independent **0.1.0** runtime extension, manifest v2. [中文说明](README.md).
Minimum host version is **0.6.1**, **with a build containing the new contracts**:
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

All six exports have source `@taichuy/taichuy-calendar`:

| Export | Original functionality |
| --- | --- |
| `Calendar` | Independent lunar month/year calendar, header selection, today, memory-only sign |
| `Almanac` | Full almanac, auspicious/inauspicious items, hourly luck, stars and clipboard |
| `BirthInfo` | Date/time, province/city/district, coordinates, true solar time and gender |
| `ZiWei` | Twelve palaces, focus/connected palaces, mobile details, horoscope stepping and copy |
| `Bazi` | Pillars, hidden stems/ten gods, five elements, eight decades, years/months and copy |
| `CalendarPage` | Linked complete page; calendar-only desktop rail and original mobile placement |

```tsx
import { CalendarPage } from '@taichuy/taichuy-calendar';
export default function Block() { return <CalendarPage />; }
```

Each independent export provides its own Context. The complete page shares one
provider. Calendar/almanac selection intentionally does not change birth date;
birth date/time/location/gender/true-solar edits recalculate the natal charts.
Separately mounted host blocks are independent, without global state sharing.

Optional initialization props are declared in `types/index.d.ts`: `initialDate`,
`initialBirthDate`, `initialTime` (HH:mm[:ss]), `initialGender`, `initialLongitude`,
`initialLatitude`, `initialUseTrueSolarTime`, `locale` (zh/en). They initialize
state rather than making the component controlled. The browser-local civil-time
semantics of the source algorithm are preserved, not changed to UTC or a birth-
location timezone. The original English subtitle/quote/disclaimer translations
remain; traditional chart/form terminology remains Chinese, even with locale en.

## Frontend factory ABI

The ESM asset `_assets/ui/browser.js` exports only
`createFrontendModule(hostModules)`. The host first resolves manifest bindings
and passes frozen ESM namespaces keyed by their exact sources. The factory
returns exactly the six components. It has no bare ES imports, global React
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
- To retain source styles and clipboard behavior: `antd-style@4.1.0`,
  `clsx@2.1.1`, `copy-to-clipboard@4.0.2`.
- Locale defaults: `antd/locale/zh_CN`, `dayjs/locale/zh-cn`.

Original scoped CSS-in-JS objects live in the bundle and execute through host
antd-style. There is no missing static stylesheet and no artificial empty
browser.css. React, ReactDOM and every host package must not enter the release
bundle. The build enforces this boundary.

Only the five approved business dependencies are bundled:
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
node scripts/build-fixtures.mjs
TZ=Asia/Shanghai node --test "$TAICHUY_HOST_ROOT/tmp/test-governance/taichuy-calendar/compiled/algorithms.test.cjs"
node scripts/run-component-fixtures.mjs
```

`TAICHUY_EVIDENCE_ROOT` overrides the evidence location. Fixtures require a real
built browser.js and existing host React/Antd/Playwright. They start no server,
install/download nothing, and block network. Algorithms compare to hash-checked
bodies extracted from the **original source**, with a fixed clock and Shanghai
timezone, covering hour/day/leap/solar-term boundaries and source-site snapshots.
The browser fixture executes the actual factory with frozen namespace bindings,
rejects a missing dayjs binding, mounts every export, and exercises desktop/mobile
calendar/form/chart/clipboard paths. It saves text, screenshots and a report.
The QA-only host bundle is not shipped and does not make the plugin privately
bundle React.

This source packet has **not** run a build, Cargo, tests, server, browser or
installer. Root still owns generic factory loading, the additional host sources,
module deduplication and UI-only automatic component registration, followed by
centralized install/disable/uninstall and visual/interaction acceptance. Existing
source screenshots are inputs, not proof of the migrated plugin. Do not call
this source-only packet a cloud-installable release before Root's asset assembly.
