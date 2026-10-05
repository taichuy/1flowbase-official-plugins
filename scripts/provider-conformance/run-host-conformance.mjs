import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync, spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { listProviderPackageTargets, listProviderReleaseTargets } from '../list-provider-package-targets.mjs';
import { matchesProviderPlatformAsset } from '../provider-release/assets.mjs';

export const HOST_TEST_NAMES = Object.freeze([
  'd_008_eight_official_runtime_extensions_execute_through_the_real_host',
  'drs_008_013_session_retry_executes_through_real_host_and_plugin_data',
]);
const TARGET = 'x86_64-unknown-linux-musl';
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const commandText = (command, args) => JSON.stringify([command, ...args]);
const execute = (command, args, options = {}) => execFileSync(command, args, {
  encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, ...options,
});

function archivePath(name) {
  const normalized = name.replace(/^\.\//u, '').replace(/\/$/u, '');
  if (normalized === '.' || normalized === '') return '';
  if (normalized.includes('\\') || path.posix.isAbsolute(normalized) || path.win32.isAbsolute(normalized)
    || normalized.split('/').some(segment => !segment || segment === '.' || segment === '..')
    || /[\x00-\x1f\x7f]/u.test(normalized)) throw new Error(`unsafe archive path ${name}`);
  return normalized;
}

function unpackArchive(archive, destination) {
  const names = execute('tar', ['-tzf', archive, '--quoting-style=escape']).trimEnd().split('\n');
  const verbose = execute('tar', ['-tvzf', archive, '--quoting-style=escape']).trimEnd().split('\n');
  if (names.length !== verbose.length || !names.length) throw new Error('archive listing mismatch');
  const seen = new Set();
  names.forEach((name, index) => {
    const type = verbose[index][0];
    if (!['-', 'd'].includes(type)) throw new Error(`archive link or unsupported type ${name}`);
    const relative = archivePath(name);
    if (!relative && type !== 'd') throw new Error('unsafe archive root file');
    if (seen.has(relative)) throw new Error(`duplicate archive path ${name}`);
    seen.add(relative);
  });
  fs.mkdirSync(destination, { recursive: true });
  execute('tar', ['-xzf', archive, '--no-same-owner', '-C', destination]);
}

export function stagePackages({ officialRoot, packageDir, stageRoot }) {
  const packageTargets = listProviderPackageTargets(officialRoot);
  const releaseTargets = listProviderReleaseTargets(officialRoot);
  const releaseByDirectory = new Map(releaseTargets.map(row => [row.plugin_dir, row]));
  if (!packageTargets.length || releaseByDirectory.size !== packageTargets.length
    || new Set(packageTargets.map(row => row.plugin_dir)).size !== packageTargets.length) {
    throw new Error('empty or duplicate expected package inventory');
  }
  const archives = fs.readdirSync(packageDir).map(name => {
    const file = path.join(packageDir, name);
    if (!name.endsWith('.1flowbasepkg') || !fs.lstatSync(file).isFile()) throw new Error(`unexpected archive inventory entry ${name}`);
    return { name, file };
  });
  const consumed = new Set();
  const selected = packageTargets.map(target => {
    const release = releaseByDirectory.get(target.plugin_dir);
    if (!release || release.provider_code !== target.provider_code) throw new Error('package/release inventory mismatch');
    const assetBase = `${release.asset_vendor}@${release.asset_plugin_code}@${release.version}@linux-amd64`;
    const matches = archives.filter(archive => matchesProviderPlatformAsset(archive.name, assetBase));
    if (matches.length !== 1) throw new Error(`archive inventory requires exactly one ${assetBase}, got ${matches.length}`);
    if (consumed.has(matches[0].file)) throw new Error('archive inventory reused an archive');
    consumed.add(matches[0].file);
    return { ...release, binary_name: target.binary_name, archive: matches[0] };
  });
  if (consumed.size !== archives.length) throw new Error('archive inventory contains unknown version or platform');
  const rows = [];
  for (const target of selected) {
    // Directory ownership comes from inventory, never from manifest ID or vendor.
    archivePath(target.plugin_dir);
    const destination = path.join(stageRoot, target.plugin_dir);
    const sourceManifest = path.join(officialRoot, target.plugin_dir, 'manifest.yaml');
    unpackArchive(target.archive.file, destination);
    const stagedManifest = path.join(destination, 'manifest.yaml');
    if (!fs.existsSync(stagedManifest) || !fs.readFileSync(stagedManifest).equals(fs.readFileSync(sourceManifest))) {
      throw new Error(`${target.provider_code} archive manifest differs from paired official source`);
    }
    // The package owner requires runtime.entry to be bin/<binary_name>.
    // The paired manifest's exact bytes and package inventory establish this entry.
    const runtimeSection = fs.readFileSync(stagedManifest, 'utf8').match(/^runtime:\s*\n((?:[ \t]+[^\n]*(?:\n|$)|\s*\n)*)/mu)?.[1];
    const runtimeEntry = runtimeSection?.match(/^  entry:\s*([^\s#]+)\s*(?:#.*)?$/mu)?.[1];
    if (runtimeEntry !== `bin/${target.binary_name}` || archivePath(runtimeEntry) !== runtimeEntry) {
      throw new Error(`${target.provider_code} unsupported archive runtime entry`);
    }
    const binary = path.join(destination, runtimeEntry);
    if (!fs.existsSync(binary) || !fs.lstatSync(binary).isFile() || !(fs.statSync(binary).mode & 0o111)) {
      throw new Error(`${target.provider_code} archive binary must be a regular executable file`);
    }
    const fixtureBinary = path.join(destination, 'target/debug', target.binary_name);
    fs.mkdirSync(path.dirname(fixtureBinary), { recursive: true });
    fs.copyFileSync(binary, fixtureBinary);
    fs.chmodSync(fixtureBinary, 0o755);
    const binaryHash = hash(binary);
    const fixtureHash = hash(fixtureBinary);
    if (binaryHash !== fixtureHash) throw new Error('archive/fixture binary hash mismatch');
    rows.push({
      provider_code: target.provider_code, plugin_dir: target.plugin_dir,
      asset_plugin_code: target.asset_plugin_code, asset_vendor: target.asset_vendor,
      version: target.version, archive: target.archive.name, archive_sha256: hash(target.archive.file),
      manifest_sha256: hash(stagedManifest), runtime_entry: runtimeEntry,
      binary_sha256: binaryHash, fixture_binary_sha256: fixtureHash,
    });
  }
  return rows;
}

export function assertHostTestEvidence(output, status) {
  const plain = output.replace(/\x1b\[[0-?]*[ -/]*[@-~]/gu, '');
  const summaries = [...plain.matchAll(/test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;/gu)];
  if (status !== 0 || summaries.length !== 1 || summaries[0][1] !== 'ok'
    || summaries[0][2] !== '2' || summaries[0].slice(3, 7).some(count => count !== '0')) {
    throw new Error('Host test evidence requires exactly two passed, zero failed/ignored/filtered cases');
  }
  const passedNames = [...plain.matchAll(/^test (\S+) \.\.\. ok\s*$/gmu)].map(match => match[1]);
  if (passedNames.length !== HOST_TEST_NAMES.length || new Set(passedNames).size !== HOST_TEST_NAMES.length
    || HOST_TEST_NAMES.some(name => !passedNames.includes(name))) throw new Error('Host test evidence omitted or replaced a required test');
  return [...HOST_TEST_NAMES];
}

async function runCargo(command, args, { env, cwd, log }) {
  const fd = fs.openSync(log, 'a');
  try {
    return await new Promise((resolve, reject) => {
      const child = spawn(command, args, { cwd, env, stdio: ['ignore', fd, fd] });
      child.once('error', reject);
      child.once('exit', (code, signal) => resolve(signal ? null : code));
    });
  } finally { fs.closeSync(fd); }
}

export async function runHostConformance(options, dependencies = {}) {
  const stageRoot = options.stageRoot ?? path.join(os.tmpdir(), `official-host-conformance-${crypto.randomUUID()}`);
  const receipt = {
    schema_version: '1flowbase.official-package-host-conformance/v1', verdict: 'FAIL',
    target: options.target, expected_test_names: [...HOST_TEST_NAMES], packages: [],
    requested_main_sha: options.mainSha, requested_official_sha: options.officialSha,
    log: path.resolve(options.log),
  };
  fs.mkdirSync(path.dirname(options.artifact), { recursive: true });
  fs.mkdirSync(path.dirname(options.log), { recursive: true });
  fs.writeFileSync(options.log, '');
  let ownsStage = false;
  try {
    if (options.target !== TARGET) throw new Error(`actual package gate requires upstream target ${TARGET}`);
    if (![options.mainSha, options.officialSha].every(sha => /^[a-f0-9]{40}$/u.test(sha ?? ''))) throw new Error('full paired SHA inputs required');
    const checkoutSha = dependencies.checkoutSha ?? (root => execute('git', ['-C', root, 'rev-parse', 'HEAD']).trim());
    receipt.main_sha = checkoutSha(options.mainRoot);
    receipt.official_sha = checkoutSha(options.officialRoot);
    if (receipt.main_sha !== options.mainSha || receipt.official_sha !== options.officialSha) throw new Error('paired checkout SHA mismatch');
    if (fs.existsSync(stageRoot)) throw new Error('staging directory must not already exist');
    ownsStage = true;
    receipt.packages = stagePackages({ ...options, stageRoot });
    const cargoJobs = dependencies.cargoJobs ?? (() => execute(process.execPath, [path.join(options.mainRoot, 'scripts/node/testing/verify-runtime.js'), 'cargo-jobs'], { cwd: options.mainRoot }).trim());
    const jobs = cargoJobs();
    if (!/^[1-9]\d*$/u.test(jobs)) throw new Error('host cargo-jobs did not return a successful positive integer');
    const args = ['test', '--manifest-path', path.join(options.mainRoot, 'api/Cargo.toml'), '-p', 'runtime-extension-host', '--test', 'official_plugin_compatibility', '--', '--ignored'];
    receipt.command = ['cargo', ...args];
    receipt.cargo_build_jobs = Number(jobs);
    fs.appendFileSync(options.log, `${commandText('cargo', args)}\n`);
    receipt.exit_code = await (dependencies.runCargo ?? runCargo)('cargo', args, {
      cwd: options.mainRoot, log: options.log,
      env: { ...process.env, CARGO_BUILD_JOBS: jobs, ONEFLOWBASE_OFFICIAL_PLUGIN_ROOT: stageRoot },
    });
    receipt.passed_test_names = assertHostTestEvidence(fs.readFileSync(options.log, 'utf8'), receipt.exit_code);
    receipt.verdict = 'PASS';
    return receipt;
  } catch (error) {
    receipt.error = error.message;
    fs.appendFileSync(options.log, `conformance failure: ${error.message}\n`);
    throw error;
  } finally {
    try { fs.writeFileSync(options.artifact, `${JSON.stringify(receipt, null, 2)}\n`); }
    finally { if (ownsStage) fs.rmSync(stageRoot, { recursive: true, force: true }); }
  }
}

function parseOptions(argv) {
  const names = new Map([
    ['--main-root', 'mainRoot'], ['--official-root', 'officialRoot'], ['--main-sha', 'mainSha'],
    ['--official-sha', 'officialSha'], ['--package-dir', 'packageDir'], ['--target', 'target'],
    ['--artifact', 'artifact'], ['--log', 'log'],
  ]);
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = names.get(argv[index]);
    if (!name || !argv[index + 1] || argv[index + 1].startsWith('--') || options[name]) throw new Error(`invalid argument ${argv[index]}`);
    options[name] = argv[index + 1];
  }
  for (const name of names.values()) if (!options[name]) throw new Error(`missing ${name}`);
  for (const name of ['mainRoot', 'officialRoot', 'packageDir', 'artifact', 'log']) options[name] = path.resolve(options[name]);
  return options;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try { await runHostConformance(parseOptions(process.argv.slice(2))); }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
