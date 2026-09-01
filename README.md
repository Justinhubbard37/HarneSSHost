# HarneSSHost

HarneSSHost is a modular desktop host and evaluation platform for agent harnesses. This repository contains the product source; harness-specific upstream source checkouts and retained project evidence are kept outside the product Git tree.

## Project layout

```text
C:\dev\HarneSSHost\
├── repo\       # this product repository
├── upstream\   # independent upstream source repositories
├── audits\     # retained execution audits
├── evidence\   # retained machine evidence
└── snapshots\  # recovery bundles and verified snapshots
```

The verified DeepSeek Harness source checkout is expected at `../upstream/deepseek-harness` relative to this repository.

## Development

Frontend commands run from `app/`:

```powershell
corepack pnpm build
node --test tests/openAction.test.mjs
```

Rust checks run from `app/src-tauri/` with the Windows MSVC toolchain:

```powershell
cargo +stable-x86_64-pc-windows-msvc fmt --check
cargo +stable-x86_64-pc-windows-msvc check --offline
cargo +stable-x86_64-pc-windows-msvc test --offline --no-run
```

On the current Windows host, the full Rust suite requires the established verification-only Common Controls v6 manifest on a disposable copy of the compiled test executable. Repository binaries and build configuration remain unchanged.

## Governance

- [HarneSSHost Project Doctrine — CANONICAL](docs/governance/HarneSSHost-Project-Doctrine-CANONICAL.md)
- [OnTheLLow Workflow Architecture — CANONICAL](docs/governance/OnTheLLow-Workflow-Architecture-CANONICAL.md)

These canonical documents are protected. Current Git state, test results, and audit evidence remain distinct from governance acceptance.
