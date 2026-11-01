# Approved-source migration / 批准源码迁移

Source / 来源：`/home/taichuy/git/taichu-y-web-master`。The user supplied and approved
this private site's source for this independent extension. No third-party/public
license for the private site is inferred. The enclosing plugin repository's
license applies to the contributed migration; all bundled dependency license
texts are collected by `scripts/licenses.mjs` into the release assets.

`inventory.json` records every migrated source file and the SHA-256 of its
approved bytes. `source-expectations.json` records the existing source-site
observation in the host's `tmp/test-governance/taichuy-calendar/` folder. It is a
fixture input, **not** a claim that the new plugin has passed acceptance.

## Inventory and changes / 库存与适配

- `src/app/bazi/page.tsx` → `src/page/{CalendarPage,BirthInfo,styles}`,
  `src/state/PluginContext.tsx`, `src/algorithms/true-solar-time.ts`.
  The original complete form, lunar date cells, astronomical correction,
  independent calendar-vs-birth date state, ZiWei/Bazi tabs and live recalculation
  remain. The original button now remounts/recalculates the same charts instead
  of doing nothing. There is no prediction endpoint.
- `TaichuyCalendar.tsx` and `TaichuyCalendarCard.tsx` → `src/components/`.
  Month/year, header selections, today and session-memory sign actions remain.
  Font Awesome's old site-global check icon becomes the host's CheckOutlined;
  calendar panel initialization respects an explicit initial date. The old
  Next `/bazi` link becomes an in-page almanac action on the complete page and
  is hidden for standalone Calendar. No navigation to the old site is retained.
- **All seven** `components/taichuy/divination/**` source files are present:
  `BaziCalculator`, `ZiWeiCalculator`, `ZiWeiMobile`, `LunarInfoCard`, both prompt
  generators and `types`. Natal/flowing-star calculations, all 12 palaces,
  focus/triads/opposite-palace lines, mobile detail cards, horoscope stepping,
  decade/year/month Bazi tables and all clipboard prompt paths remain.
- Bazi's calculation effect is extracted into `src/algorithms/bazi.ts` (clock
  parameter defaults to the same current time). ZiWei's hour-index/astro call is
  extracted into `src/algorithms/ziwei.ts`; ZiWei's style object and unchanged
  palace-position table are in `ziwei-styles.ts` and `palace-layout.ts`. These
  extractions were already present in the untracked packet draft; they are not
  replacements with approximate algorithms. Reference fixtures derive the
  original bodies from hash-checked source, independently of these extractions.
- `LocationSelector.tsx`, `DateContext.tsx`, `utils/clipboard.ts` are migrated.
  The original 3,428 coordinate records are retained in eight prefix shards;
  record values are unchanged. `china-division/dist/pca-code.json` is bundled
  from approved dependency 2.7.0. There are no geocoding/network calls.
- Five `useT` keys from original `local/{zh,en}.json` are retained in plugin-local
  dictionaries and a React Context translator, without Next/server/site imports.
  The original traditional calendar/chart terminology remains Chinese, including
  when `locale="en"`; the quote/disclaimer/subtitle text has the source's English
  translations. README files are bilingual; this is not a claim of a fully
  English-localized original application.
- The standalone complete page owns only the old desktop rail's calendar, with
  the original mobile calendar placement. Hot lists, header/sidebar/login/search,
  Next router, system configuration and other site-only dependencies are omitted
  intentionally. Original scoped CSS-in-JS objects and Antd markup remain;
  `classNames.popup.root` is the current Antd 6 API with the same popup styles.
- `data-*`/ARIA attributes identify fixtures and improve access to controls; they
  do not replace real controls with test-only data. No fixture-only model is
  returned from the six public exports.

## Evidence and unverified work / 证据与未验证项

This D1 packet does **not** run a build, Cargo, tests, server, browser or installer.
The existing source screenshots/text are input evidence, not new-plugin evidence.
Root owns one centralized build/QA/assembly pass. Until that pass, the packet is
source-complete but **not a cloud-installable release**: there is deliberately no
fake `_assets/ui/browser.js`, no fake asset digest and no placeholder manifest.
The build emits the actual manifest and asset digest together; Root must commit
them with `THIRD_PARTY_LICENSES.txt` and `migration/bundle-contract.json`.

Root must additionally complete generic host support for factory-backed fetched
modules, the additional declared host sources, native_react manifests with complete code_template,
module deduplication and UI-only contribution registration. The current v2 schema
requires one explicit declarative_only execution binding per frontend payload;
all six point to the packaging guard, not a business service. Minimum host version
is 0.6.1 **with the new contract code**; older published 0.6.1 builds are insufficient. The plugin must not compensate with global React, iframe loading,
hardcoded management records, backend prediction/data APIs, or permission grants.

### Native prefix adaptation (0.1.2)

The 21 Ant Design descendant selectors in page/styles.ts, ziwei-styles.ts and
BaziCalculator.tsx interpolate the existing createStyles `prefixCls`. Selector
meaning and rule declarations are unchanged; both default `ant` and host-owned
native prefixes work. No global CSS, provider prefix override or algorithm change.

- 0.1.3 preserves the host-provided date-cell class from `cellRender.originNode`; no hardcoded `ant-picker-cell-inner` remains in the custom lunar date cell. Algorithm inputs and behavior are unchanged.
