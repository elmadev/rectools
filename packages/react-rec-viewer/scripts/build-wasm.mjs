// Builds the rectools-web crate for WebGPU and for WebGL2 and generates their JS bindings into
// ./wasm, made smaller with wasm-opt when it is installed, and copies the license in for the
// npm package
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

// Two builds: one for WebGPU, and one for WebGL2 that engine.ts loads only where the browser
// has no WebGPU. WebGPU compiles the shaders itself, so its build is a fifth of the size.
const builds = [
  { name: "rectools_web", feature: "webgpu" },
  { name: "rectools_web_gl", feature: "webgl" },
];
for (const { name, feature } of builds) {
  run("cargo", [
    "build",
    "-p",
    "rectools-web",
    "--target",
    "wasm32-unknown-unknown",
    "--profile",
    "web",
    "--no-default-features",
    "--features",
    feature,
  ]);
  run("wasm-bindgen", [
    "--target",
    "web",
    "--out-dir",
    out,
    "--out-name",
    name,
    path.join(root, "target/wasm32-unknown-unknown/web/rectools_web.wasm"),
  ]);
  // binaryen's wasm-opt; CI has it, a local build goes without
  const wasm = path.join(out, `${name}_bg.wasm`);
  try {
    run("wasm-opt", ["-Oz", "--enable-bulk-memory", "--enable-nontrapping-float-to-int", "--enable-sign-ext", "--enable-mutable-globals", wasm, "-o", wasm]);
  } catch (e) {
    if (e.code !== "ENOENT") throw e;
    console.log("wasm-opt isn't installed, so the wasm is left as it is");
  }
}
copyFileSync(path.join(root, "LICENSE.md"), path.resolve(here, "../LICENSE.md"));
