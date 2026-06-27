# llm-wiki Claude Code Skill — Installation

This skill lets Claude Code operate the `wiki` CLI for personal knowledge
base workflows.

## Prerequisites — CHECK FIRST

Before installing the skill, verify the `wiki` CLI is installed and
working:

### macOS / Linux

```bash
# Check if wiki is on PATH
which wiki && wiki --version

# If not found, install the CLI first:
# See: https://github.com/llm-wiki/llm-wiki-rust#cli-wiki
```

### Windows

```powershell
# Check if wiki is on PATH
where wiki.exe
wiki.exe --version

# If not found, install the CLI first:
# See: https://github.com/llm-wiki/llm-wiki-rust#cli-wiki
```

**If `wiki` is not installed**, download the CLI binary for your platform
first. The skill will not work without the CLI. See the project README for
CLI installation instructions.

Also verify Claude Code is available:

```bash
claude --version
```

## Install

### macOS / Linux

**Option 1: Install from zip**

```bash
mkdir -p ~/.claude/skills
unzip llm-wiki-cli.zip -d ~/.claude/skills/llm-wiki-cli
```

**Option 2: Copy raw directory**

```bash
mkdir -p ~/.claude/skills
cp -r llm-wiki-cli ~/.claude/skills/
```

**Option 3: Symlink for development**

```bash
mkdir -p ~/.claude/skills
ln -s "$(pwd)/llm-wiki-cli" ~/.claude/skills/llm-wiki-cli
```

### Windows

**Option 1: Install from zip (PowerShell)**

```powershell
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude\skills"
Expand-Archive -Path llm-wiki-cli.zip -DestinationPath "$env:USERPROFILE\.claude\skills\llm-wiki-cli"
```

**Option 2: Copy raw directory**

```powershell
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude\skills"
Copy-Item -Recurse llm-wiki-cli "$env:USERPROFILE\.claude\skills\"
```

## Verify

After installation, restart Claude Code and verify the skill is loaded:

```
/llm-wiki-cli
```

If you see the skill help output, the installation succeeded.

If the skill reports `wiki: command not found`, install the CLI binary
first (see Prerequisites above).

## Configuration

The skill reads configuration from the same sources as the CLI:

1. `LLM_WIKI_CONFIG` environment variable
2. Nearest project `wiki_config.yaml`
3. `~/.config/llm-wiki/wiki_config.yaml`

Set up your config before using the skill:

```bash
wiki config --check
```

## Troubleshooting

### "/llm-wiki-cli" not recognized

- Make sure the skill directory is at `~/.claude/skills/llm-wiki-cli/`
- Verify the directory contains `SKILL.md`
- Restart Claude Code completely

### Skill says "wiki: command not found"

The skill depends on the `wiki` CLI. Install it first:

- **macOS/Linux**: Download the `wiki` binary to `~/.local/bin/wiki`
- **Windows**: Download `wiki.exe` to `%LOCALAPPDATA%\llm-wiki\bin\wiki.exe`
- Verify with: `wiki --version`

### Skill has no effect / config not loading

Run `wiki config --check` to verify your configuration is valid and
readable.

## Uninstall

### macOS / Linux

```bash
rm -rf ~/.claude/skills/llm-wiki-cli
```

### Windows

```powershell
Remove-Item -Recurse -Force "$env:USERPROFILE\.claude\skills\llm-wiki-cli"
```
