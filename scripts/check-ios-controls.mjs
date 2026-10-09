// Run the actual UIKit overlay's touch routing checks without a Tauri/Rust host.
// Usage: node scripts/check-ios-controls.mjs <available iOS simulator UDID>
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const device = process.argv[2];
if (!device || process.platform !== 'darwin') throw new Error('Pass an available iOS simulator UDID on macOS.');
function run(cmd, args, timeout = 120000) {
  const result = spawnSync(cmd, args, { encoding: 'utf8', timeout });
  if (result.status !== 0) throw new Error(`${cmd} failed: ${result.error ?? ''}\n${result.stdout}\n${result.stderr}`);
  return `${result.stdout}${result.stderr}`;
}
const root = mkdtempSync(join(tmpdir(), 'noland-controls-test-'));
const bundle = 'dev.noland.inputchecks';
let bootedHere = false;
try {
  const app = join(root, 'NolandControlsTest.app');
  mkdirSync(app);
  const sdk = run('xcrun', ['--sdk', 'iphonesimulator', '--show-sdk-path']).trim();
  const source = resolve('src-tauri/native/noland-moonlight');
  run('xcrun', ['clang', '-target', 'arm64-apple-ios15.0-simulator', '-isysroot', sdk,
    '-fobjc-arc', '-framework', 'UIKit', '-framework', 'Foundation', '-framework', 'QuartzCore', '-framework', 'CoreGraphics',
    '-I', join(source, 'src'), '-I', resolve('src-tauri/native/moonlight-common-c/src'),
    join(source, 'tests/ios_controls_harness.m'), join(source, 'src/noland_stream_controls_ios.m'),
    '-o', join(app, 'NolandControlsTest')]);
  writeFileSync(join(app, 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>${bundle}</string>
<key>CFBundleExecutable</key><string>NolandControlsTest</string>
<key>CFBundleName</key><string>NolandControlsTest</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>MinimumOSVersion</key><string>15.0</string>
<key>UILaunchScreen</key><dict/>
</dict></plist>`);
  run('codesign', ['--force', '--sign', '-', app]);
  const devices = JSON.parse(run('xcrun', ['simctl', 'list', 'devices', 'available', '--json']));
  const selected = Object.values(devices.devices).flat().find(d => d.udid === device);
  if (!selected) throw new Error('Simulator not found');
  if (selected.state !== 'Booted') { run('xcrun', ['simctl', 'boot', device]); bootedHere = true; }
  run('xcrun', ['simctl', 'bootstatus', device, '-b'], 300000);
  run('xcrun', ['simctl', 'install', device, app]);
  const output = run('xcrun', ['simctl', 'launch', '--console', '--terminate-running-process', device, bundle]);
  if (!output.includes('NOLAND_UI_ROUTING_PASS')) throw new Error(`UIKit checks did not complete:\n${output}`);
  console.log('UIKit routing passed: hidden/visible gamepad hit targets, multi-button state, gesture exclusion, no keyboard requests, menu capture and cleanup.');
} finally {
  spawnSync('xcrun', ['simctl', 'uninstall', device, bundle]);
  if (bootedHere) spawnSync('xcrun', ['simctl', 'shutdown', device]);
  rmSync(root, { recursive: true, force: true });
}
