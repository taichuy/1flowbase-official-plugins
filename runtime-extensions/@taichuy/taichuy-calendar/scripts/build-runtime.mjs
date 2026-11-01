import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, chmod } from 'node:fs/promises';
import { resolve } from 'node:path';
import { root } from './toolchain.mjs';

const target = process.env.CARGO_BUILD_TARGET;
const args = ['build', '--release', '--offline', '--locked', '--manifest-path', resolve(root, 'Cargo.toml')];
if (target) args.push('--target', target);
const targetDir = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : resolve(root, 'target');
const build = spawnSync('cargo', args, { cwd: root, stdio: 'inherit', env: { ...process.env, CARGO_TARGET_DIR: targetDir } });
if (build.error) throw build.error;
if (build.status !== 0) process.exit(build.status ?? 1);
const suffix = (target ? target.includes('windows') : process.platform === 'win32') ? '.exe' : '';
const directory = target ? resolve(targetDir, target, 'release') : resolve(targetDir, 'release');
await mkdir(resolve(root, 'bin'), { recursive: true });
await copyFile(resolve(directory, `taichuy-calendar${suffix}`), resolve(root, `bin/taichuy-calendar${suffix}`));
if (!suffix) await chmod(resolve(root, 'bin/taichuy-calendar'), 0o755);
