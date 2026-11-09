<?php
/**
 * Public API handlers: health, version, update/check, announcements, releases,
 * stats, download.
 *
 * @api
 *   GET /api/health
 *   GET /api/version?platform=windows&channel=stable
 *   GET /api/update/check?version=0.1.0&platform=windows&channel=stable
 *   GET /api/announcements[?scope=history]
 *   GET /api/releases?platform=windows&channel=stable&limit=20
 *   GET /api/stats
 *   GET /api/download?platform=windows&channel=stable   (302 redirect + log)
 */

declare(strict_types=1);

namespace RareBox;

use function RareBox\db;
use function RareBox\json_cache;
use function RareBox\json_response;
use function RareBox\get_param;
use function RareBox\client_ip;
use function RareBox\user_agent;
use function RareBox\version_compare_semver;
use function RareBox\redirect;
use function RareBox\now;

/**
 * Newest published release for a platform/channel.
 *
 * Selection is by **version**, not by publish time: a hotfix for an old
 * series must not outrank a newer major release just because it was uploaded
 * later. Ties prefer the exact platform row, then the requested channel, then
 * the most recent row.
 *
 * Channel falls back to `stable` only when a non-stable channel is requested,
 * so a stable client is never offered a beta build. Platform falls back to the
 * wildcard `all` row; `platform = 'all'` means "newest of any platform". Rows
 * whose `published_at` lies in the future are scheduled and therefore skipped.
 */
function latest_release(string $platform, string $channel): ?array
{
    $pdo = db();
    $channels = $channel === 'stable' ? ['stable'] : [$channel, 'stable'];
    $placeholders = implode(', ', array_fill(0, count($channels), '?'));

    $sql = "SELECT * FROM releases WHERE channel IN ($placeholders)";
    $args = $channels;
    if ($platform !== 'all') {
        $sql .= " AND (platform = ? OR platform = 'all')";
        $args[] = $platform;
    }
    $sql .= ' AND (published_at IS NULL OR published_at <= ?)';
    $args[] = now();

    $stmt = $pdo->prepare($sql);
    $stmt->execute($args);

    $best = null;
    foreach ($stmt->fetchAll() as $row) {
        if ($best === null) {
            $best = $row;
            continue;
        }
        $cmp = version_compare_semver((string) $row['version'], (string) $best['version']);
        if ($cmp > 0) {
            $best = $row;
            continue;
        }
        if ($cmp < 0) {
            continue;
        }

        // Same version: exact platform wins, then the requested channel.
        $rowExact = $platform !== 'all' && $row['platform'] === $platform;
        $bestExact = $platform !== 'all' && $best['platform'] === $platform;
        if ($rowExact !== $bestExact) {
            if ($rowExact) {
                $best = $row;
            }
            continue;
        }
        $rowChannel = $row['channel'] === $channel;
        $bestChannel = $best['channel'] === $channel;
        if ($rowChannel !== $bestChannel) {
            if ($rowChannel) {
                $best = $row;
            }
            continue;
        }
        $rowPublished = (string) ($row['published_at'] ?? '');
        $bestPublished = (string) ($best['published_at'] ?? '');
        if ($rowPublished > $bestPublished
            || ($rowPublished === $bestPublished && (int) $row['id'] > (int) $best['id'])) {
            $best = $row;
        }
    }
    return $best;
}

function empty_update_payload(string $current): array
{
    return [
        'update_available' => false,
        'current_version' => $current,
        'latest_version' => null,
        'reason' => 'no_release',
        'force_update' => false,
        'required_min_version' => null,
        'reasons' => [],
        'announcements' => active_announcements(),
    ];
}

function announcement_payload(array $r, bool $with_window = false): array
{
    $payload = [
        'id' => (int) $r['id'],
        'title' => $r['title'],
        'body' => $r['body'],
        'level' => $r['level'],
        'created_at' => $r['created_at'] ?? null,
    ];
    if ($with_window) {
        $payload['starts_at'] = $r['starts_at'] ?? null;
        $payload['ends_at'] = $r['ends_at'] ?? null;
    }
    return $payload;
}

