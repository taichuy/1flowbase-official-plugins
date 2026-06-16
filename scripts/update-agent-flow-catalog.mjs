import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PAGE_SIZE = 100;
const CATALOG_VERSION = 1;
const TEMPLATE_FILE_NAME = 'template.json';
const DEFAULT_RAW_BASE_URL =
  process.env.AGENT_FLOW_CATALOG_RAW_BASE_URL ||
  'https://raw.githubusercontent.com/taichuy/1flowbase-official-plugins/main';

function toPosixPath(filePath) {
  return filePath.split(path.sep).join('/');
}

function repoRelativePath(repoRoot, filePath) {
  return toPosixPath(path.relative(repoRoot, filePath));
}

function readJsonIfExists(filePath) {
  if (!fs.existsSync(filePath)) {
    return null;
  }

  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function stringifyJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function sha256Value(input) {
  return `sha256:${crypto.createHash('sha256').update(input).digest('hex')}`;
}

function rawUrl(rawBaseUrl, relativePath) {
  return `${rawBaseUrl.replace(/\/+$/, '')}/${relativePath}`;
}

function workflowStateOrder([leftId, left], [rightId, right]) {
  const leftPage = Number.isInteger(left?.page) ? left.page : Number.MAX_SAFE_INTEGER;
  const rightPage = Number.isInteger(right?.page) ? right.page : Number.MAX_SAFE_INTEGER;
  if (leftPage !== rightPage) {
    return leftPage - rightPage;
  }

  const leftPosition = Number.isInteger(left?.position)
    ? left.position
    : Number.MAX_SAFE_INTEGER;
  const rightPosition = Number.isInteger(right?.position)
    ? right.position
    : Number.MAX_SAFE_INTEGER;
  if (leftPosition !== rightPosition) {
    return leftPosition - rightPosition;
  }

  return leftId.localeCompare(rightId);
}

function getPaths(repoRoot) {
  const agentFlowRoot = path.join(repoRoot, 'agent-flow');
  const catalogRoot = path.join(agentFlowRoot, 'catalog', 'v1');
  return {
    agentFlowRoot,
    workflowsRoot: path.join(agentFlowRoot, 'workflows'),
    catalogRoot,
    pagesRoot: path.join(catalogRoot, 'pages'),
    indexPath: path.join(catalogRoot, 'index.json'),
    maintenanceRoot: path.join(agentFlowRoot, '_maintenance'),
    statePath: path.join(agentFlowRoot, '_maintenance', 'catalog-state.json'),
  };
}

function parseAgentFlowTemplate(template, workflowId) {
  if (template?.schema_version !== '1flowbase.application-template/v1') {
    throw new Error(`Invalid AgentFlow template schema_version for ${workflowId}`);
  }

  if (template?.application?.application_type !== 'agent_flow') {
    throw new Error(`Invalid AgentFlow application_type for ${workflowId}`);
  }

  if (!template.application.name || typeof template.application.name !== 'string') {
    throw new Error(`Missing AgentFlow application.name for ${workflowId}`);
  }

  return {
    schema_version: template.schema_version,
    application: template.application,
  };
}

export function discoverAgentFlowTemplates(repoRoot) {
  const { workflowsRoot } = getPaths(repoRoot);
  if (!fs.existsSync(workflowsRoot)) {
    return [];
  }

  return fs
    .readdirSync(workflowsRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => ({
      workflowId: entry.name,
      templatePath: path.join(workflowsRoot, entry.name, TEMPLATE_FILE_NAME),
    }))
    .filter((entry) => fs.existsSync(entry.templatePath))
    .sort((left, right) => left.workflowId.localeCompare(right.workflowId));
}

export function buildAgentFlowCatalog({
  repoRoot,
  now = new Date(),
  pageSize = PAGE_SIZE,
  rawBaseUrl = DEFAULT_RAW_BASE_URL,
} = {}) {
  if (!repoRoot) {
    throw new Error('repoRoot is required');
  }

  const paths = getPaths(repoRoot);
  const nowIso = typeof now === 'string' ? now : now.toISOString();
  const previousState = readJsonIfExists(paths.statePath) || {
    version: CATALOG_VERSION,
    workflows: {},
  };
  const previousIndex = readJsonIfExists(paths.indexPath);
  const previousWorkflows =
    typeof previousState.workflows === 'object' && previousState.workflows !== null
      ? previousState.workflows
      : {};

  const workflows = new Map();
  for (const discovered of discoverAgentFlowTemplates(repoRoot)) {
    const templateBytes = fs.readFileSync(discovered.templatePath);
    const template = JSON.parse(templateBytes.toString('utf8'));
    const parsed = parseAgentFlowTemplate(template, discovered.workflowId);
    const templateRelativePath = repoRelativePath(repoRoot, discovered.templatePath);

    workflows.set(discovered.workflowId, {
      workflowId: discovered.workflowId,
      templateRelativePath,
      templateHash: sha256Value(templateBytes),
      ...parsed,
    });
  }

  const previousOrderedIds = Object.entries(previousWorkflows)
    .filter(([workflowId]) => workflows.has(workflowId))
    .sort(workflowStateOrder)
    .map(([workflowId]) => workflowId);
  const newWorkflowIds = [...workflows.keys()]
    .filter((workflowId) => !previousWorkflows[workflowId])
    .sort((left, right) => left.localeCompare(right));
  const orderedWorkflowIds = [...previousOrderedIds, ...newWorkflowIds];

  const previousWorkflowIds = Object.keys(previousWorkflows).sort();
  const currentWorkflowIds = [...workflows.keys()].sort();
  let semanticChanges =
    !previousIndex ||
    previousWorkflowIds.length !== currentWorkflowIds.length ||
    previousWorkflowIds.some((workflowId, index) => workflowId !== currentWorkflowIds[index]);

  const entries = [];
  const nextStateWorkflows = {};

  orderedWorkflowIds.forEach((workflowId, index) => {
    const workflow = workflows.get(workflowId);
    const page = Math.floor(index / pageSize) + 1;
    const position = (index % pageSize) + 1;
    const previous = previousWorkflows[workflowId];
    const hashChanged = previous?.template_sha256 !== workflow.templateHash;
    const positionChanged = previous?.page !== page || previous?.position !== position;
    semanticChanges ||= hashChanged || positionChanged;
    const updatedAt =
      !hashChanged && typeof previous?.updated_at === 'string'
        ? previous.updated_at
        : nowIso;

    nextStateWorkflows[workflowId] = {
      page,
      position,
      template_sha256: workflow.templateHash,
      updated_at: updatedAt,
    };
    entries.push({
      workflow_id: workflow.workflowId,
      schema_version: workflow.schema_version,
      application: workflow.application,
      template_url: rawUrl(rawBaseUrl, workflow.templateRelativePath),
      template_sha256: workflow.templateHash,
      updated_at: updatedAt,
    });
  });

  const generatedAt =
    semanticChanges || typeof previousIndex?.generated_at !== 'string'
      ? nowIso
      : previousIndex.generated_at;
  const stateUpdatedAt =
    semanticChanges || typeof previousState?.updated_at !== 'string'
      ? nowIso
      : previousState.updated_at;

  const pageDocuments = [];
  for (let offset = 0; offset < entries.length; offset += pageSize) {
    const pageNumber = Math.floor(offset / pageSize) + 1;
    const pageEntries = entries.slice(offset, offset + pageSize);
    const pageRelativePath = `agent-flow/catalog/v1/pages/${pageNumber}.json`;
    const nextPageRelativePath =
      offset + pageSize < entries.length
        ? `agent-flow/catalog/v1/pages/${pageNumber + 1}.json`
        : null;
    const document = {
      version: CATALOG_VERSION,
      page: pageNumber,
      page_size: pageSize,
      next_page_url: nextPageRelativePath ? rawUrl(rawBaseUrl, nextPageRelativePath) : null,
      entries: pageEntries,
    };
    const json = stringifyJson(document);

    pageDocuments.push({
      page: pageNumber,
      relativePath: pageRelativePath,
      filePath: path.join(repoRoot, pageRelativePath),
      entryCount: pageEntries.length,
      sha256: sha256Value(json),
      document,
      json,
    });
  }

  const indexDocument = {
    version: CATALOG_VERSION,
    generated_at: generatedAt,
    page_size: pageSize,
    total_entries: entries.length,
    first_page_url:
      pageDocuments.length > 0 ? rawUrl(rawBaseUrl, pageDocuments[0].relativePath) : null,
    pages: pageDocuments.map((page) => ({
      page: page.page,
      url: rawUrl(rawBaseUrl, page.relativePath),
      entry_count: page.entryCount,
      sha256: page.sha256,
    })),
  };
  const stateDocument = {
    version: CATALOG_VERSION,
    updated_at: stateUpdatedAt,
    page_size: pageSize,
    workflows: nextStateWorkflows,
  };

  return {
    paths,
    semanticChanges,
    indexDocument,
    pageDocuments,
    stateDocument,
  };
}

function writeFileIfChanged(filePath, content) {
  if (fs.existsSync(filePath) && fs.readFileSync(filePath, 'utf8') === content) {
    return false;
  }

  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, content);
  return true;
}

