import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, spawnSync, spawn } from 'node:child_process';
import http from 'node:http';
import { listCollectors, PLATFORM_TARGETS, archiveName } from '../catalog.mjs';
import { buildDistribution, verifyDistributionSignature } from '../distribution.mjs';
import { discoverCatalogEntries } from '../../extension-catalog.mjs';

const repoRoot = path.resolve(import.meta.dirname, '../../..');
const collector = listCollectors(repoRoot)[0];
function fixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'collector-distribution-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const nativeDirectory = path.join(directory, 'native'); fs.mkdirSync(nativeDirectory);
  // Deliberately opaque bytes: the outer packager must not interpret native archives.
  const originals = new Map(PLATFORM_TARGETS.map(platform => {
    const name = archiveName(collector, platform); const bytes = Buffer.from(`opaque:${platform.rust_target}`);
    fs.writeFileSync(path.join(nativeDirectory, name), bytes); return [name, bytes];
  }));
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
  const options = { repoRoot, collector, nativeDirectory, outputDirectory: path.join(directory, 'output'),
    sourceSha: 'a'.repeat(40), privateKey, keyId: 'fixture-ed25519' };
  return { directory, originals, options, publicKey };
}

test('DIST-006 one signed outer archive contains exact flat members and all six untouched platforms', t => {
  const f = fixture(t); const result = buildDistribution(f.options);
  const bytes = fs.readFileSync(result.archive);
  assert.equal(verifyDistributionSignature(bytes, result.entry, f.publicKey), true);
  const names = execFileSync('tar', ['-tzf', result.archive], { encoding: 'utf8' }).trim().split('\n');
  assert.deepEqual(new Set(names), new Set(['collector-manifest.json', ...result.manifest.assets.map(asset => asset.name)]));
  assert.equal(names.length, new Set(names).size);
  assert.ok(names.every(name => /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name)));
  assert.deepEqual(Object.keys(result.manifest.assets[0]).sort(), ['name', 'sha256', 'size']);
  const extracted = path.join(f.directory, 'extracted'); fs.mkdirSync(extracted);
  execFileSync('tar', ['-xzf', result.archive, '-C', extracted]);
  const manifest = JSON.parse(fs.readFileSync(path.join(extracted, 'collector-manifest.json')));
  for (const asset of manifest.assets) {
    const member = fs.readFileSync(path.join(extracted, asset.name));
    assert.equal(member.length, asset.size);
    assert.equal(crypto.createHash('sha256').update(member).digest('hex'), asset.sha256);
    if (f.originals.has(asset.name)) assert.deepEqual(member, f.originals.get(asset.name));
  }
  assert.ok(['README.md', 'README.en.md', 'install.sh', 'install.ps1', 'release.json', 'checksums.txt',
    'checksums.txt.sig', 'signing-public-key.pem'].every(name => names.includes(name)));
  assert.ok(crypto.verify(null, fs.readFileSync(path.join(extracted, 'checksums.txt')), f.publicKey,
    fs.readFileSync(path.join(extracted, 'checksums.txt.sig'))));
  const tampered = Buffer.from(bytes); tampered[10] ^= 1;
  assert.throws(() => verifyDistributionSignature(tampered, result.entry, f.publicKey), /mismatch/);
  const { publicKey: wrongKey } = crypto.generateKeyPairSync('ed25519');
  assert.throws(() => verifyDistributionSignature(bytes, result.entry, wrongKey), /mismatch/);
  assert.deepEqual(fs.readFileSync(buildDistribution(f.options).archive), bytes, 'same source builds immutable bytes');
});

test('distribution requires all six regular archives and rejects symlink/hardlink input', t => {
  const f = fixture(t); const first = path.join(f.options.nativeDirectory, [...f.originals.keys()][0]);
  fs.unlinkSync(first); assert.throws(() => buildDistribution(f.options), /ENOENT/);
  const target = path.join(f.directory, 'archive'); fs.writeFileSync(target, 'opaque');
  fs.symlinkSync(target, first); assert.throws(() => buildDistribution(f.options), /regular unlinked/);
  fs.unlinkSync(first); fs.linkSync(target, first); assert.throws(() => buildDistribution(f.options), /regular unlinked/);
});

test('catalog framework publishes typed client entry and rejects contract/manifest mismatches', t => {
  const f = fixture(t); const { entry } = buildDistribution(f.options);
  const root = path.join(f.directory, 'catalog-repo'); const dir = path.join(root, collector.plugin_dir);
  fs.mkdirSync(dir, { recursive: true });
  fs.copyFileSync(path.join(repoRoot, collector.plugin_dir, 'collector-manifest.json'), path.join(dir, 'collector-manifest.json'));
  entry.download_locator.locator = 'https://fixture.example/releases/distribution.tar.gz';
  const save = () => fs.writeFileSync(path.join(dir, 'catalog-entry.json'), JSON.stringify(entry)); save();
  const read = () => discoverCatalogEntries({ repoRoot: root, category: 'runtime-extensions' });
  assert.equal(read()[0].id, 'runtime-extensions:taichuy/codex-logs-collector');
  assert.equal(read()[0].download_locator.kind, 'release_asset');
  assert.equal(read()[0].source.client_collector.execution_target, 'client');
  assert.deepEqual(read()[0].slot_codes, []);
  entry.source.client_collector.source_client = 'different'; save(); assert.throws(read, /does not match manifest/);
  entry.source.client_collector.source_client = 'codex'; entry.source.distribution_kind = 'unknown'; save(); assert.throws(read, /unsupported/);
  entry.source.distribution_kind = 'client_collector'; entry.download_locator.kind = 'platform_release_assets'; save(); assert.throws(read, /one signed release asset/);
  entry.download_locator.kind = 'release_asset'; entry.signature = null; save(); assert.throws(read, /one signed release asset/);
});