function active_announcements(): array
{
    $pdo = db();
    $stmt = $pdo->prepare(<<<'SQL'
SELECT id, title, body, level, created_at FROM announcements
WHERE active = 1
  AND (starts_at IS NULL OR starts_at <= ?)
  AND (ends_at IS NULL OR ends_at >= ?)
ORDER BY id DESC
SQL
    );
    $stmt->execute([now(), now()]);
    return array_map(fn ($r) => announcement_payload($r), $stmt->fetchAll());
}

/**
 * 已发布过的全部公告（含已过期），供客户端「以往公告」列表使用。
 * active = 0 的是后台草稿，不对客户端公开。
 */
function announcement_history(): array
{
    $pdo = db();
    $rows = $pdo->query(<<<'SQL'
SELECT id, title, body, level, created_at, starts_at, ends_at FROM announcements
WHERE active = 1
ORDER BY id DESC
SQL
    )->fetchAll();
    return array_map(fn ($r) => announcement_payload($r, true), $rows);
}

function release_to_version_payload(array $rel): array
{
    return [
        'version' => $rel['version'],
        'channel' => $rel['channel'],
        'platform' => $rel['platform'],
        'download_url' => $rel['download_url'],
        'file_size' => (int) $rel['file_size'],
        'notes' => $rel['notes'],
        'published_at' => $rel['published_at'],
        'force_update' => (bool) $rel['force_update'],
        'required_min_version' => $rel['min_version'],
    ];
}

function api_health(): void
{
    $dbOk = false;
    $dbError = null;
    try {
        $dbOk = (int) db()->query('SELECT 1')->fetchColumn() === 1;
    } catch (\Throwable $e) {
        $dbError = 'database_unavailable';
    }
    $code = $dbOk ? 200 : 503;
    json_response($code, [
        'status' => $dbOk ? 'ok' : 'degraded',
        'time' => date('c'),
        'php' => PHP_VERSION,
        'database' => $dbOk ? 'ok' : 'error',
        'error' => $dbError,
    ], $dbOk ? [] : ['Retry-After' => '30']);
}

function api_version(): void
{
    $platform = (string) get_param('platform', 'all');
    $channel = (string) get_param('channel', 'stable');
    if (!in_array($platform, RB_ALLOWED_PLATFORMS, true)) {
        json_response(400, ['error' => 'bad_platform', 'allowed' => RB_ALLOWED_PLATFORMS]);
    }
    if (!in_array($channel, RB_ALLOWED_CHANNELS, true)) {
        json_response(400, ['error' => 'bad_channel', 'allowed' => RB_ALLOWED_CHANNELS]);
    }

    $rel = latest_release($platform, $channel);
    if (!$rel) {
        json_response(404, ['error' => 'no_release', 'message' => 'No matching release found.']);
    }
    $payload = release_to_version_payload($rel);
    $payload['platform_requested'] = $platform;
    json_cache(200, $payload, RB_CACHE_VERSION);
}

