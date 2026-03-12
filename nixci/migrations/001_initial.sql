-- GitHub App installations
CREATE TABLE IF NOT EXISTS installations (
    id INTEGER PRIMARY KEY,              -- GitHub installation ID
    account_login TEXT NOT NULL,          -- org or user name
    account_type TEXT NOT NULL,           -- "Organization" or "User"
    access_token TEXT,                    -- cached installation token
    token_expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Repos connected via the UI
CREATE TABLE IF NOT EXISTS repos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    installation_id INTEGER NOT NULL REFERENCES installations(id),
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    full_name TEXT NOT NULL,             -- "owner/name"
    default_branch TEXT NOT NULL DEFAULT 'main',
    webhook_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(owner, name)
);

-- Builds triggered by webhooks or manual retry
CREATE TABLE IF NOT EXISTS builds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    commit_sha TEXT NOT NULL,
    branch TEXT,                          -- NULL for PR builds
    pr_number INTEGER,                   -- NULL for branch builds
    status TEXT NOT NULL DEFAULT 'pending',  -- pending|running|success|failure|cancelled
    flake_attr TEXT NOT NULL,            -- e.g. ".#checks" or ".#packages.x86_64-linux.default"
    triggered_by TEXT NOT NULL,          -- "webhook" | "manual"
    started_at TEXT,
    finished_at TEXT,
    exit_code INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Log lines stored for persistence
CREATE TABLE IF NOT EXISTS build_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    build_id INTEGER NOT NULL REFERENCES builds(id),
    seq INTEGER NOT NULL,                -- ordering within build
    stream TEXT NOT NULL,                -- "stdout" | "stderr"
    line TEXT NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_build_logs_build_id ON build_logs(build_id);

-- Actions (post-build tasks in microVMs)
CREATE TABLE IF NOT EXISTS actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    build_id INTEGER NOT NULL REFERENCES builds(id),
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    name TEXT NOT NULL,
    app_attr TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending|running|success|failure|skipped
    started_at TEXT,
    finished_at TEXT,
    exit_code INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS action_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    action_id INTEGER NOT NULL REFERENCES actions(id),
    seq INTEGER NOT NULL,
    stream TEXT NOT NULL,
    line TEXT NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_action_logs_action_id ON action_logs(action_id);

-- Encrypted secret values (age-encrypted ciphertext)
CREATE TABLE IF NOT EXISTS secrets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    action_name TEXT NOT NULL,
    secret_name TEXT NOT NULL,            -- e.g. "DOCKER_PASS"
    ciphertext BLOB NOT NULL,            -- age-encrypted value
    pubkey TEXT NOT NULL,                 -- the age public key used to encrypt
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(repo_id, action_name, secret_name)
);
