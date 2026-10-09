import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const fail = (message) => { throw new Error(`[iOS release] ${message}`); };
const text = (path) => readFileSync(resolve(root, path), "utf8");
const packageJson = JSON.parse(text("package.json"));
const mobileConfig = JSON.parse(text("src-tauri/tauri.mobile.conf.json"));
const project = text("src-tauri/apple/project.yml");
const version = packageJson.version;
const escapedVersion = version.replaceAll(".", "\\.");

if (mobileConfig.version !== version) fail(`package version ${version} differs from mobile version ${mobileConfig.version}`);
if (mobileConfig.identifier !== "noland.main.app") fail("unexpected application bundle identifier");
if (!new RegExp(`CFBundleShortVersionString: ${escapedVersion}(?:\\s|$)`).test(project)) fail("app marketing version is not synchronized");
const appBuild = project.match(/CFBundleShortVersionString: [^\n]+\n\s+CFBundleVersion: "(\d+)"/u)?.[1];
const extensionBuild = project.match(/Noland Tunnel[\s\S]*?CFBundleVersion: "(\d+)"/u)?.[1];
if (!appBuild || appBuild !== extensionBuild) fail("app and packet-tunnel build numbers must match");
if (!project.includes("ITSAppUsesNonExemptEncryption: false")) fail("export-compliance declaration is missing");
if (!project.includes("packet-tunnel-provider")) fail("packet-tunnel entitlement is missing");
if (!project.includes("PrivacyInfo.xcprivacy")) {
  // XcodeGen discovers the manifest through the Sources/noland-connect folder.
  const privacy = text("src-tauri/apple/Sources/noland-connect/PrivacyInfo.xcprivacy");
  if (!privacy.includes("NSPrivacyAccessedAPITypes")) fail("privacy manifest is incomplete");
}

const iconDir = resolve(root, "src-tauri/icons/ios");
const icons = readdirSync(iconDir).filter((name) => name.endsWith(".png"));
if (!icons.includes("AppIcon-512@2x.png")) fail("1024x1024 App Store icon is missing");
for (const name of icons) {
  const png = readFileSync(resolve(iconDir, name));
  if (png.toString("ascii", 1, 4) !== "PNG") fail(`${name} is not PNG`);
  const width = png.readUInt32BE(16);
  const height = png.readUInt32BE(20);
  const colorType = png[25];
  if (width !== height) fail(`${name} is not square (${width}x${height})`);
  if (colorType === 4 || colorType === 6) fail(`${name} contains an alpha channel, which App Store icons reject`);
  if (name === "AppIcon-512@2x.png" && (width !== 1024 || height !== 1024)) fail("App Store icon must be 1024x1024");
}

console.log(`iOS release inputs valid: ${mobileConfig.productName} ${version} (${appBuild}), ${icons.length} opaque icons.`);