function api_update_check(): void
{
    $current = (string) get_param('version', '0.0.0');
    $platform = (string) get_param('platform', 'all');
    $channel = (string) get_param('channel', 'stable');
    if (!in_array($platform, RB_ALLOWED_PLATFORMS, true)) {
        json_response(400, ['error' => 'bad_platform', 'allowed' => RB_ALLOWED_PLATFORMS]);
    }
    if (!in_array($channel, RB_ALLOWED_CHANNELS, true)) {
        json_response(400, ['error' => 'bad_channel', 'allowed' => RB_ALLOWED_CHANNELS]);
    }

    $rel = latest_release($platform, $channel);
    if (!$rel) {
        json_response(200, empty_update_payload($current));
    }

    $latest = (string) $rel['version'];
    $cmp = version_compare_semver($latest, $current);
    $newer = $cmp > 0;

    $min = $rel['min_version'];
    $below_min = $min !== null && $min !== '' && version_compare_semver($current, $min) < 0;

    $force = (bool) $rel['force_update'];
    $update_available = $newer || $force || $below_min;

    $reasons = [];
    if ($newer) {
        $reasons[] = 'new_version';
    }
    if ($force) {
        $reasons[] = 'forced';
    }
    if ($below_min) {
        $reasons[] = 'below_minimum';
    }

    json_response(200, [
        'update_available' => $update_available,
        'current_version' => $current,
        'latest_version' => $latest,
        'channel' => $rel['channel'],
        'platform' => $rel['platform'],
        'platform_requested' => $platform,
        'download_url' => $rel['download_url'],
        'file_size' => (int) $rel['file_size'],
        'notes' => $rel['notes'],
        'published_at' => $rel['published_at'],
        'force_update' => $force,
        'required_min_version' => $min,
        'reasons' => $reasons,
        'announcements' => active_announcements(),
    ]);
}

function api_announcements(): void
{
    $scope = (string) get_param('scope', 'active');
    $items = $scope === 'history' ? announcement_history() : active_announcements();
    json_cache(200, ['announcements' => $items], RB_CACHE_ANNOUNCEMENTS);
}

/**
 * Public changelog: published releases grouped by version.
 *
 * Rows that share a version (one per platform) collapse into a single entry so
 * the site can render "0.2.0 — Windows / macOS" instead of three duplicate rows.
 */
function api_releases(): void
{
    $platform = (string) get_param('platform', 'all');
    $channel = (string) get_param('channel', 'stable');
    $limit = (int) get_param('limit', 20);
    $limit = max(1, min(100, $limit));

    if (!in_array($platform, RB_ALLOWED_PLATFORMS, true) || !in_array($channel, RB_ALLOWED_CHANNELS, true)) {
        json_response(400, ['error' => 'bad_filter']);
    }

    $sql = 'SELECT * FROM releases WHERE (published_at IS NULL OR published_at <= ?)';
    $args = [now()];
    if ($channel !== 'all') {
        $sql .= ' AND channel = ?';
        $args[] = $channel;
    }
    if ($platform !== 'all') {
        $sql .= " AND (platform = ? OR platform = 'all')";
        $args[] = $platform;
    }
    $sql .= ' ORDER BY published_at DESC, id DESC LIMIT 500';

    $stmt = db()->prepare($sql);
    $stmt->execute($args);

    $grouped = [];
    foreach ($stmt->fetchAll() as $row) {
        $version = $row['version'];
        if (!isset($grouped[$version])) {
            $grouped[$version] = [
                'version' => $version,
                'channel' => $row['channel'],
                'notes' => $row['notes'],
                'published_at' => $row['published_at'],
                'force_update' => (bool) $row['force_update'],
                'min_version' => $row['min_version'],
                'platforms' => [],
                'download_urls' => [],
            ];
        }
        if (!in_array($row['platform'], $grouped[$version]['platforms'], true)) {
            $grouped[$version]['platforms'][] = $row['platform'];
        }
        $grouped[$version]['download_urls'][$row['platform']] = $row['download_url'];
    }

    $items = array_slice(array_values($grouped), 0, $limit);
    json_cache(200, ['releases' => $items], RB_CACHE_RELEASES);
}