function removeStaleGeneratedPages(pagesRoot, expectedFilePaths) {
  if (!fs.existsSync(pagesRoot)) {
    return [];
  }

  const expected = new Set(expectedFilePaths.map((filePath) => path.resolve(filePath)));
  const removed = [];
  for (const entry of fs.readdirSync(pagesRoot, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith('.json')) {
      continue;
    }

    const filePath = path.join(pagesRoot, entry.name);
    if (!expected.has(path.resolve(filePath))) {
      fs.unlinkSync(filePath);
      removed.push(filePath);
    }
  }
  return removed;
}

export function updateAgentFlowCatalog(options = {}) {
  const repoRoot = options.repoRoot || path.resolve(import.meta.dirname, '..');
  const catalog = buildAgentFlowCatalog({ ...options, repoRoot });
  fs.mkdirSync(catalog.paths.pagesRoot, { recursive: true });
  fs.mkdirSync(catalog.paths.maintenanceRoot, { recursive: true });

  const changedFiles = [];
  if (
    writeFileIfChanged(catalog.paths.indexPath, stringifyJson(catalog.indexDocument))
  ) {
    changedFiles.push(catalog.paths.indexPath);
  }

  for (const page of catalog.pageDocuments) {
    if (writeFileIfChanged(page.filePath, page.json)) {
      changedFiles.push(page.filePath);
    }
  }

  const removedPages = removeStaleGeneratedPages(
    catalog.paths.pagesRoot,
    catalog.pageDocuments.map((page) => page.filePath)
  );
  changedFiles.push(...removedPages);

  if (
    writeFileIfChanged(catalog.paths.statePath, stringifyJson(catalog.stateDocument))
  ) {
    changedFiles.push(catalog.paths.statePath);
  }

  return {
    changed: changedFiles.length > 0,
    changedFiles,
    totalEntries: catalog.indexDocument.total_entries,
    pageCount: catalog.pageDocuments.length,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const result = updateAgentFlowCatalog();
  if (result.changed) {
    console.log(
      `Updated AgentFlow catalog: ${result.totalEntries} workflows across ${result.pageCount} pages.`
    );
    for (const filePath of result.changedFiles) {
      console.log(`- ${repoRelativePath(path.resolve(import.meta.dirname, '..'), filePath)}`);
    }
  } else {
    console.log(
      `AgentFlow catalog is already current: ${result.totalEntries} workflows across ${result.pageCount} pages.`
    );
  }
}
