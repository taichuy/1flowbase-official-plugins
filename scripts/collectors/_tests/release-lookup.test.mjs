import test from 'node:test';
import assert from 'node:assert/strict';
import { lookupCollectorRelease } from '../release-lookup.mjs';
const repository = 'taichuy/1flowbase-official-plugins';
const tag = 'codex-logs-collector-v0.1.0';
const failView = () => { throw Error('release view failed'); };

test('existing release reads only target metadata and does not probe or enumerate history', () => {
  const expected = { targetCommitish: 'source', assets: [{ name: 'distribution.tar.gz' }] };
  assert.deepEqual(lookupCollectorRelease(repository, tag, {
    exec(command, args) { assert.equal(command, 'gh'); assert.equal(args[0], 'release'); assert.ok(args.includes(tag)); assert.ok(args.includes('--jq')); return JSON.stringify(expected); },
    spawn() { assert.fail('existing release must not probe'); },
  }), expected);
});

test('explicit tag404 alone permits initial publication without loading repository releases', () => {
  assert.equal(lookupCollectorRelease(repository, tag, {
    exec: failView,
    spawn(command, args) {
      assert.equal(command, 'gh');
      assert.deepEqual(args, ['api', `repos/${repository}/releases/tags/${tag}`, '--include', '--silent']);
      return { status: 1, stdout: 'HTTP/2.0 404 Not Found\nContent-Type: application/json\n', stderr: 'gh: Not Found' };
    },
  }), undefined);
});

for (const status of [401, 403, 429, 500, 200]) test(`tag${status} never permits creating or overwriting an unreadable release`, () => {
  assert.throws(() => lookupCollectorRelease(repository, tag, { exec: failView,
    spawn: () => ({ status: status === 200 ? 0 : 1, stdout: `HTTP/2.0 ${status} response\n` }),
  }), /Could not read collector release/);
});

test('network/process buffer failure and malformed metadata fail closed', () => {
  for (const probe of [{ status: null, error: Error('ENOBUFS'), stdout: '' }, { status: 1, stdout: '' },
    { status: 1, error: Error('transport interrupted'), stdout: 'HTTP/2.0 404 Not Found\n' }]) {
    assert.throws(() => lookupCollectorRelease(repository, tag, { exec: () => 'invalid JSON', spawn: () => probe }), /Could not read collector release/);
  }
});

test('parseable but incomplete release metadata never means absent', () => {
  for (const value of [null, [], {}, { targetCommitish: 'source', assets: [null] }]) {
    assert.throws(() => lookupCollectorRelease(repository, tag, { exec: () => JSON.stringify(value),
      spawn: () => ({ status: 0, stdout: 'HTTP/2.0 200 OK\n' }),
    }), /Could not read collector release/);
  }
});
