// Exercise the actual Darwin ICMP implementation without linking the desktop
// media stack or needing simulator runtime availability.
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

if (process.platform !== 'darwin') throw new Error('This check requires Darwin sockets.');
const root = mkdtempSync(join(tmpdir(), 'noland-ios-network-'));
try {
  writeFileSync(join(root, 'Cargo.toml'), `[package]
name = "noland-ios-network-check"
version = "0.1.0"
edition = "2021"
[workspace]
[lib]
path = "lib.rs"
[dependencies]
libc = "0.2"
uuid = { version = "1", features = ["v4"] }
`);
  writeFileSync(join(root, 'lib.rs'), `#[path = ${JSON.stringify(resolve('src-tauri/src/services/ios_network.rs'))}]
mod ios_network;
`);
  const result = spawnSync('cargo', ['test', '--offline', '--manifest-path', join(root, 'Cargo.toml')], {
    stdio: 'inherit', env: { ...process.env, CARGO_INCREMENTAL: '0', CARGO_PROFILE_DEV_DEBUG: '0' },
  });
  process.exitCode = result.status ?? 1;
} finally {
  rmSync(root, { recursive: true, force: true });
}