test('unpublished newer collector source retains the signed published snapshot until verified publication', t => {
  const f = fixture(t); const result = buildDistribution(f.options);
  const root = path.join(f.directory, 'catalog-repo'); const dir = path.join(root, collector.plugin_dir);
  fs.mkdirSync(dir, { recursive: true });
  const sourceManifest = JSON.parse(fs.readFileSync(path.join(repoRoot, collector.plugin_dir, 'collector-manifest.json'), 'utf8'));
  const manifestPath = path.join(dir, 'collector-manifest.json');
  const entry = structuredClone(result.entry);
  entry.download_locator.locator = 'https://fixture.example/releases/distribution.tar.gz';
  const catalogBytes = JSON.stringify(entry);
  fs.writeFileSync(path.join(dir, 'catalog-entry.json'), catalogBytes);
  const saveManifest = () => fs.writeFileSync(manifestPath, JSON.stringify(sourceManifest));
  const read = () => discoverCatalogEntries({ repoRoot: root, category: 'runtime-extensions' });
  const [major, minor, patch] = collector.version.split('.').map(BigInt);
  sourceManifest.version = `${major}.${minor}.${patch + 1n}`;
  sourceManifest.display_name = 'Next unpublished name';
  sourceManifest.minimum_host_version = '99.0.0';
  saveManifest();
  const [published] = read();
  assert.equal(published.version, entry.version);
  assert.deepEqual(published.signature, entry.signature);
  assert.equal(published.checksum, entry.checksum);
  assert.equal(published.host_version_requirement, entry.host_version_requirement);
  assert.equal(published.source.client_collector.display_name, entry.source.client_collector.display_name);
  assert.equal(fs.readFileSync(path.join(dir, 'catalog-entry.json'), 'utf8'), catalogBytes);
  assert.equal(verifyDistributionSignature(fs.readFileSync(result.archive), entry, f.publicKey), true);
  sourceManifest.version = collector.version;
  saveManifest();
  assert.throws(read, /does not match manifest/, 'same release metadata must still match');
  sourceManifest.version = '0.0.0'; saveManifest();
  assert.throws(read, /does not match manifest/, 'unbuilt catalog ahead of source is rejected');
  sourceManifest.version = 'invalid'; saveManifest();
  assert.throws(read, /three numeric components/);
  sourceManifest.version = `${major + 1n}.0.0`;
  sourceManifest.organization = 'wrong-owner'; saveManifest();
  assert.throws(read, /does not match manifest/, 'source identity remains checked across versions');
});

test('installer requires explicit 1flowbase release base before downloading or configuring', () => {
  const shell = path.join(repoRoot, 'installers/client-collectors/install.sh');
  const result = spawnSync('bash', [shell, '--endpoint', 'https://fixture.example/api/logs/v1/events', '--no-start'], { encoding: 'utf8' });
  assert.equal(result.status, 2); assert.match(result.stderr, /--release-base is required/);
  for (const name of ['install.sh', 'install.ps1']) {
    assert.ok(!fs.readFileSync(path.join(repoRoot, 'installers/client-collectors', name), 'utf8').includes('github.com'));
  }
});


test('installer refuses redirected member downloads without requesting the redirect target', async t => {
  const requests = [];
  const server = http.createServer((request, response) => {
    requests.push(request.url);
    response.writeHead(302, { Location: '/redirect-target' }).end('redirect body');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const base = `http://127.0.0.1:${server.address().port}`;
  const result = await new Promise((resolve, reject) => {
    const child = spawn('bash', [path.join(repoRoot, 'installers/client-collectors/install.sh'),
      '--endpoint', `${base}/api/logs/v1/events`, '--release-base', `${base}/assets`, '--no-start'],
      { stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', bytes => { output += bytes; });
    child.stderr.on('data', bytes => { output += bytes; });
    child.on('error', reject); child.on('close', code => resolve({ code, output }));
  });
  assert.equal(result.code, 1); assert.match(result.output, /redirects are refused/);
  assert.equal(requests.length, 1); assert.ok(requests[0].startsWith('/assets/'));
  assert.ok(!requests.includes('/redirect-target'));
  const powershell = fs.readFileSync(path.join(repoRoot, 'installers/client-collectors/install.ps1'), 'utf8');
  const downloads = powershell.split('\n').filter(line => line.includes('Invoke-WebRequest'));
  assert.equal(downloads.length, 2);
  assert.ok(downloads.every(line => line.includes('-MaximumRedirection 0')));
});
