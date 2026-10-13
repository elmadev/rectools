// Builds the rectools-web crate and generates its JS bindings into ./wasm, made smaller with
// wasm-opt when it is installed, and copies the license in for the npm package
import { copyFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import os from "node:os";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.resolve(here, "../wasm");
// Homebrew's rustup is keg-only and cargo install puts wasm-bindgen in ~/.cargo/bin, so
// neither is necessarily on PATH
const env = {
  ...process.env,
  PATH: [
    process.env.PATH,
    path.join(os.homedir(), ".cargo/bin"),
    "/opt/homebrew/opt/rustup/bin",
    "/usr/local/opt/rustup/bin",
  ].join(path.delimiter),
};
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit", env });

run("cargo", [
  "build",
  "-p",
  "rectools-web",
  "--target",
  "wasm32-unknown-unknown",
  "--profile",
  "web",
]);
run("wasm-bindgen", [
  "--target",
  "web",
  "--out-dir",
  out,
  path.join(root, "target/wasm32-unknown-unknown/web/rectools_web.wasm"),
]);
copyFileSync(path.join(root, "LICENSE.md"), path.resolve(here, "../LICENSE.md"));

// binaryen's wasm-opt; CI has it, a local build goes without
const wasm = path.join(out, "rectools_web_bg.wasm");
try {
  run("wasm-opt", ["-Oz", "--enable-bulk-memory", "--enable-nontrapping-float-to-int", "--enable-sign-ext", "--enable-mutable-globals", wasm, "-o", wasm]);
} catch (e) {
  if (e.code !== "ENOENT") throw e;
  console.log("wasm-opt isn't installed, so the wasm is left as it is");
}
