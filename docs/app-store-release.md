# App Store release setup

The repository contains two GitHub Actions workflows:

- `iOS CI` validates the web application, localization, release metadata, opaque
  icon set, native stream backend, contracts, and packet-tunnel extension.
- `App Store Connect` creates and validates a signed App Store IPA, retains it as
  a workflow artifact, and uploads it to App Store Connect/TestFlight.

The release workflow is protected by the `app-store` GitHub environment. A manual
run can build without uploading by disabling its upload switch. A pushed tag that
exactly matches `ios-v<package-version>` always validates and uploads. It does not
automatically submit a build for App Review.

## One-time Apple setup

1. Enroll the team in the Apple Developer Program and confirm team
   `U66WLT4SP6` owns both identifiers:
   - `noland.main.app`
   - `noland.main.app.PacketTunnel`
2. Ask Apple to approve the **Network Extension / Packet Tunnel Provider**
   capability for both the app and extension where required. App Store signing
   cannot be completed with development-only entitlement approval.
3. Create the App Store Connect app record for `noland.main.app`. The SKU is an
   internal choice. Keep the displayed product name consistent with the final
   App Store name.
4. Create an Apple Distribution certificate and export it as a password-protected
   `.p12` including its private key.
5. Create separate **App Store** provisioning profiles for the app and packet
   tunnel extension. Both profiles must include the entitlements in
   `src-tauri/apple/project.yml`; the app profile must embed the extension.
6. In App Store Connect, create a team API key with App Manager access. Save its
   issuer ID, key ID, and one-time `.p8` private key download.

## GitHub configuration

Create a protected environment named `app-store`. Add required reviewers if a
human must approve production uploads. Add these environment secrets:

| Secret | Value |
| --- | --- |
| `IOS_DISTRIBUTION_CERTIFICATE_BASE64` | Base64 of the distribution `.p12` |
| `IOS_DISTRIBUTION_CERTIFICATE_PASSWORD` | Password used when exporting it |
| `IOS_APP_PROVISIONING_PROFILE_BASE64` | Base64 of the app App Store profile |
| `IOS_TUNNEL_PROVISIONING_PROFILE_BASE64` | Base64 of the extension App Store profile |
| `APPSTORE_ISSUER_ID` | App Store Connect API issuer UUID |
| `APPSTORE_KEY_ID` | API key ID |
| `APPSTORE_PRIVATE_KEY` | Complete contents of `AuthKey_<id>.p8` |

On macOS, generate a single-line base64 value with:

```sh
base64 -i file.p12 | pbcopy
base64 -i app.mobileprovision | pbcopy
```

Never commit certificates, profiles, API keys, passwords, or generated IPAs.

The environment has been created in this repository. After generating the Apple
assets, an administrator can populate it from macOS with:

```sh
base64 -i Distribution.p12 | gh secret set IOS_DISTRIBUTION_CERTIFICATE_BASE64 --env app-store
gh secret set IOS_DISTRIBUTION_CERTIFICATE_PASSWORD --env app-store
base64 -i NolandAppStore.mobileprovision | gh secret set IOS_APP_PROVISIONING_PROFILE_BASE64 --env app-store
base64 -i NolandTunnelAppStore.mobileprovision | gh secret set IOS_TUNNEL_PROVISIONING_PROFILE_BASE64 --env app-store
gh secret set APPSTORE_ISSUER_ID --env app-store
gh secret set APPSTORE_KEY_ID --env app-store
gh secret set APPSTORE_PRIVATE_KEY --env app-store < AuthKey_KEYID.p8
```

The workflow fails before signing with secret names only—never values—if setup is
incomplete. It decodes and verifies both profile bundle identifiers, uses an
ephemeral keychain, and deletes temporary signing material even after failure.

## Store listing work the owner must supply

The binary pipeline cannot truthfully invent these business/legal materials:

- final app name, subtitle, description, keywords, category, copyright;
- public support URL, marketing URL (optional), and privacy-policy URL;
- iPhone/iPad screenshots for every device class selected in App Store Connect;
- age-rating answers and App Privacy answers based on actual backend operations;
- review contact details and a review/demo account if authentication is required;
- reviewer notes explaining why the packet tunnel is needed and how to test it;
- availability, pricing, and phased-release choices.

The checked-in privacy manifest declares no tracking or developer data
collection and declares app-owned `NSUserDefaults` use. Reconcile it and the App
Privacy questionnaire against production analytics, logging, account, and server
behavior before submission. Third-party SDK or backend changes can change the
correct answers.

## Important review risks

- The app opens external Vast.ai login, API-key, and billing pages and uses paid
  cloud compute for functionality consumed in the app. Apple may classify this
  under App Review Guideline 3.1.1. Resolve the business model with App Review or
  qualified counsel before submitting; CI success does not imply policy approval.
- Packet-tunnel apps receive additional entitlement and reviewer scrutiny. Give
  Apple precise test steps and explain that the tunnel reaches the user's remote
  workstation for streaming.
- If accounts can be created in the app or via a linked flow, verify whether
  Guideline 5.1.1 requires in-app account deletion.

## Release procedure

1. Increment `version` in `package.json` and `src-tauri/tauri.mobile.conf.json`.
   Set the same marketing version for app and extension in
   `src-tauri/apple/project.yml`. CI assigns both app and extension a unique build
   number from the GitHub run number and attempt; do not manually edit build
   numbers for CI releases.
2. Run `npm ci`, `npm run build`, and `npm run check:ios:release` locally.
3. Merge only after `iOS CI` passes.
4. Run **App Store Connect** manually with upload disabled if a signed-archive
   rehearsal is needed. Download and inspect the retained IPA.
5. To publish the configured version automatically, create and push the exact tag:

   ```sh
   version=$(node -p "require('./package.json').version")
   git tag "ios-v$version"
   git push origin "ios-v$version"
   ```

   The workflow verifies the IPA archive, bundle/version/build numbers, app and
   extension signatures, provisioning profiles, privacy manifest, and packet
   tunnel entitlements. It then runs Apple's validation before uploading.
6. Wait for App Store Connect processing, perform TestFlight testing, and complete
   export compliance, metadata, privacy, and review details.
7. Submit the selected build for review in App Store Connect. Keep review
   submission until the listing, legal answers, billing model, and entitlement
   approval are complete.

`ITSAppUsesNonExemptEncryption` is currently false because the app's encryption
is used for exempt standard networking/authentication behavior. The account owner
must confirm export-compliance answers for every release and operating region.
