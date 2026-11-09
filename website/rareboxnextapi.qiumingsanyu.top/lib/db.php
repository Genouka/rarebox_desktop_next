<?php
/**
 * PDO/SQLite connection, schema bootstrap, and admin seeding.
 */

declare(strict_types=1);

namespace RareBox;

function db(): \PDO
{
    static $pdo = null;
    if ($pdo !== null) {
        return $pdo;
    }
    $path = RB_DB_PATH;
    $dir = dirname($path);
    if (!is_dir($dir) && !mkdir($dir, 0755, true) && !is_dir($dir)) {
        throw new \RuntimeException('Cannot create database directory: ' . $dir);
    }
    $pdo = new \PDO('sqlite:' . $path);
    $pdo->setAttribute(\PDO::ATTR_ERRMODE, \PDO::ERRMODE_EXCEPTION);
    $pdo->setAttribute(\PDO::ATTR_DEFAULT_FETCH_MODE, \PDO::FETCH_ASSOC);
    // Wait for the write lock instead of failing instantly under concurrent hits.
    $pdo->exec('PRAGMA busy_timeout = 5000');
    $pdo->exec('PRAGMA journal_mode = WAL');
    $pdo->exec('PRAGMA foreign_keys = ON');
    init_schema($pdo);
    seed_admin($pdo);
    return $pdo;
}

function init_schema(\PDO $pdo): void
{
    $pdo->exec(<<<'SQL'
CREATE TABLE IF NOT EXISTS releases (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    version       TEXT NOT NULL,
    channel       TEXT NOT NULL DEFAULT 'stable',
    platform      TEXT NOT NULL DEFAULT 'all',
    download_url  TEXT NOT NULL,
    file_size     INTEGER NOT NULL DEFAULT 0,
    notes         TEXT NOT NULL DEFAULT '',
    min_version   TEXT,
    force_update  INTEGER NOT NULL DEFAULT 0,
    published_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS announcements (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL DEFAULT '',
    level       TEXT NOT NULL DEFAULT 'info',
    active      INTEGER NOT NULL DEFAULT 1,
    starts_at   TEXT,
    ends_at     TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT
);

CREATE TABLE IF NOT EXISTS admins (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    username        TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS download_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    release_id   INTEGER,
    version      TEXT,
    platform     TEXT,
    channel      TEXT,
    ip           TEXT,
    user_agent   TEXT,
    created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS login_attempts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ip          TEXT NOT NULL,
    username    TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_releases_channel_platform ON releases (channel, platform);
CREATE INDEX IF NOT EXISTS idx_releases_published ON releases (published_at);
CREATE INDEX IF NOT EXISTS idx_announcements_active ON announcements (active);
CREATE INDEX IF NOT EXISTS idx_download_log_created ON download_log (created_at);
CREATE INDEX IF NOT EXISTS idx_download_log_platform ON download_log (platform, created_at);
CREATE INDEX IF NOT EXISTS idx_login_attempts ON login_attempts (ip, username, created_at);
SQL);
}

function seed_admin(\PDO $pdo): void
{
    $count = $pdo->query('SELECT COUNT(*) FROM admins')->fetchColumn();
    if ((int) $count === 0) {
        $pdo->prepare('INSERT INTO admins (username, password_hash, created_at) VALUES (?, ?, ?)')
            ->execute([RB_ADMIN_USER, password_hash(RB_ADMIN_PASS, PASSWORD_DEFAULT), now()]);
    }
}

/** Size of the SQLite file in bytes (0 when it does not exist yet). */
function db_size_bytes(): int
{
    clearstatcache(true, RB_DB_PATH);
    return is_file(RB_DB_PATH) ? (int) filesize(RB_DB_PATH) : 0;
}
