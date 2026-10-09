import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const target = process.argv[2] ?? "aarch64-apple-ios-sim";
if (!["aarch64-apple-ios-sim", "aarch64-apple-ios"].includes(target)) {
  throw new Error("Expected aarch64-apple-ios-sim or aarch64-apple-ios");
}
const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.mobile.conf.json", import.meta.url), "utf8"));
console.log(`Checking Rust and the UIKit bridge for ${target}; this does not link the native media library or produce an app.`);
const result = spawnSync("cargo", ["check", "--locked", "--manifest-path", "src-tauri/Cargo.toml", "--lib", "--target", target], {
  cwd: root,
  stdio: "inherit",
  env: {
    ...process.env,
    NOLAND_SKIP_NATIVE_BUILD: "1",
    IPHONEOS_DEPLOYMENT_TARGET: config.bundle.iOS.minimumSystemVersion,
    TAURI_CONFIG: JSON.stringify(config),
  },
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
