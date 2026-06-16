#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const releaseDir = join(root, "release");
const cliReleaseDir = join(releaseDir, "cli");

function run(cmd, args) {
  const result = spawnSync(cmd, args, {
    cwd: root,
    stdio: "inherit",
    shell: process.platform === "win32",
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

mkdirSync(releaseDir, { recursive: true });
mkdirSync(cliReleaseDir, { recursive: true });

run("npm", ["run", "build"]);
run("cargo", ["build", "--release", "-p", "llm-wiki-cli"]);
run("npx", ["tauri", "build"]);

const cliName = process.platform === "win32" ? "wiki.exe" : "wiki";
const cliPath = join(root, "target", "release", cliName);
if (existsSync(cliPath)) {
  const releaseCliPath = join(cliReleaseDir, cliName);
  copyFileSync(cliPath, releaseCliPath);
  if (process.platform !== "win32") {
    chmodSync(releaseCliPath, 0o755);
  }
}

writeFileSync(
  join(cliReleaseDir, "INSTALL.md"),
  `# llm-wiki CLI

This folder contains the compiled \`${cliName}\` binary.

## macOS / Linux

\`\`\`bash
mkdir -p ~/.local/bin
cp ./${cliName} ~/.local/bin/wiki
chmod +x ~/.local/bin/wiki
wiki --help
\`\`\`

If \`~/.local/bin\` is not on PATH, add this to your shell profile:

\`\`\`bash
export PATH="$HOME/.local/bin:$PATH"
\`\`\`

## Windows

Copy \`${cliName}\` to a directory on PATH, then run:

\`\`\`powershell
wiki --help
\`\`\`

## Configuration

The CLI reads configuration from:

1. \`LLM_WIKI_CONFIG\`
2. nearest project \`wiki_config.yaml\`
3. \`~/.config/llm-wiki/wiki_config.yaml\`

You do not need Rust or Cargo to use this binary.
`,
);

console.log(`Artifacts prepared under ${releaseDir}`);
console.log(`CLI binary prepared under ${cliReleaseDir}`);
