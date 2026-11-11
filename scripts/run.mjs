import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = fileURLToPath(new URL('../', import.meta.url));
const [tool, ...args] = process.argv.slice(2);
const env = { ...process.env, CARGO_TARGET_DIR: path.join(root, 'target'), CARGO_BUILD_JOBS: '2' };
const cargoHome = path.join(root, '.local-tools/cargo');
if (existsSync(path.join(cargoHome, 'bin/cargo'))) {
  env.CARGO_HOME = cargoHome;
  env.RUSTUP_HOME = path.join(root, '.local-tools/rustup');
  env.PATH = path.join(cargoHome, 'bin') + path.delimiter + env.PATH;
}
const executable = tool === 'tauri' ? path.join(root, 'node_modules/.bin/tauri') : tool;
if (!['cargo', 'rustc', 'rustup', 'tauri'].includes(tool)) throw Error('Unsupported project tool');
const child = spawn(executable, args, { cwd: root, env, stdio: 'inherit', shell: false });
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => child.kill(signal));
