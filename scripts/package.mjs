#!/usr/bin/env node
/**
 * llm-wiki Multi-Platform Package Script
 *
 * Builds CLI binaries, desktop app bundles, and skill packages for one or
 * more target platforms.
 *
 * Usage:
 *   node scripts/package.mjs [--target <tag>] [flags]
 *
 * Targets (--target):
 *   macos-arm64    macOS Apple Silicon     (aarch64-apple-darwin)
 *   macos-x64      macOS Intel             (x86_64-apple-darwin)
 *   windows-x64    Windows x86_64          (x86_64-pc-windows-msvc)
 *   linux-x64      Linux x86_64            (x86_64-unknown-linux-gnu)
 *   linux-arm64    Linux ARM64             (aarch64-unknown-linux-gnu)
 *   current        Host platform           (auto-detected)
 *   all            All supported targets
 *
 * Flags:
 *   --setup            Install required Rust cross-compilation targets
 *   --dry-run          Print what would be built without building
 *   --skip-frontend    Skip Vite/TypeScript frontend build
 *   --skip-cli         Skip CLI binary build
 *   --skip-desktop     Skip Tauri desktop app build
 *   --skip-skill       Skip skill package
 *
 * Examples:
 *   node scripts/package.mjs                           # current platform only
 *   node scripts/package.mjs --target all --setup       # everything + install targets
 *   node scripts/package.mjs --target linux-x64 --skip-desktop  # CLI only for Linux
 */

import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { arch as _arch, platform as _platform } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// ---------------------------------------------------------------------------
// Resolve paths
// ---------------------------------------------------------------------------
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT = resolve(__dirname, "..");
const RELEASE_DIR = join(ROOT, "release");
const SKILL_SRC = join(ROOT, "skills", "llm-wiki-cli");
const VERSION = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;

// ---------------------------------------------------------------------------
// Target definitions
// ---------------------------------------------------------------------------
const TARGETS = {
  "macos-arm64": {
    rustTarget: "aarch64-apple-darwin",
    os: "macos",
    arch: "arm64",
    cliName: "wiki",
    desktopExt: "dmg",
    description: "macOS Apple Silicon",
  },
  "macos-x64": {
    rustTarget: "x86_64-apple-darwin",
    os: "macos",
    arch: "x64",
    cliName: "wiki",
    desktopExt: "dmg",
    description: "macOS Intel",
  },
  "windows-x64": {
    rustTarget: "x86_64-pc-windows-gnu",
    os: "windows",
    arch: "x64",
    cliName: "wiki.exe",
    desktopExt: "msi",
    description: "Windows x86_64",
    crossEnv: { USERPROFILE: "/tmp" },
  },
  "linux-x64": {
    rustTarget: "x86_64-unknown-linux-gnu",
    os: "linux",
    arch: "x64",
    cliName: "wiki",
    desktopExt: "deb",
    description: "Linux x86_64",
  },
  "linux-arm64": {
    rustTarget: "aarch64-unknown-linux-gnu",
    os: "linux",
    arch: "arm64",
    cliName: "wiki",
    desktopExt: "deb",
    description: "Linux ARM64",
  },
};

const ALL_TARGETS = Object.keys(TARGETS);

// ---------------------------------------------------------------------------
// Host platform detection
// ---------------------------------------------------------------------------
function detectHost() {
  const os = _platform();   // darwin | win32 | linux
  const arch = _arch();     // arm64 | x64
  if (os === "darwin")
    return arch === "arm64" ? "macos-arm64" : "macos-x64";
  if (os === "win32")
    return "windows-x64";
  if (os === "linux")
    return arch === "arm64" ? "linux-arm64" : "linux-x64";
  return "linux-x64"; // fallback
}

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------
function parseArgs() {
  const args = process.argv.slice(2);
  const opts = {
    target: null,
    setup: false,
    dryRun: false,
    skipFrontend: false,
    skipCli: false,
    skipDesktop: false,
    skipSkill: false,
  };

  for (let i = 0; i < args.length; i++) {
    switch (args[i]) {
      case "--target":
        opts.target = args[++i];
        break;
      case "--setup":
        opts.setup = true;
        break;
      case "--dry-run":
        opts.dryRun = true;
        break;
      case "--skip-frontend":
        opts.skipFrontend = true;
        break;
      case "--skip-cli":
        opts.skipCli = true;
        break;
      case "--skip-desktop":
        opts.skipDesktop = true;
        break;
      case "--skip-skill":
        opts.skipSkill = true;
        break;
      case "--help":
      case "-h":
        printHelp();
        process.exit(0);
      default:
        console.error(`Unknown flag: ${args[i]}`);
        process.exit(1);
    }
  }

  if (!opts.target) {
    opts.target = detectHost();
  }

  // Resolve "all" → every supported target
  if (opts.target === "all") {
    opts.targets = [...ALL_TARGETS];
    opts.isAll = true;
  } else if (opts.target === "current") {
    opts.targets = [detectHost()];
    opts.isAll = false;
  } else {
    if (!TARGETS[opts.target]) {
      console.error(
        `Unknown target: "${opts.target}". Valid targets: ${ALL_TARGETS.join(", ")}, all, current`
      );
      process.exit(1);
    }
    opts.targets = [opts.target];
    opts.isAll = false;
  }

  return opts;
}

