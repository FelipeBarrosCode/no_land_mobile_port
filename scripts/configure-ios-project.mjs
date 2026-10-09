import { cpSync, existsSync, mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const apple = `${root}/src-tauri/apple`;
const generated = `${root}/src-tauri/gen/apple`;
if (!existsSync(`${generated}/project.yml`)) {
  throw new Error("Run `tauri ios init` before applying the maintained Apple project sources.");
}
mkdirSync(`${generated}/Sources`, { recursive: true });
cpSync(`${apple}/Sources`, `${generated}/Sources`, { recursive: true, force: true });
cpSync(`${apple}/project.yml`, `${generated}/project.yml`, { force: true });
const xcodegen = spawnSync("xcodegen", ["generate", "--spec", `${generated}/project.yml`, "--project", generated], {
  cwd: generated,
  stdio: "inherit",
});
if (xcodegen.error) throw xcodegen.error;
if (xcodegen.status !== 0) process.exit(xcodegen.status ?? 1);
console.log("Applied maintained iOS sources, packet-tunnel target, entitlements, and WireGuardKit dependency.");
