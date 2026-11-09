<?php
/**
 * RareBox Desktop — API & Admin backend configuration.
 *
 * Deploy at: rareboxnextapi.qiumingsanyu.top
 * Requires: PHP 8.0+ with PDO_SQLite and sqlite3.
 *
 * Local overrides: create `config.local.php` next to this file returning an
 * array of the constants below you want to replace, e.g.
 *
 *     <?php return ['RB_FORCE_HTTPS' => false, 'RB_ADMIN_PASS' => 'dev-pass'];
 *
 * That file is git-ignored and never shipped.
 */

declare(strict_types=1);

$defaults = [
    // Application display name (used in admin UI only).
    'RB_APP_NAME' => 'RareBox Desktop',

    // Canonical public URLs. Used for absolute links in downloads/backups.
    'RB_SITE_URL' => 'https://rare.genouka.top',
    'RB_API_URL' => 'https://rareboxnextapi.qiumingsanyu.top',
    'RB_GITHUB_URL' => 'https://github.com/qiumingsanyu/rarebox_desktop',

    // Local time used for every stored timestamp and every "now" comparison.
    'RB_TIMEZONE' => 'Asia/Shanghai',

    // SQLite database file. The `data/` directory must be writable by the
    // web server (php-fpm/fastcgi) user. It is git-ignored.
    'RB_DB_PATH' => __DIR__ . '/data/app.sqlite',

    // Default admin account, seeded on first run when the admins table is empty.
    // CHANGE THIS IMMEDIATELY after first login in Admin → Settings.
    'RB_ADMIN_USER' => 'admin',
    'RB_ADMIN_PASS' => 'change-me-please',

    // Session cookie name for the admin backend.
    'RB_SESSION_NAME' => 'rarebox_admin',

    // When true, session cookies are marked Secure (send cookies only over
    // HTTPS). Keep true in production; config.local.php can disable it for
    // plain-HTTP development.
    'RB_FORCE_HTTPS' => true,

    // Allowed enums (validated on write).
    'RB_ALLOWED_PLATFORMS' => ['windows', 'macos', 'linux', 'android', 'all'],
    'RB_ALLOWED_CHANNELS' => ['stable', 'beta', 'nightly'],
    'RB_ALLOWED_LEVELS' => ['info', 'warning', 'critical'],

    // Public API caching (seconds). `/api/update/check` is per-client and is
    // never cached; these cover the shared reads.
    'RB_CACHE_VERSION' => 60,
    'RB_CACHE_ANNOUNCEMENTS' => 60,
    'RB_CACHE_RELEASES' => 60,
    'RB_CACHE_STATS' => 300,

    // Download logging: ignore repeat hits from the same IP for the same
    // release inside this window, so browser retries and download managers
    // cannot inflate the counters.
    'RB_DOWNLOAD_DEDUPE_SECONDS' => 120,

    // Admin login throttling: at most N failed attempts per IP+username inside
    // the window before the login form stops checking the password.
    'RB_LOGIN_MAX_ATTEMPTS' => 8,
    'RB_LOGIN_WINDOW_MINUTES' => 15,

    // Rows per page in the admin lists.
    'RB_PAGE_SIZE' => 25,
];

$localFile = __DIR__ . '/config.local.php';
$overrides = is_file($localFile) ? (static function (string $file): array {
    $value = require $file;
    return is_array($value) ? $value : [];
})($localFile) : [];

foreach ($overrides + $defaults as $name => $value) {
    if (!defined($name)) {
        define($name, $value);
    }
}
unset($defaults, $overrides, $localFile);
