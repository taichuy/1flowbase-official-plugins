import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { root } from './toolchain.mjs';
import { splitPackage, buildArchive } from '../../../../scripts/application-template/archive.mjs';
const page = 'd36ae101-7c46-485e-a4da-f5928a114c01';
const tab = 'd36ae101-7c46-485e-a4da-f5928a114c02';
const block = 'd36ae101-7c46-485e-a4da-f5928a114c03';
// Source installation identity is remapped by the host to the target authorized catalog.
const installation = 'd36ae101-7c46-485e-a4da-f5928a114c04';
const source = await readFile(resolve(root, 'examples/composed-page.tsx'), 'utf8');
const packageValue = {
  schema_version: '1flowbase.portable-template/v1',
  release: { template_id: '@taichuy/calendar-composition', release_version: 7,
    name: '太初y 日历排盘 · 可编辑组合 / Editable Calendar',
    description: 'Linked calendar, almanac, birth form and charts composed in editable JSX; requires taichuy-calendar 0.1.15.',
    exported_from_system_version: '0.6.2', exported_at: '2026-11-01T19:53:34Z' },
  pages: [{ id: page, parent_id: null, kind: 'page', title: '日历排盘', icon: null, tooltip: null,
    is_hidden: false, placement: 'topbar', content_presentation: 'single', slug: 'calendar-composition', rank: 'a', visibility_rules: [],
    tabs: [{ id: tab, title: 'Default', rank: 'a', is_default: true, route_segment: null,
      document_root_uid: `frontstage.tab.${tab}.root`,
      document_payload: { version: 1, root_uid: `frontstage.tab.${tab}.root` },
      blocks: [{ block_id: block, parent_block_id: null, rank: 'a', presentation: 'page',
        title: '日历、黄历与排盘', description: '可编辑的多组件组合',
        code_ref: `frontstage.block.${block}`, schema_version: 1, input_mapping: {}, output_mapping: {},
        source_code: source, runtime_descriptor: {
          id: block, codeRef: `frontstage.block.${block}`, rendererVersion: 'v1', renderer_version: 'v1',
          ports: { inputs: [], outputs: [] }, props: {},
          catalog: { providerCode: 'taichuy-calendar', installationId: installation },
          contribution: { code: 'Calendar', pluginId: 'taichuy-calendar@0.1.15', pluginVersion: '0.1.15' },
          runtime: { kind: 'native_react', hint: 'native_react', entry: 'default', code_template_version: '0.1.15', code_template_language: 'tsx' },
          'x-layout': { order: 0 }, 'x-presentation': { height: null, heightMode: 'auto' },
        } }],
    }],
  }],
  applications: [], data_models: [], mcp_bundle: null,
  plugins: [{ plugin_id: 'taichuy-calendar@0.1.15', plugin_version: '0.1.15', checksum: null, contribution_code: 'Calendar' }],
};
const directory = resolve(root, '../../../applications-demo/@taichuy/calendar-composition');
await mkdir(directory, { recursive: true });
await splitPackage(packageValue, directory);
const archive = await buildArchive(directory);
await mkdir(resolve(root, '_assets/templates'), { recursive: true });
await writeFile(resolve(root, '_assets/templates/calendar-composition.zip'), archive);
console.log(JSON.stringify({ directory, archiveBytes: archive.length, pages: 1, blocks: 1 }));
