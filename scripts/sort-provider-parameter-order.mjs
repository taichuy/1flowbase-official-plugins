import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(import.meta.dirname, '..');
const DEFAULT_STEP = 10;

function resolveProviderCode(pluginId) {
  const raw = String(pluginId || '').trim();
  if (!raw) {
    throw new Error('plugin_id 不能为空');
  }

  return raw.includes('@') ? raw.slice(0, raw.indexOf('@')) : raw;
}

function isTopLevelLine(line) {
  return line.length > 0 && !line.startsWith(' ');
}

function findParameterFieldsRange(lines) {
  const parameterFormIndex = lines.findIndex((line) => /^parameter_form:\s*$/.test(line));
  if (parameterFormIndex < 0) {
    return null;
  }

  const fieldsIndex = lines.findIndex(
    (line, index) => index > parameterFormIndex && /^  fields:\s*$/.test(line)
  );
  if (fieldsIndex < 0) {
    return null;
  }

  let endIndex = lines.length;
  for (let index = fieldsIndex + 1; index < lines.length; index += 1) {
    if (isTopLevelLine(lines[index])) {
      endIndex = index;
      break;
    }
  }

  return {
    startIndex: fieldsIndex + 1,
    endIndex,
  };
}

function rewriteFieldBlock(block, nextOrder) {
  const orderLineIndex = block.findIndex((line) => /^    order:\s*\d+\s*$/.test(line));
  const nextOrderLine = `    order: ${nextOrder}`;

  if (orderLineIndex >= 0) {
    const nextBlock = [...block];
    nextBlock[orderLineIndex] = nextOrderLine;
    return nextBlock;
  }

  const insertIndex = Math.min(1, block.length);
  return [
    ...block.slice(0, insertIndex),
    nextOrderLine,
    ...block.slice(insertIndex),
  ];
}

export function sortProviderParameterOrderContent(content, options = {}) {
  const step = Number(options.step || DEFAULT_STEP);
  if (!Number.isInteger(step) || step <= 0) {
    throw new Error('step 必须是正整数');
  }

  const hasTrailingNewline = content.endsWith('\n');
  const lines = content.split(/\r?\n/);
  if (hasTrailingNewline) {
    lines.pop();
  }

  const range = findParameterFieldsRange(lines);
  if (!range) {
    return {
      changed: false,
      fieldCount: 0,
      content,
    };
  }

  const nextLines = [...lines.slice(0, range.startIndex)];
  let cursor = range.startIndex;
  let fieldIndex = 0;

  while (cursor < range.endIndex) {
    const line = lines[cursor];
    if (!/^  - key:\s*.+$/.test(line)) {
      nextLines.push(line);
      cursor += 1;
      continue;
    }

    const blockStart = cursor;
    cursor += 1;
    while (cursor < range.endIndex && !/^  - key:\s*.+$/.test(lines[cursor])) {
      cursor += 1;
    }

    fieldIndex += 1;
    const block = lines.slice(blockStart, cursor);
    nextLines.push(...rewriteFieldBlock(block, fieldIndex * step));
  }

  nextLines.push(...lines.slice(range.endIndex));
  const nextContent = `${nextLines.join('\n')}${hasTrailingNewline ? '\n' : ''}`;

  return {
    changed: nextContent !== content,
    fieldCount: fieldIndex,
    content: nextContent,
  };
}

export function sortProviderParameterOrderFile(providerPath, options = {}) {
  const content = fs.readFileSync(providerPath, 'utf8');
  const result = sortProviderParameterOrderContent(content, options);

  if (result.changed) {
    fs.writeFileSync(providerPath, result.content, 'utf8');
  }

  return {
    ...result,
    providerPath,
  };
}

export function sortProviderParameterOrderForPlugin(pluginId, options = {}) {
  const providerCode = resolveProviderCode(pluginId);
  const rootDir = path.resolve(options.rootDir || repoRoot);
  const providerPath = path.join(
    rootDir,
    'runtime-extensions',
    'model-providers',
    providerCode,
    'provider',
    `${providerCode}.yaml`
  );

  if (!fs.existsSync(providerPath)) {
    throw new Error(`provider yaml 不存在：${path.relative(rootDir, providerPath)}`);
  }

  return {
    ...sortProviderParameterOrderFile(providerPath, options),
    providerCode,
  };
}

function parseCliArgs(argv) {
  const options = {
    rootDir: repoRoot,
    step: DEFAULT_STEP,
    pluginId: null,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];

    if (arg === '--root-dir') {
      if (!next) {
        throw new Error('--root-dir 需要值');
      }
      options.rootDir = path.resolve(repoRoot, next);
      index += 1;
      continue;
    }

    if (arg === '--step') {
      if (!next) {
        throw new Error('--step 需要值');
      }
      options.step = Number(next);
      index += 1;
      continue;
    }

    if (arg.startsWith('--')) {
      throw new Error(`未知参数：${arg}`);
    }

    if (options.pluginId) {
      throw new Error(`未知参数：${arg}`);
    }
    options.pluginId = arg;
  }

  if (!options.pluginId) {
    throw new Error('Usage: node scripts/sort-provider-parameter-order.mjs <plugin_id> [--root-dir <dir>] [--step <n>]');
  }

  return options;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const options = parseCliArgs(process.argv.slice(2));
  const result = sortProviderParameterOrderForPlugin(options.pluginId, options);
  const relativePath = path.relative(options.rootDir, result.providerPath);
  process.stdout.write(
    `${result.changed ? 'Sorted' : 'Already sorted'} ${relativePath}; ${result.fieldCount} field(s).\n`
  );
}
