import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const workflow = name => readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8');

test('shared CI checks every script suite, localization and runtime contracts', () => {
  const ci = workflow('ci');
  assert.match(ci, /workflow_call:/);
  assert.match(ci, /os: \[ubuntu-24\.04, macos-latest\]/);
  assert.match(ci, /run: npm run test:scripts/);
  assert.match(ci, /run: npm run i18n:check/);
  assert.match(ci, /runtime-contracts\/Cargo\.toml/);
  assert.match(ci, /needs: \[script-tests, typecheck-build, rust-format, rust-check, unit-tests, security\]/);
});

test('mobile releases cannot accidentally invoke the inherited six-desktop builder', () => {
  const release = workflow('release');
  assert.match(release, /workflow_call:/);
  assert.match(release, /uses: .*\/ci\.yml/);
  assert.match(release, /needs: checks\n    uses: .*\/_tests\.yml/);
  assert.doesNotMatch(release + workflow('nightly'), /_build\.yml|signed-six-platform|git tag/);
});

test('App Store signing waits for shared gates and native iOS validation', () => {
  const store = workflow('app-store');
  assert.match(store, /uses: .*\/release\.yml/);
  assert.match(store, /uses: .*\/ios-ci\.yml/);
  assert.match(store, /needs: \[validation, ios-validation\]/);
  assert.match(store, /without producing an Xcode archive[\s\S]*?exit 1/);
  assert.match(store, /--verify-app "\$APP"/);
  assert.match(workflow('ios-ci'), /uses: .*\/_remote-agents\.yml/);
  const nightly = workflow('nightly');
  assert.match(nightly, /uses: .*\/ios-ci\.yml/);
  assert.doesNotMatch(nightly, /app-store\.yml|contents: write|secrets: inherit/);
});
