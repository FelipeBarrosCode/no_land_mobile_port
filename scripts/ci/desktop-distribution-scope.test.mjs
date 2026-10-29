import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";

function workflow(name) {
  return readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), "utf8");
}

// Inspect the known job indentation without introducing a YAML dependency.
function job(source, name) {
  const marker = `\n  ${name}:\n`;
  assert.equal(source.split(marker).length, 2, `Expected exactly one ${name} job`);
  return source.split(marker)[1].split(/\n  [a-z][a-z0-9-]*:\n/u)[0];
}

function needs(source) {
  const value = source.match(/^    needs: (.+)$/mu)?.[1];
  return value ? value.replace(/[\[\]]/gu, "").split(",").map((name) => name.trim()) : [];
}

const canonicalGuard = "github.repository == 'FelipeBarrosCode/no_land'";
const release = workflow("release");

for (const name of ["release", "nightly"]) {
  const source = workflow(name);
  const build = job(source, "build");

  test(`${name}: desktop build uses the canonical repository guard before invoking the reusable workflow`, () => {
    const condition = build.match(/^    if: (.+)$/mu)?.[1];
    assert.equal(condition, canonicalGuard);
    assert.match(build, /^    uses: \.\/\.github\/workflows\/_build\.yml$/mu);
    assert.match(build, /^    secrets: inherit$/mu);
    assert.match(job(release, "release"), /^      github\.repository == 'FelipeBarrosCode\/no_land' &&$/mu);

    for (const [repository, allowed] of [
      ["FelipeBarrosCode/no_land", true],
      ["FelipeBarrosCode/no_land_mobile_port", false],
      ["FelipeBarrosCode/no_land_android", false],
      ["fork-owner/no_land", false],
      ["fork-owner/no_land_mobile_port", false],
      ["FelipeBarrosCode/no_land-fork", false],
    ]) {
      assert.equal(runInNewContext(condition, { github: { repository } }, { timeout: 1000 }), allowed, repository);
    }
  });

  test(`${name}: validation, security, full tests and version remain required and ungated`, () => {
    for (const id of ["validation", "security", "tests", "version", ...(name === "nightly" ? ["dast"] : [])]) {
      assert.doesNotMatch(job(source, id), /^    if:|continue-on-error/mu, id);
    }
    assert.deepEqual(needs(job(source, "validation")), []);
    assert.deepEqual(needs(job(source, "security")), ["validation"]);
    assert.match(job(source, "security"), /^    uses: \.\/\.github\/workflows\/_security\.yml$/mu);
    assert.deepEqual(needs(job(source, "tests")), name === "nightly" ? ["security", "dast"] : ["security"]);
    assert.match(job(source, "tests"), /^    uses: \.\/\.github\/workflows\/_tests\.yml$/mu);
    if (name === "nightly") assert.deepEqual(needs(job(source, "dast")), ["security"]);
    assert.deepEqual(needs(job(source, "version")), ["tests"]);
    assert.deepEqual(needs(build), ["tests", "version"]);
  });

  test(`${name}: distribution keeps implicit success gating so a skipped or failed build cannot distribute`, () => {
    const downstream = name === "release"
      ? [["package-validation", ["build", "version"]], ["release", ["version", "package-validation"]]]
      : [["discard", ["build"]]];
    for (const [id, dependencies] of downstream) {
      const distribution = job(source, id);
      assert.deepEqual(needs(distribution), dependencies, id);
      assert.doesNotMatch(distribution, /\b(?:always|cancelled|failure|success)\s*\(|continue-on-error/u, id);
      if (id !== "release") assert.doesNotMatch(distribution, /^    if:/mu, id);
    }
    assert.doesNotMatch(build, /continue-on-error/u);
    if (name === "release") {
      assert.match(job(source, "release"), /if: >-\n      github\.repository == 'FelipeBarrosCode\/no_land' &&\n      github\.ref == 'refs\/heads\/main' &&\n      \(github\.event_name == 'push' \|\| github\.event_name == 'workflow_dispatch'\)/u);
    }
  });
}

test("native iOS distribution remains owned by App Store Connect, independent of desktop jobs", () => {
  const appStore = workflow("app-store");
  const archive = job(appStore, "archive");
  assert.match(appStore, /push:\n    branches: \[main\]\n    tags:\n      - "ios-v\*"/u);
  assert.deepEqual(needs(job(appStore, "remote-agents")), []);
  assert.match(job(appStore, "remote-agents"), /^    uses: \.\/\.github\/workflows\/_remote-agents\.yml$/mu);
  assert.deepEqual(needs(archive), ["remote-agents"]);
  assert.doesNotMatch(archive, /^    if:|github\.repository|_build\.yml|continue-on-error/mu);
  assert.match(archive, /^    environment: app-store$/mu);
  assert.match(archive, /npm run check:ios:release/u);
  assert.match(archive, /name: Require App Store secrets/u);
  assert.match(archive, /name: Import App Store signing assets/u);
  assert.match(archive, /npm run tauri:ios:build/u);
  assert.match(archive, /codesign --verify --deep --strict --verbose=2/u);
  assert.match(archive, /name: Publish unsigned iOS GitHub release/u);
  assert.match(archive, /name: Upload to App Store Connect\n        if: \$\{\{ github\.event_name == 'push' \|\| inputs\.upload \}\}/u);
  assert.match(archive, /xcrun altool --validate-app/u);
  assert.match(archive, /xcrun altool --upload-app/u);
  assert.match(archive, /name: Clean signing material\n        if: always\(\)/u);

  const iosCI = workflow("ios-ci");
  const validate = job(iosCI, "validate");
  assert.deepEqual(needs(validate), ["remote-agents"]);
  assert.match(job(iosCI, "remote-agents"), /^    uses: \.\/\.github\/workflows\/_remote-agents\.yml$/mu);
  assert.doesNotMatch(iosCI, /github\.repository|_build\.yml|^    if:|continue-on-error/mu);
  assert.match(validate, /npm run check:ios:release/u);
  assert.match(validate, /npm run check:ios:vpn/u);
});

test("PR CI already discovers the source-only regression test glob", () => {
  assert.match(workflow("ci"), /node --test [^\n]*scripts\/ci\/\*\.test\.mjs/u);
});
