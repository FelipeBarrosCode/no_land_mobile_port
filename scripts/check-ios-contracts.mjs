import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const baseline = "b49160a";
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
const oldEntry = git("show", `${baseline}:src-tauri/src/main.rs`);
const entry = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
function commands(text) {
  const block = text.match(/tauri::generate_handler!\[([\s\S]*?)\]/)?.[1];
  if (!block) throw new Error("Could not find the command registration block");
  const names = block.split(",").map((name) => name.trim()).filter(Boolean);
  if (!names.length || names.some((name) => !/^(?:\w+::)*\w+$/.test(name))) {
    throw new Error("Unrecognized command registration syntax; update the inventory parser");
  }
  return names.map((name) => name.split("::").at(-1));
}
const expected = commands(oldEntry);
const current = new Set(commands(entry));
const missing = expected.filter((command) => !current.has(command));
if (missing.length) throw new Error(`Missing baseline commands: ${missing.join(", ")}`);

const oldFacade = git("show", `${baseline}:src/lib/backend.ts`);
const facade = readFileSync(new URL("../src/lib/backend.ts", import.meta.url), "utf8");
const exports = (text) => [...text.matchAll(/export (?:async )?function (\w+)/g)].map((match) => match[1]);
const currentExports = new Set(exports(facade));
const missingExports = exports(oldFacade).filter((name) => !currentExports.has(name));
if (missingExports.length) throw new Error(`Missing baseline frontend APIs: ${missingExports.join(", ")}`);
console.log(`Preserved ${expected.length} registered commands and ${exports(oldFacade).length} frontend functions from ${baseline}.`);
console.log("This is a contract-removal guard, not proof of runtime behavior parity.");
