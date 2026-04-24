import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const REMOTE_NAME = 'origin';
export const BRANCH_NAME = 'main';
export const UPSTREAM_REF = `${REMOTE_NAME}/${BRANCH_NAME}`;
export const REGISTRY_FILE = 'official-registry.json';
export const BOT_AUTHOR_NAME = 'github-actions[bot]';
export const BOT_AUTHOR_EMAIL = '41898282+github-actions[bot]@users.noreply.github.com';
export const AUTO_REGISTRY_COMMIT_SUBJECT =
  'chore: update official plugin registry for version changes';

const LOG_FORMAT = '%H%x1f%an%x1f%ae%x1f%s%x1e';

export function runGit(args) {
  return execFileSync('git', args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

export function parseGitLogRecords(output) {
  return output
    .split('\x1e')
    .map((record) => record.trim())
    .filter(Boolean)
    .map((record) => {
      const [sha, authorName, authorEmail, subject] = record.split('\x1f');
      return { sha, authorName, authorEmail, subject };
    });
}

export function parseNameStatus(output) {
  return output
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [status, ...paths] = line.split('\t');
      return { status, path: paths.at(-1) ?? '' };
    });
}

export function isAllowedRegistryAutomationCommit(commit, changedFiles) {
  return (
    commit.authorName === BOT_AUTHOR_NAME &&
    commit.authorEmail === BOT_AUTHOR_EMAIL &&
    commit.subject === AUTO_REGISTRY_COMMIT_SUBJECT &&
    changedFiles.length === 1 &&
    changedFiles[0].status === 'M' &&
    changedFiles[0].path === REGISTRY_FILE
  );
}

function pluralize(count, singular, plural = `${singular}s`) {
  return count === 1 ? singular : plural;
}

function ensureExpectedBranch(git) {
  const branch = git(['branch', '--show-current']);
  if (branch !== BRANCH_NAME) {
    throw new Error(`Expected current branch to be ${BRANCH_NAME}, got ${branch || '(detached HEAD)'}.`);
  }

  const upstream = git(['rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{u}']);
  if (upstream !== UPSTREAM_REF) {
    throw new Error(`Expected ${BRANCH_NAME} to track ${UPSTREAM_REF}, got ${upstream}.`);
  }
}

function ensureCleanWorkingTree(git) {
  const status = git(['status', '--porcelain']);
  if (status) {
    throw new Error('Working tree is not clean. Commit or stash local changes before syncing.');
  }
}

function listRemoteAheadCommits(git) {
  return parseGitLogRecords(git(['log', `--format=${LOG_FORMAT}`, `HEAD..${UPSTREAM_REF}`]));
}

function describeCommit(commit, changedFiles) {
  const paths = changedFiles.map((file) => `${file.status} ${file.path}`).join(', ');
  return `${commit.sha.slice(0, 12)} ${commit.subject} (${commit.authorName}; ${paths || 'no files'})`;
}

function assertRemoteAheadCommitsAreSafe(git, commits) {
  const unsafeDescriptions = [];

  for (const commit of commits) {
    const changedFiles = parseNameStatus(
      git(['diff-tree', '--no-commit-id', '--name-status', '-r', commit.sha, '--'])
    );

    if (!isAllowedRegistryAutomationCommit(commit, changedFiles)) {
      unsafeDescriptions.push(describeCommit(commit, changedFiles));
    }
  }

  if (unsafeDescriptions.length > 0) {
    throw new Error(
      [
        `Refusing to auto-rebase because ${UPSTREAM_REF} contains non-registry automation commits:`,
        ...unsafeDescriptions.map((description) => `- ${description}`),
      ].join('\n')
    );
  }
}

function verifyPushedHead(git) {
  git(['fetch', REMOTE_NAME, BRANCH_NAME]);

  const localHead = git(['rev-parse', 'HEAD']);
  const remoteHead = git(['rev-parse', UPSTREAM_REF]);
  if (localHead !== remoteHead) {
    throw new Error(`Push verification failed: HEAD is ${localHead}, but ${UPSTREAM_REF} is ${remoteHead}.`);
  }
}

export function syncMain({ git = runGit, stdout = process.stdout } = {}) {
  ensureExpectedBranch(git);
  ensureCleanWorkingTree(git);

  stdout.write(`Fetching ${UPSTREAM_REF}...\n`);
  git(['fetch', REMOTE_NAME, BRANCH_NAME]);

  const remoteAheadCommits = listRemoteAheadCommits(git);
  assertRemoteAheadCommitsAreSafe(git, remoteAheadCommits);

  if (remoteAheadCommits.length > 0) {
    stdout.write(
      `Auto-rebasing over ${remoteAheadCommits.length} registry automation ${pluralize(
        remoteAheadCommits.length,
        'commit'
      )}...\n`
    );
    git(['rebase', UPSTREAM_REF]);
  }

  stdout.write(`Pushing ${BRANCH_NAME} to ${UPSTREAM_REF}...\n`);
  git(['push', REMOTE_NAME, BRANCH_NAME]);
  verifyPushedHead(git);
  stdout.write(`${BRANCH_NAME} is synced with ${UPSTREAM_REF}.\n`);
}

function formatError(error) {
  const stderr = error?.stderr?.toString?.().trim();
  if (stderr) {
    return stderr;
  }
  return error?.message || String(error);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    syncMain();
  } catch (error) {
    process.stderr.write(`sync-main: ${formatError(error)}\n`);
    process.exitCode = 1;
  }
}
