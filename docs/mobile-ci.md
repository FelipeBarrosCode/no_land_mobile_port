# iOS CI and releases

- Pull requests run shared TypeScript/localization, Rust, security, script and
  restore-engine checks, plus iOS contracts/streaming/controller and simulator
  packet-tunnel compilation.
- `npm run test:scripts` discovers script and feedback component regressions on
  both Linux and macOS; localized copy assertions verify referenced catalog keys.
- `app-store.yml` owns signing and publication. Both reusable `release.yml`
  (shared gates/full suite) and `ios-ci.yml` (native validation) must succeed
  before the archive job receives signing assets.
- Linux provisioning agents are built natively for x86_64 and aarch64, verified,
  and reused by the native validation and signed archive in the same run.
- Nightly runs shared and native validation without App Store secrets, uploads,
  tags or desktop signing. The inherited six-desktop builder is never called.
- GitHub receives only the separately unsigned IPA. The original signed IPA is
  retained for the existing App Store Connect upload path.

Shared desktop fixes carried forward: bounded authenticated restore streaming,
runtime app discovery/backup, diagnostic-log exclusion, and startup failures
that preserve original state. The mobile SSH and protected-storage boundaries
remain unchanged. Feedback/dashboard/menu copy now uses all nine locale bundles,
reusing existing translations where available and English fallback otherwise.

Local checks: `npm run test:scripts`, `npm run i18n:check`, `npm run build`,
`npm run check:ios:release`, and `actionlint`. Device signing is validated in CI.
