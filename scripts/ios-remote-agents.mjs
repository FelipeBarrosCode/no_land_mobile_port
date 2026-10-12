import { cpSync, mkdirSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { REMOTE_AGENT_FILES, verifyRemoteAgentArtifacts } from './remote-agent-artifacts.mjs';

export function verifyIOSAppRemoteAgents(app) {
  // Tauri's iOS resource_dir is <app>/assets; ../ resources map to _up_.
  verifyRemoteAgentArtifacts(join(app, 'assets', '_up_', 'state-agent'));
}

export function stageIOSRemoteAgents(root) {
  const source = join(root, 'state-agent');
  try {
    verifyRemoteAgentArtifacts(source);
  } catch (error) {
    throw new Error(`iOS builds require both Linux remote-agent bundles. Run npm run prepare:ios:agents -- --run <GitHub-run-id> first. ${error.message}`);
  }
  const generated = join(root, 'src-tauri', 'gen', 'apple');
  for (const arch of ['x86_64', 'aarch64']) {
    const dest = join(generated, 'assets', '_up_', 'state-agent', 'prebuilt', arch);
    mkdirSync(dest, { recursive: true });
    for (const name of [...REMOTE_AGENT_FILES, 'SHA256SUMS']) {
      cpSync(join(source, 'prebuilt', arch, name), join(dest, name));
    }
  }
  verifyIOSAppRemoteAgents(generated);
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const root = resolve(import.meta.dirname, '..');
  if (process.argv[2] === '--verify-app' && process.argv[3]) {
    verifyIOSAppRemoteAgents(resolve(process.argv[3]));
  } else if (process.argv[2] === '--verify-staged') {
    verifyIOSAppRemoteAgents(join(root, 'src-tauri', 'gen', 'apple'));
  } else if (process.argv[2] === '--verify-source') {
    verifyRemoteAgentArtifacts(join(root, 'state-agent'));
  } else {
    throw new Error('Usage: ios-remote-agents.mjs --verify-source | --verify-staged | --verify-app <app-directory>');
  }
  console.log('Verified Linux state, lifecycle, network, and eBPF resources for x86_64 and aarch64 (no microphone).');
}