function printHelp() {
  console.log(`llm-wiki Package Script v${VERSION}

Usage:
  node scripts/package.mjs [--target <tag>] [flags]

Targets (--target):
  macos-arm64    macOS Apple Silicon     (aarch64-apple-darwin)
  macos-x64      macOS Intel             (x86_64-apple-darwin)
  windows-x64    Windows x86_64          (x86_64-pc-windows-msvc)
  linux-x64      Linux x86_64            (x86_64-unknown-linux-gnu)
  linux-arm64    Linux ARM64             (aarch64-unknown-linux-gnu)
  current        Host platform           (auto-detected)
  all            All supported targets

Flags:
  --setup            Install required Rust cross-compilation targets
  --dry-run          Print what would be built without building
  --skip-frontend    Skip Vite/TypeScript frontend build
  --skip-cli         Skip CLI binary build
  --skip-desktop     Skip Tauri desktop app build
  --skip-skill       Skip skill package

Examples:
  node scripts/package.mjs                           # current platform only
  node scripts/package.mjs --target all --setup       # everything + install targets
  node scripts/package.mjs --target linux-x64 --skip-desktop  # CLI only for Linux
`);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function run(cmd, args, opts = {}) {
  const { cwd = ROOT, env = process.env, ignoreError = false } = opts;
  const result = spawnSync(cmd, args, {
    cwd,
    stdio: "inherit",
    shell: process.platform === "win32",
    env,
  });
  if (result.status !== 0 && !ignoreError) {
    console.error(`\n  Command failed: ${cmd} ${args.join(" ")}`);
    console.error(`  Exit code: ${result.status}`);
    process.exit(result.status ?? 1);
  }
  return result;
}

function quietRun(cmd, args, opts = {}) {
  const { cwd = ROOT } = opts;
  return spawnSync(cmd, args, {
    cwd,
    stdio: "pipe",
    shell: process.platform === "win32",
    encoding: "utf8",
  });
}

function step(msg) {
  console.log(`\n  ▸ ${msg}`);
}

function section(title) {
  const line = "─".repeat(60);
  console.log(`\n${line}`);
  console.log(`  ${title}`);
  console.log(line);
}

function ensureDir(dir) {
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
}

function copyIf(src, dst) {
  if (existsSync(src)) {
    copyFileSync(src, dst);
    if (process.platform !== "win32") {
      try { chmodSync(dst, 0o755); } catch { /* ok */ }
    }
    return true;
  }
  return false;
}

/** Check if a Rust target is installed */
function hasRustTarget(target) {
  const result = quietRun("rustup", ["target", "list", "--installed"]);
  if (result.status !== 0) return false;
  return result.stdout.includes(target);
}

/** Install a Rust cross-compilation target */
function installRustTarget(target) {
  if (hasRustTarget(target)) {
    console.log(`    Rust target ${target} already installed`);
    return;
  }
  step(`Installing Rust target: ${target}`);
  run("rustup", ["target", "add", target]);
}

// ---------------------------------------------------------------------------
// Build steps
// ---------------------------------------------------------------------------

/** Step A: Build the React frontend (TypeScript + Vite) */
function buildFrontend(dryRun) {
  section("Frontend (TypeScript + Vite)");
  if (dryRun) {
    console.log("  [dry-run] npm run build");
    return;
  }
  run("npm", ["run", "build"]);
  console.log("  Frontend built → dist/");
}

/** Step B: Build CLI binary for a single target */
function buildCli(targetInfo, targetTag, dryRun) {
  const { rustTarget, cliName, os, crossEnv } = targetInfo;

  section(`CLI — ${targetTag} (${rustTarget})`);
  const outDir = join(RELEASE_DIR, targetTag, "cli");
  const outPath = join(outDir, cliName);
  const srcPath = join(ROOT, "target", rustTarget, "release", cliName);

  if (dryRun) {
    const buildCmd = os === "linux" ? "cargo zigbuild" : "cargo build";
    console.log(`  [dry-run] ${buildCmd} --release -p llm-wiki-cli --target ${rustTarget}`);
    console.log(`  [dry-run] Output → ${outPath}`);
    return;
  }

  ensureDir(outDir);

  // Install cross-compilation target if needed
  if (rustTarget !== detectHostRustTarget()) {
    installRustTarget(rustTarget);
  }

  // Cross-compilation environment variables (e.g. USERPROFILE for Windows)
  const buildEnv = { ...process.env };
  if (crossEnv) {
    Object.assign(buildEnv, crossEnv);
  }

  step(`Building CLI for ${rustTarget}...`);

  // Use cargo-zigbuild for Linux targets (needs zig as cross-linker)
  const isLinux = os === "linux";
  if (isLinux) {
    run("cargo", ["zigbuild", "--release", "-p", "llm-wiki-cli", "--target", rustTarget], { env: buildEnv });
  } else {
    run("cargo", ["build", "--release", "-p", "llm-wiki-cli", "--target", rustTarget], { env: buildEnv });
  }

  if (copyIf(srcPath, outPath)) {
    console.log(`  CLI binary → ${outPath}`);
  } else {
    console.error(`  WARNING: CLI binary not found at ${srcPath}`);
  }

  // Bundle the appropriate install script alongside the CLI binary
  const scriptsDir = join(ROOT, "scripts");
  if (os === "windows") {
    const ps1Src = join(scriptsDir, "install-cli.ps1");
    const ps1Dst = join(outDir, "install-cli.ps1");
    copyFileSync(ps1Src, ps1Dst);
    console.log(`  Install script → ${ps1Dst}`);
  } else {
    const shSrc = join(scriptsDir, "install-cli.sh");
    const shDst = join(outDir, "install-cli.sh");
    copyFileSync(shSrc, shDst);
    chmodSync(shDst, 0o755);
    console.log(`  Install script → ${shDst}`);
  }
}

/** Detect host Rust target triple */
function detectHostRustTarget() {
  const result = quietRun("rustc", ["-vV"]);
  const match = result.stdout.match(/host:\s*(\S+)/);
  return match ? match[1] : "aarch64-apple-darwin";
}

/** Step C: Build Tauri desktop app */
function buildDesktop(targetInfo, targetTag, dryRun) {
  const { rustTarget, os } = targetInfo;
  const hostTag = detectHost();

  section(`Desktop App — ${targetTag} (${rustTarget})`);

  // Tauri bundling requires native platform tooling.
  // Cross-OS builds need CI/CD (GitHub Actions) or a native machine.
  const hostOs = hostTag.startsWith("macos") ? "macos" : hostTag.startsWith("windows") ? "windows" : "linux";

  if (os !== hostOs) {
    console.log(`  ⓘ  Desktop app for "${os}" cannot be bundled from "${hostOs}".`);
    console.log(`  → Use GitHub Actions or a ${os} machine for the full desktop bundle.`);
    console.log(`  → Run: npx tauri build --target ${rustTarget}`);
    return;
  }

  // For macOS, match arch
  if (os === "macos" && targetTag !== hostTag) {
    console.log(`  ⓘ  Building macOS ${targetTag} from ${hostTag} — attempting cross-arch build.`);
    console.log(`  → Install target: rustup target add ${rustTarget}`);
  }

  if (dryRun) {
    console.log(`  [dry-run] npx tauri build --target ${rustTarget}`);
    return;
  }

  step(`Building Tauri desktop app for ${rustTarget}...`);
  run("npx", ["tauri", "build", "--target", rustTarget]);

  // Find the produced bundle and copy to release dir
  const bundleDir = join(ROOT, "target", rustTarget, "release", "bundle");
  if (existsSync(bundleDir)) {
    const outDir = join(RELEASE_DIR, targetTag, "desktop");
    ensureDir(outDir);

    // Copy DMG (macOS)
    const dmgDir = join(bundleDir, "dmg");
    if (existsSync(dmgDir)) {
      for (const f of readDirSafe(dmgDir)) {
        if (f.endsWith(".dmg")) {
          copyFileSync(join(dmgDir, f), join(outDir, f));
          console.log(`  Desktop bundle → ${join(outDir, f)}`);
        }
      }
    }

    // Copy MSI / NSIS (Windows)
    const msiDir = join(bundleDir, "msi");
    if (existsSync(msiDir)) {
      for (const f of readDirSafe(msiDir)) {
        copyFileSync(join(msiDir, f), join(outDir, f));
        console.log(`  Desktop bundle → ${join(outDir, f)}`);
      }
    }
    const nsisDir = join(bundleDir, "nsis");
    if (existsSync(nsisDir)) {
      for (const f of readDirSafe(nsisDir)) {
        copyFileSync(join(nsisDir, f), join(outDir, f));
        console.log(`  Desktop bundle → ${join(outDir, f)}`);
      }
    }

    // Copy DEB / AppImage (Linux)
    const debDir = join(bundleDir, "deb");
    if (existsSync(debDir)) {
      for (const f of readDirSafe(debDir)) {
        copyFileSync(join(debDir, f), join(outDir, f));
        console.log(`  Desktop bundle → ${join(outDir, f)}`);
      }
    }
    const appimageDir = join(bundleDir, "appimage");
    if (existsSync(appimageDir)) {
      for (const f of readDirSafe(appimageDir)) {
        copyFileSync(join(appimageDir, f), join(outDir, f));
        console.log(`  Desktop bundle → ${join(outDir, f)}`);
      }
    }
  } else {
    console.log(`  WARNING: No Tauri bundle found at ${bundleDir}`);
  }
}

function readDirSafe(dir) {
  try {
    const result = spawnSync("ls", ["-1", dir], {
      stdio: "pipe",
      encoding: "utf8",
    });
    if (result.status !== 0) return [];
    return result.stdout.trim().split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

/** Step D: Package the Claude Code skill */
function packageSkill(targetInfo, targetTag, dryRun) {
  section(`Skill — ${targetTag}`);

  const outDir = join(RELEASE_DIR, targetTag, "skill");
  const outZip = join(outDir, "llm-wiki-cli.zip");

  if (dryRun) {
    console.log(`  [dry-run] zip ${SKILL_SRC} → ${outZip}`);
    return;
  }

  ensureDir(outDir);

  // On macOS/Linux use zip, on Windows use PowerShell Compress-Archive
  if (process.platform === "win32") {
    run("powershell", [
      "-Command",
      `Compress-Archive -Path "${SKILL_SRC}\\*" -DestinationPath "${outZip}" -Force`,
    ]);
  } else {
    // Remove old zip if exists
    try {
      const { unlinkSync } = { unlinkSync: (p) => {
        spawnSync("rm", ["-f", p]);
      }};
      unlinkSync(outZip);
    } catch { /* ok */ }

    run("zip", ["-r", "-q", outZip, ".", "-x", "*.DS_Store", "-x", "__MACOSX/*"], { cwd: SKILL_SRC });
  }

  console.log(`  Skill package → ${outZip}`);

  // Also copy the raw skill directory for direct use
  const rawDir = join(outDir, "llm-wiki-cli");
  ensureDir(rawDir);
  run("cp", ["-r", `${SKILL_SRC}/.`, rawDir], { ignoreError: true });
  console.log(`  Skill raw copy → ${rawDir}`);
}

/** Step E: Write per-target INSTALL.md */
function writeInstallDoc(targetInfo, targetTag) {
  const { cliName, os, arch } = targetInfo;
  const outDir = join(RELEASE_DIR, targetTag);
  ensureDir(outDir);

  let content = `# llm-wiki v${VERSION} — ${targetInfo.description} (${arch})\n\n`;

  content += `## CLI\n\n`;
  if (os === "windows") {
    content += `### Install\n\n`;
    content += `**Option 1: PowerShell installer (recommended)**\n\n`;
    content += `\`\`\`powershell\n`;
    content += `powershell -ExecutionPolicy Bypass -File .\\cli\\install-cli.ps1\n`;
    content += `\`\`\`\n\n`;
    content += `**Option 2: Manual install**\n\n`;
    content += `\`\`\`powershell\n`;
    content += `mkdir -Force "$env:LOCALAPPDATA\\llm-wiki\\bin"\n`;
    content += `cp .\\cli\\${cliName} "$env:LOCALAPPDATA\\llm-wiki\\bin\\"\n`;
    content += `[Environment]::SetEnvironmentVariable("PATH", "$env:PATH;$env:LOCALAPPDATA\\llm-wiki\\bin", "User")\n`;
    content += `\`\`\`\n\n`;
  } else {
    content += `### Install\n\n`;
    content += `**Option 1: Install script (recommended)**\n\n`;
    content += `\`\`\`bash\n`;
    content += `bash cli/install-cli.sh\n`;
    content += `\`\`\`\n\n`;
    content += `**Option 2: Manual install**\n\n`;
    content += `\`\`\`bash\n`;
    content += `mkdir -p ~/.local/bin\n`;
    content += `cp cli/${cliName} ~/.local/bin/wiki\n`;
    content += `chmod +x ~/.local/bin/wiki\n`;
    content += `export PATH="$HOME/.local/bin:$PATH"\n`;
    content += `\`\`\`\n\n`;
  }

  content += `### Verify\n\n`;
  content += `\`\`\`bash\n${cliName} --help\n\`\`\`\n\n`;

  if (existsSync(join(outDir, "desktop"))) {
    content += `## Desktop App\n\n`;
    if (os === "macos") {
      content += `Open \`desktop/llm-wiki_*.dmg\` and drag \`llm-wiki.app\` to Applications.\n\n`;
    } else if (os === "windows") {
      content += `Run \`desktop/llm-wiki_*-setup.exe\` or \`desktop/llm-wiki_*.msi\`.\n\n`;
    } else {
      content += `Install \`desktop/llm-wiki_*.deb\`:\n\n`;
      content += `\`\`\`bash\n`;
      content += `sudo dpkg -i desktop/llm-wiki_*.deb\n`;
      content += `# Or use the AppImage:\n`;
      content += `chmod +x desktop/llm-wiki_*.AppImage\n`;
      content += `\`\`\`\n\n`;
    }
  }

  content += `## Claude Code Skill\n\n`;
  content += `### Install\n\n`;
  content += `\`\`\`bash\n`;
  content += `# Option 1: From zip\n`;
  content += `mkdir -p ~/.claude/skills\n`;
  content += `unzip skill/llm-wiki-cli.zip -d ~/.claude/skills/llm-wiki-cli\n\n`;
  content += `# Option 2: From raw directory\n`;
  content += `cp -r skill/llm-wiki-cli ~/.claude/skills/\n`;
  content += `\`\`\`\n\n`;

  content += `## Configuration\n\n`;
  content += `Copy \`wiki_config.yaml.example\` to \`~/.config/llm-wiki/wiki_config.yaml\`\n`;
  content += `and edit with your API keys and preferences.\n\n`;
  content += `See the full README at: https://github.com/llm-wiki/llm-wiki-rust\n`;

  writeFileSync(join(outDir, "INSTALL.md"), content);
  console.log(`  INSTALL.md written → ${join(outDir, "INSTALL.md")}`);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
async function main() {
  const opts = parseArgs();

  section(`llm-wiki v${VERSION} — Package Builder`);
  console.log(`  Targets: ${opts.targets.join(", ")}`);
  if (opts.dryRun) console.log("  Mode: DRY RUN (no actual builds)");
  if (opts.setup) console.log("  Setup: installing cross-compilation targets");
  console.log(`  Host:   ${detectHost()} (${detectHostRustTarget()})`);

  // Setup: install Rust targets for all requested platforms
  if (opts.setup) {
    section("Installing Rust Targets");
    const seen = new Set();
    for (const tag of opts.targets) {
      const t = TARGETS[tag];
      if (t && !seen.has(t.rustTarget) && t.rustTarget !== detectHostRustTarget()) {
        installRustTarget(t.rustTarget);
        seen.add(t.rustTarget);
      }
    }
  }

  // Build frontend once (shared across desktop builds)
  if (!opts.skipFrontend && !opts.skipDesktop) {
    buildFrontend(opts.dryRun);
  }

  // Iterate targets
  for (const tag of opts.targets) {
    const t = TARGETS[tag];
    section(`Target: ${tag} (${t.rustTarget}) — ${t.description}`);

    // CLI
    if (!opts.skipCli) {
      buildCli(t, tag, opts.dryRun);
    }

    // Desktop
    if (!opts.skipDesktop) {
      buildDesktop(t, tag, opts.dryRun);
    }

    // Skill
    if (!opts.skipSkill) {
      packageSkill(t, tag, opts.dryRun);
    }

    // INSTALL.md
    if (!opts.dryRun) {
      writeInstallDoc(t, tag);
    }
  }

  // Summary
  section("Done");
  console.log(`  Release artifacts under: ${RELEASE_DIR}`);
  if (!opts.dryRun) {
    const treeResult = quietRun("find", [RELEASE_DIR, "-type", "f", "-maxdepth", "4"]);
    if (treeResult.status === 0) {
      for (const line of treeResult.stdout.trim().split("\n")) {
        console.log(`    ${line.replace(RELEASE_DIR + "/", "")}`);
      }
    }
  }
  console.log("");
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
