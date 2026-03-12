# nixci

A lightweight, self-hostable Nix CI service wrapping [nix-fast-build](https://github.com/Mic92/nix-fast-build). Receives GitHub webhooks, clones repos, reads `.nixci.toml`, runs parallel eval+build, streams logs via SSE, and reports status back.

No CSS. No React. No JS beyond htmx.

## How it works

```
GitHub ──webhook──▶ nixci (axum) ──spawn──▶ nix-fast-build
       ◀──status──          │      ◀──stdout/stderr──
                         SQLite
                            │
                    Web UI (HTMX+SSE)
```

1. GitHub App sends push/PR webhook
2. nixci verifies signature, clones repo, reads `.nixci.toml`
3. For each matching build entry, spawns `nix-fast-build`
4. Streams output to web UI via SSE and persists to SQLite
5. Reports commit status back to GitHub

## Quick start

```bash
# Clone and enter dev shell
nix develop

# Set required env vars
export NIXCI_GITHUB_APP_ID=12345
export NIXCI_GITHUB_PRIVATE_KEY=/path/to/private-key.pem
export NIXCI_GITHUB_WEBHOOK_SECRET=your-webhook-secret

# Run
cargo run -p nixci
```

The server starts on `http://0.0.0.0:3000` by default.

## Configuration

### Environment variables

| Variable | Default | Description |
|---|---|---|
| `NIXCI_LISTEN` | `0.0.0.0:3000` | Listen address |
| `NIXCI_DATABASE_URL` | `sqlite:nixci.db` | SQLite database URL |
| `NIXCI_WORK_DIR` | `/tmp/nixci` | Directory for repo clones |
| `NIXCI_MAX_CONCURRENT_BUILDS` | `4` | Max parallel builds |
| `NIXCI_SECRETS_SOCKET` | `/run/nixci-secrets/nixci-secrets.sock` | Unix socket for secrets service |
| `NIXCI_GITHUB_APP_ID` | *required* | GitHub App ID |
| `NIXCI_GITHUB_PRIVATE_KEY` | *required* | Path to GitHub App private key PEM |
| `NIXCI_GITHUB_WEBHOOK_SECRET` | *required* | GitHub webhook secret |

### Per-repo config: `.nixci.toml`

Place in the repo root. If absent, defaults to building `.#checks` on all branches and PRs.

```toml
# Build .#checks on main and release branches
[[build]]
attr = ".#checks"
branches = "main|release/.*"

# Build the default package on main only
[[build]]
attr = ".#packages.x86_64-linux.default"
branches = "main"

# Build checks on all PRs
[[build]]
attr = ".#checks"
prs = true

# Or only PRs matching a pattern
# prs = "feat/.*"

# Post-build action: run in a throwaway microVM with secrets
[[action]]
name = "push-docker"
app = ".#apps.x86_64-linux.push-docker"
on = "success"
branches = "main"
secrets = ["DOCKER_USER", "DOCKER_PASS"]

# Build options
[options]
max_jobs = 4
skip_cached = true
# systems = "x86_64-linux aarch64-linux"
```

**Fields:**

- `build.attr` — flake attribute to build
- `build.branches` — regex; omit to match all branches
- `build.prs` — `true`/`false` or a regex pattern for PR branch matching
- `action.name` — action identifier
- `action.app` — flake app attribute to run
- `action.on` — `"success"` (default) or `"failure"`
- `action.branches` — regex; omit to match all
- `action.secrets` — list of secret names to inject as env vars
- `options.max_jobs` — parallel nix-fast-build jobs
- `options.skip_cached` — skip already-cached derivations
- `options.systems` — space-separated target systems

## Web UI

All pages are server-rendered with [maud](https://maud.lambda.xyz/). HTMX handles live updates.

| Route | Description |
|---|---|
| `GET /` | Dashboard — repos + recent builds |
| `GET /repos` | List connected repos |
| `GET /repos/:id` | Repo detail with build history |
| `GET /builds/:id` | Build detail with live log viewer |
| `GET /builds/:id/logs` | SSE log stream |
| `POST /builds/:id/retry` | Retry a failed build |
| `GET /actions/:id` | Action detail with logs |
| `GET /settings` | GitHub App installation status |
| `POST /webhooks/github` | Webhook receiver |

Build rows auto-refresh every 5s. Log pages stream in real-time via SSE when a build is running.

## Secrets

nixci uses a separate `nixci-secrets` process that holds a master key and derives per-repo per-action [age](https://age-encryption.org/) keys deterministically:

```
private_key = age_key(HKDF(master_key, "nixci-v1/{owner}/{repo}/{action}"))
```

The main service never sees the master key. It communicates with `nixci-secrets` over a unix socket to decrypt secrets at action runtime. Secrets are stored as age-encrypted ciphertext in SQLite.

**Public key endpoint:** `GET /secrets/pubkey/:owner/:repo/:action` — returns the age public key for encrypting secrets.

### PR secret exfiltration protection

- PRs from forks never get secrets
- Actions only run on branches matching the configured regex (typically `main`)
- `.nixci.toml` changes in PRs cause actions to be skipped (takes effect after merge)
- Log redaction of known secret values as defense-in-depth

## NixOS deployment

The flake includes a NixOS module for both services:

```nix
{
  inputs.nixci.url = "github:youruser/nixci";

  outputs = { nixci, ... }: {
    nixosConfigurations.myhost = {
      imports = [ nixci.nixosModules.default ];

      services.nixci = {
        enable = true;
        githubAppId = 12345;
        githubPrivateKeyFile = "/run/secrets/nixci-github-key.pem";
        githubWebhookSecretFile = "/run/secrets/nixci-webhook-secret";
        maxConcurrentBuilds = 8;

        secrets = {
          enable = true;
          masterKeyFile = "/run/secrets/nixci-master-key";
        };
      };
    };
  };
}
```

This creates two systemd services: `nixci` and `nixci-secrets` (if enabled), both running as dynamic users with isolated state directories.

## Building

```bash
# Dev shell
nix develop

# Build both binaries
nix build .#nixci
nix build .#nixci-secrets

# Run checks
nix flake check

# Run tests
cargo test --workspace
```

## Architecture

```
nixci/
├── flake.nix                   # Nix flake (crane build, NixOS module)
├── Cargo.toml                  # Workspace root
├── nixci/                      # Main CI service
│   ├── migrations/001_initial.sql
│   ├── static/                 # Vendored htmx.js
│   └── src/
│       ├── main.rs
│       ├── config.rs           # Env-based configuration
│       ├── db.rs               # SQLite pool + migrations
│       ├── models.rs           # DB row types
│       ├── templates.rs        # Maud HTML fragments
│       ├── forge/
│       │   ├── mod.rs          # ForgeBackend trait (pluggable)
│       │   └── github.rs       # GitHub App implementation
│       ├── builder/
│       │   ├── mod.rs          # Build orchestration + concurrency
│       │   ├── runner.rs       # nix-fast-build subprocess + log capture
│       │   └── config.rs       # .nixci.toml parsing
│       ├── actions/
│       │   ├── mod.rs          # Post-build action orchestration
│       │   ├── microvm.rs      # microVM execution (stub)
│       │   └── secrets.rs      # Unix socket client to nixci-secrets
│       └── web/
│           ├── mod.rs          # Axum router
│           ├── pages.rs        # Full-page handlers
│           ├── partials.rs     # HTMX partial responses
│           ├── sse.rs          # SSE log streaming
│           └── webhooks.rs     # Webhook receiver
└── nixci-secrets/              # Standalone secrets service
    └── src/
        ├── main.rs             # Unix socket listener
        ├── keyderive.rs        # HKDF + age key derivation
        └── protocol.rs         # IPC message types
```

The `ForgeBackend` trait is designed to be pluggable — GitHub is implemented first, other forges can be added by implementing the same trait.
