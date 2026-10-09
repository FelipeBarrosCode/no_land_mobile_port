import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const value = process.argv[2];
if (!value || !/^\d+$/u.test(value) || Number(value) < 1) {
  throw new Error("Usage: node scripts/set-ios-build-number.mjs <positive integer>");
}

const path = resolve(import.meta.dirname, "../src-tauri/apple/project.yml");
const current = readFileSync(path, "utf8");
const matches = current.match(/CFBundleVersion: "\d+"/gu) ?? [];
if (matches.length !== 2) {
  throw new Error(`Expected app and extension CFBundleVersion entries; found ${matches.length}`);
}
const updated = current.replace(/CFBundleVersion: "\d+"/gu, `CFBundleVersion: "${value}"`);
writeFileSync(path, updated);
console.log(`Set iOS app and packet-tunnel build number to ${value}.`);
