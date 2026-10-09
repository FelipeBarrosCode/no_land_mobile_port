import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const build = join(tmpdir(), "noland-native-ios-check");
const run = (command, args) => {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};

run("cmake", [
  "-S", "src-tauri/native", "-B", build,
  "-DCMAKE_SYSTEM_NAME=iOS",
  "-DCMAKE_OSX_SYSROOT=iphonesimulator",
  "-DCMAKE_OSX_ARCHITECTURES=arm64",
  "-DCMAKE_OSX_DEPLOYMENT_TARGET=15.0",
  "-DBUILD_NOLAND_MOONLIGHT_HARNESS=OFF",
  "-DBUILD_NOLAND_MOONLIGHT_TESTS=OFF",
  "-DCMAKE_BUILD_TYPE=Release",
]);
run("cmake", ["--build", build, "--parallel", "8"]);
console.log("Built the native iOS renderer, audio, input, microphone, Moonlight common C, ENet, Opus, and Mbed TLS.");
