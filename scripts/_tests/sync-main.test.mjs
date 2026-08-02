import test from 'node:test';
import assert from 'node:assert/strict';

import {
  AUTO_REGISTRY_COMMIT_SUBJECT,
  BOT_AUTHOR_EMAIL,
  BOT_AUTHOR_NAME,
  parseGitLogRecords,
  syncMain,
} from '../sync-main.mjs';

function createGitStub(responses) {
  const calls = [];

  return {
    calls,
    git(args) {
      calls.push(args);
      const key = args.join('\0');
      if (!(key in responses)) {
        throw new Error(`Unexpected git call: ${args.join(' ')}`);
      }
      const response = responses[key];
      if (response instanceof Error) {
        throw response;
      }
      return response;
    },
  };
}

test('parseGitLogRecords reads commit metadata from machine format', () => {
  const records = parseGitLogRecords(
    [
      `abc123\x1f${BOT_AUTHOR_NAME}\x1f${BOT_AUTHOR_EMAIL}\x1f${AUTO_REGISTRY_COMMIT_SUBJECT}`,
      `def456\x1fJane Doe\x1fjane@example.com\x1ffeat: change code`,
      '',
    ].join('\x1e')
  );

  assert.deepEqual(records, [
    {
      sha: 'abc123',
      authorName: BOT_AUTHOR_NAME,
      authorEmail: BOT_AUTHOR_EMAIL,
      subject: AUTO_REGISTRY_COMMIT_SUBJECT,
    },
    {
      sha: 'def456',
      authorName: 'Jane Doe',
      authorEmail: 'jane@example.com',
      subject: 'feat: change code',
    },
  ]);
});

test('syncMain rebases over non-overlapping remote commits before pushing', () => {
  const { git, calls } = createGitStub({
    ['branch\0--show-current']: 'main',
    ['rev-parse\0--abbrev-ref\0--symbolic-full-name\0@{u}']: 'origin/main',
    ['status\0--porcelain']: '',
    ['fetch\0origin\0main']: '',
    ['diff\0--name-only\0origin/main...HEAD\0--']:
      'mcp/@taichuy/example/manifest.json',
    [`log\0--format=%H%x1f%an%x1f%ae%x1f%s%x1e\0HEAD..origin/main`]:
      'abc123\x1fgithub-actions[bot]\x1fbot@example.com\x1fchore: publish signed MCP bundle catalog\x1e',
    ['diff-tree\0--no-commit-id\0--name-status\0-r\0abc123\0--']:
      'M\tmcp/catalog.json',
    ['rebase\0origin/main']: '',
    ['push\0origin\0main']: '',
    ['rev-parse\0HEAD']: 'def456',
    ['rev-parse\0origin/main']: 'def456',
  });

  const messages = [];
  syncMain({
    git,
    stdout: { write: (message) => messages.push(message) },
    stderr: { write() {} },
  });

  assert.deepEqual(calls, [
    ['branch', '--show-current'],
    ['rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{u}'],
    ['status', '--porcelain'],
    ['fetch', 'origin', 'main'],
    ['diff', '--name-only', 'origin/main...HEAD', '--'],
    ['log', '--format=%H%x1f%an%x1f%ae%x1f%s%x1e', 'HEAD..origin/main'],
    ['diff-tree', '--no-commit-id', '--name-status', '-r', 'abc123', '--'],
    ['rebase', 'origin/main'],
    ['push', 'origin', 'main'],
    ['fetch', 'origin', 'main'],
    ['rev-parse', 'HEAD'],
    ['rev-parse', 'origin/main'],
  ]);
  assert.match(messages.join(''), /Auto-rebasing over 1 non-overlapping remote commit/);
});

test('syncMain accepts human and automation commits when their files do not overlap local work', () => {
  const { git } = createGitStub({
    ['branch\0--show-current']: 'main',
    ['rev-parse\0--abbrev-ref\0--symbolic-full-name\0@{u}']: 'origin/main',
    ['status\0--porcelain']: '',
    ['fetch\0origin\0main']: '',
    ['diff\0--name-only\0origin/main...HEAD\0--']:
      'mcp/@taichuy/1flowbase_zh_hans/manifest.json',
    [`log\0--format=%H%x1f%an%x1f%ae%x1f%s%x1e\0HEAD..origin/main`]: [
      'human123\x1fMaintainer\x1fmaintainer@example.com\x1fFix MCP extension catalog metadata',
      'bot456\x1fgithub-actions[bot]\x1fbot@example.com\x1fchore: publish signed MCP bundle catalog',
    ].join('\x1e'),
    ['diff-tree\0--no-commit-id\0--name-status\0-r\0human123\0--']:
      'M\tscripts/extension-catalog.mjs',
    ['diff-tree\0--no-commit-id\0--name-status\0-r\0bot456\0--']:
      'M\tmcp/catalog.json',
    ['rebase\0origin/main']: '',
    ['push\0origin\0main']: '',
    ['rev-parse\0HEAD']: 'synced789',
    ['rev-parse\0origin/main']: 'synced789',
  });

  assert.doesNotThrow(() => syncMain({ git, stdout: { write() {} } }));
});

test('syncMain refuses remote commits that overlap locally changed files', () => {
  const { git } = createGitStub({
    ['branch\0--show-current']: 'main',
    ['rev-parse\0--abbrev-ref\0--symbolic-full-name\0@{u}']: 'origin/main',
    ['status\0--porcelain']: '',
    ['fetch\0origin\0main']: '',
    ['diff\0--name-only\0origin/main...HEAD\0--']: 'README.md',
    [`log\0--format=%H%x1f%an%x1f%ae%x1f%s%x1e\0HEAD..origin/main`]:
      'abc123\x1fJane Doe\x1fjane@example.com\x1ffeat: update documentation\x1e',
    ['diff-tree\0--no-commit-id\0--name-status\0-r\0abc123\0--']:
      'M\tREADME.md',
  });

  assert.throws(
    () =>
      syncMain({
        git,
        stdout: { write() {} },
        stderr: { write() {} },
      }),
    /overlap locally changed files/
  );
});