function api_stats(): void
{
    $pdo = db();
    $totals = $pdo->query(<<<'SQL'
SELECT COUNT(*) AS downloads, COUNT(DISTINCT ip) AS unique_ips, MIN(created_at) AS first_at
FROM download_log
SQL
    )->fetch();

    $windows = [
        '7d' => date('Y-m-d H:i:s', time() - 7 * 86400),
        '30d' => date('Y-m-d H:i:s', time() - 30 * 86400),
    ];
    $stmt = $pdo->prepare('SELECT COUNT(*) FROM download_log WHERE created_at >= ?');

    $byPlatform = $pdo->query(<<<'SQL'
SELECT platform, COUNT(*) AS downloads FROM download_log
GROUP BY platform ORDER BY downloads DESC LIMIT 8
SQL
    )->fetchAll();

    $daily = [];
    $from = date('Y-m-d', time() - 13 * 86400);
    $stmtDaily = $pdo->prepare(<<<'SQL'
SELECT substr(created_at, 1, 10) AS day, COUNT(*) AS downloads
FROM download_log WHERE created_at >= ? GROUP BY day
SQL
    );
    $stmtDaily->execute([$from . ' 00:00:00']);
    $counts = [];
    foreach ($stmtDaily->fetchAll() as $row) {
        $counts[$row['day']] = (int) $row['downloads'];
    }
    for ($i = 13; $i >= 0; $i--) {
        $day = date('Y-m-d', time() - $i * 86400);
        $daily[] = ['day' => $day, 'downloads' => $counts[$day] ?? 0];
    }

    $stmt->execute([$windows['7d']]);
    $last7 = (int) $stmt->fetchColumn();
    $stmt->execute([$windows['30d']]);
    $last30 = (int) $stmt->fetchColumn();

    json_cache(200, [
        'downloads' => (int) $totals['downloads'],
        'unique_ips' => (int) $totals['unique_ips'],
        'first_download_at' => $totals['first_at'],
        'last_7d' => $last7,
        'last_30d' => $last30,
        'by_platform' => array_map(static fn ($r) => [
            'platform' => $r['platform'],
            'downloads' => (int) $r['downloads'],
        ], $byPlatform),
        'daily' => $daily,
        'generated_at' => date('c'),
    ], RB_CACHE_STATS);
}

function api_download(): void
{
    $platform = (string) get_param('platform', 'all');
    $channel = (string) get_param('channel', 'stable');
    if (!in_array($platform, RB_ALLOWED_PLATFORMS, true)) {
        json_response(400, ['error' => 'bad_platform', 'allowed' => RB_ALLOWED_PLATFORMS]);
    }
    if (!in_array($channel, RB_ALLOWED_CHANNELS, true)) {
        json_response(400, ['error' => 'bad_channel', 'allowed' => RB_ALLOWED_CHANNELS]);
    }

    $rel = latest_release($platform, $channel);
    if (!$rel || trim((string) $rel['download_url']) === '') {
        json_response(404, ['error' => 'no_release', 'message' => 'No matching release found.']);
    }

    $ip = client_ip();
    $pdo = db();
    $since = date('Y-m-d H:i:s', time() - RB_DOWNLOAD_DEDUPE_SECONDS);
    $dup = $pdo->prepare('SELECT 1 FROM download_log WHERE release_id = ? AND ip = ? AND created_at >= ? LIMIT 1');
    $dup->execute([$rel['id'], $ip, $since]);
    if ($dup->fetchColumn() === false) {
        $pdo->prepare(<<<'SQL'
INSERT INTO download_log (release_id, version, platform, channel, ip, user_agent, created_at)
VALUES (?, ?, ?, ?, ?, ?, ?)
SQL
        )->execute([
            $rel['id'], $rel['version'], $rel['platform'], $rel['channel'],
            $ip, user_agent(), now(),
        ]);
    }

    if (str_contains(strtolower((string) ($_SERVER['HTTP_ACCEPT'] ?? '')), 'application/json')) {
        json_response(200, [
            'version' => $rel['version'],
            'platform' => $rel['platform'],
            'channel' => $rel['channel'],
            'download_url' => $rel['download_url'],
            'file_size' => (int) $rel['file_size'],
        ]);
    }

    redirect((string) $rel['download_url'], 302);
}
