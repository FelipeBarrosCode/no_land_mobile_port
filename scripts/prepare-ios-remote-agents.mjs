import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { verifyRemoteAgentArtifacts } from './remote-agent-artifacts.mjs';

const root = resolve(import.meta.dirname, '..');
const run = process.argv[2] === '--run' ? process.argv[3] : null;
if (!run || !/^\d+$/.test(run)) {
  throw new Error('Usage: npm run prepare:ios:agents -- --run <GitHub-run-id>. Use a mobile CI run that built both remote-agent artifacts.');
}
const repo = 'FelipeBarrosCode/no_land_mobile_port';
const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
const { headSha } = JSON.parse(execFileSync('gh', ['run', 'view', run, '--repo', repo, '--json', 'headSha'], {
  cwd: root, encoding: 'utf8',
}));
// Don't silently pair changed local agent sources with stale CI executables.
const sources = ['state-agent', 'network-agent', 'network-contracts', 'scripts/ci/build-remote-agents.sh'];
if (git('status', '--porcelain', '--untracked-files=no', '--', ...sources)) {
  throw new Error('Commit agent source changes and build matching CI artifacts before downloading.');
}
for (const path of sources) {
  if (git('rev-parse', `${headSha}:${path}`) !== git('rev-parse', `HEAD:${path}`)) {
    throw new Error(`Run ${run} has outdated ${path}; choose a CI run with matching agent sources.`);
  }
}
// gh refuses to overwrite existing files. Download to a private staging folder,
// verify both complete bundles, then update local resources (safe to rerun).
const temporary = join(root, '.tmp');
mkdirSync(temporary, { recursive: true });
const staging = mkdtempSync(join(temporary, 'ios-remote-agents-'));
try {
  for (const arch of ['x86_64', 'aarch64']) {
    const dest = join(staging, 'prebuilt', arch);
    mkdirSync(dest, { recursive: true });
    execFileSync('gh', ['run', 'download', run, '--repo', repo, '--name', `remote-agents-${arch}`, '--dir', dest], {
      cwd: root, stdio: 'inherit',
    });
  }
  verifyRemoteAgentArtifacts(staging);
  cpSync(join(staging, 'prebuilt'), join(root, 'state-agent', 'prebuilt'), { recursive: true, force: true });
  verifyRemoteAgentArtifacts(join(root, 'state-agent'));
} finally {
  rmSync(staging, { recursive: true, force: true });
}
console.log('Both Linux remote-agent bundles are ready for local iOS builds.');
