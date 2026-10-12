import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { REMOTE_AGENT_FILES } from './remote-agent-artifacts.mjs';
import { stageIOSRemoteAgents, verifyIOSAppRemoteAgents } from './ios-remote-agents.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'noland-ios-agents-'));
  t.after(() => rmSync(root, { recursive: true }));
  for (const [arch, machine] of [['x86_64', 62], ['aarch64', 183]]) {
    const dir = join(root, 'state-agent', 'prebuilt', arch);
    mkdirSync(dir, { recursive: true });
    const sums = [];
    for (const name of REMOTE_AGENT_FILES) {
      const bytes = Buffer.alloc(64);
      bytes.writeUInt32BE(0x7f454c46, 0);
      bytes[4] = 2;
      bytes[5] = 1;
      bytes.writeUInt16LE(machine, 18);
      writeFileSync(join(dir, name), bytes);
      sums.push(`${createHash('sha256').update(bytes).digest('hex')}  ${name}`);
    }
    writeFileSync(join(dir, 'SHA256SUMS'), `${sums.join('\n')}\n`);
  }
  return root;
}

test('stages both Linux architectures under the iOS runtime resource path', (t) => {
  const root = fixture(t);
  stageIOSRemoteAgents(root);
  const app = join(root, 'src-tauri/gen/apple');
  verifyIOSAppRemoteAgents(app);
  for (const arch of ['x86_64', 'aarch64']) {
    for (const name of [...REMOTE_AGENT_FILES, 'SHA256SUMS']) {
      assert.deepEqual(readFileSync(join(app, 'assets/_up_/state-agent/prebuilt', arch, name)),
        readFileSync(join(root, 'state-agent/prebuilt', arch, name)));
    }
  }
});

test('refuses source-only builds instead of silently falling back to remote compilation', (t) => {
  const root = fixture(t);
  rmSync(join(root, 'state-agent/prebuilt/aarch64/noland-network-agent'));
  assert.throws(() => stageIOSRemoteAgents(root), /prepare:ios:agents/);
});

test('rejects an IPA whose resources omit or corrupt a Linux agent', (t) => {
  const root = fixture(t);
  stageIOSRemoteAgents(root);
  const app = join(root, 'src-tauri/gen/apple');
  const binary = join(app, 'assets/_up_/state-agent/prebuilt/x86_64/noland-state-agent');
  writeFileSync(binary, 'corrupt');
  assert.throws(() => verifyIOSAppRemoteAgents(app), /checksum mismatch/);
  rmSync(binary);
  assert.throws(() => verifyIOSAppRemoteAgents(app), /ENOENT/);
});

test('App Store builds run on main merges as well as iOS tags and manual dispatch', () => {
  const workflow = readFileSync(new URL('../.github/workflows/app-store.yml', import.meta.url), 'utf8');
  const triggers = workflow.split('\npermissions:')[0];
  assert.match(triggers, /push:\s*\n\s+branches: \[main\]/);
  assert.match(triggers, /tags:\s*\n\s+- "ios-v\*"/);
  assert.match(triggers, /workflow_dispatch:/);
  assert.ok(workflow.includes("github.event_name == 'push' || inputs.upload"));
});
