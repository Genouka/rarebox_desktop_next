<?php
/**
 * Admin handlers: releases CRUD, announcements CRUD, stats, exports, password.
 *
 * Every mutating handler requires an authenticated session (enforced by the
 * front controller) and a valid CSRF token.
 */

declare(strict_types=1);

namespace RareBox;

use function RareBox\db;
use function RareBox\json_response;
use function RareBox\get_param;
use function RareBox\has_param;
use function RareBox\normalize_datetime;
use function RareBox\now;
use function RareBox\is_valid_version;
use function RareBox\is_valid_http_url;

const RB_TABS = ['overview', 'releases', 'announcements', 'downloads', 'settings'];

function current_tab(): string
{
    $tab = (string) get_param('tab', 'overview');
    return in_array($tab, RB_TABS, true) ? $tab : 'overview';
}

function current_page(): int
{
    return max(1, (int) get_param('page', 1));
}

function admin_stats(): array
{
    return db()->query(<<<'SQL'
SELECT
  (SELECT COUNT(*) FROM releases) AS releases,
  (SELECT COUNT(*) FROM announcements) AS announcements,
  (SELECT COUNT(*) FROM announcements WHERE active = 1) AS active_announcements,
  (SELECT COUNT(*) FROM download_log) AS downloads,
  (SELECT COUNT(DISTINCT ip) FROM download_log) AS unique_ips,
  (SELECT COUNT(*) FROM download_log WHERE created_at >= datetime('now', '-1 day', 'localtime')) AS downloads_24h
SQL
    )->fetch();
}

/** @return array{items:array,total:int,page:int,pages:int,per:int} */
function paginate(array $items, int $total, int $page, int $per = RB_PAGE_SIZE): array
{
    return [
        'items' => $items,
        'total' => $total,
        'page' => $page,
        'per' => $per,
        'pages' => max(1, (int) ceil($total / $per)),
    ];
}

function admin_releases(string $q, string $platform, string $channel, int $page): array
{
    $pdo = db();
    $where = [];
    $args = [];
    if ($q !== '') {
        $where[] = '(version LIKE ? OR download_url LIKE ? OR notes LIKE ?)';
        $like = '%' . $q . '%';
        array_push($args, $like, $like, $like);
    }
    if ($platform !== 'all' && in_array($platform, RB_ALLOWED_PLATFORMS, true)) {
        $where[] = 'platform = ?';
        $args[] = $platform;
    }
    if ($channel !== 'all' && in_array($channel, RB_ALLOWED_CHANNELS, true)) {
        $where[] = 'channel = ?';
        $args[] = $channel;
    }
    $sqlWhere = $where === [] ? '' : ' WHERE ' . implode(' AND ', $where);

    $count = $pdo->prepare('SELECT COUNT(*) FROM releases' . $sqlWhere);
    $count->execute($args);
    $total = (int) $count->fetchColumn();

    $offset = ($page - 1) * RB_PAGE_SIZE;
    $stmt = $pdo->prepare('SELECT * FROM releases' . $sqlWhere . ' ORDER BY published_at DESC, id DESC LIMIT ? OFFSET ?');
    $i = 1;
    foreach ($args as $arg) {
        $stmt->bindValue($i++, $arg);
    }
    $stmt->bindValue($i++, RB_PAGE_SIZE, \PDO::PARAM_INT);
    $stmt->bindValue($i, $offset, \PDO::PARAM_INT);
    $stmt->execute();

    return paginate($stmt->fetchAll(), $total, $page);
}

function admin_announcements(string $q, string $active, int $page): array
{
    $pdo = db();
    $where = [];
    $args = [];
    if ($q !== '') {
        $where[] = '(title LIKE ? OR body LIKE ?)';
        $like = '%' . $q . '%';
        array_push($args, $like, $like);
    }
    if ($active === '1' || $active === '0') {
        $where[] = 'active = ?';
        $args[] = (int) $active;
    }
    $sqlWhere = $where === [] ? '' : ' WHERE ' . implode(' AND ', $where);

    $count = $pdo->prepare('SELECT COUNT(*) FROM announcements' . $sqlWhere);
    $count->execute($args);
    $total = (int) $count->fetchColumn();

    $offset = ($page - 1) * RB_PAGE_SIZE;
    $stmt = $pdo->prepare('SELECT * FROM announcements' . $sqlWhere . ' ORDER BY id DESC LIMIT ? OFFSET ?');
    $i = 1;
    foreach ($args as $arg) {
        $stmt->bindValue($i++, $arg);
    }
    $stmt->bindValue($i++, RB_PAGE_SIZE, \PDO::PARAM_INT);
    $stmt->bindValue($i, $offset, \PDO::PARAM_INT);
    $stmt->execute();

    return paginate($stmt->fetchAll(), $total, $page);
}

function admin_downloads(string $q, string $platform, int $days, int $page): array
{
    $pdo = db();
    $where = ['created_at >= ?'];
    $args = [date('Y-m-d H:i:s', time() - max(1, $days) * 86400)];
    if ($q !== '') {
        $where[] = '(version LIKE ? OR ip LIKE ? OR user_agent LIKE ?)';
        $like = '%' . $q . '%';
        array_push($args, $like, $like, $like);
    }
    if ($platform !== 'all' && in_array($platform, RB_ALLOWED_PLATFORMS, true)) {
        $where[] = 'platform = ?';
        $args[] = $platform;
    }
    $sqlWhere = ' WHERE ' . implode(' AND ', $where);

    $count = $pdo->prepare('SELECT COUNT(*) FROM download_log' . $sqlWhere);
    $count->execute($args);
    $total = (int) $count->fetchColumn();

    $offset = ($page - 1) * RB_PAGE_SIZE;
    $stmt = $pdo->prepare('SELECT version, platform, channel, ip, user_agent, created_at FROM download_log' . $sqlWhere . ' ORDER BY id DESC LIMIT ? OFFSET ?');
    $i = 1;
    foreach ($args as $arg) {
        $stmt->bindValue($i++, $arg);
    }
    $stmt->bindValue($i++, RB_PAGE_SIZE, \PDO::PARAM_INT);
    $stmt->bindValue($i, $offset, \PDO::PARAM_INT);
    $stmt->execute();

    return paginate($stmt->fetchAll(), $total, $page);
}

function admin_top_versions(): array
{
    return db()->query(<<<'SQL'
SELECT version, platform, COUNT(*) AS downloads FROM download_log
GROUP BY version, platform ORDER BY downloads DESC LIMIT 8
SQL
    )->fetchAll();
}

/** Downloads per day for the last `$days` days, gap-filled. */
function admin_download_series(int $days = 14): array
{
    $pdo = db();
    $from = date('Y-m-d', time() - ($days - 1) * 86400);
    $stmt = $pdo->prepare(<<<'SQL'
SELECT substr(created_at, 1, 10) AS day, COUNT(*) AS downloads
FROM download_log WHERE created_at >= ? GROUP BY day
SQL
    );
    $stmt->execute([$from . ' 00:00:00']);
    $counts = [];
    foreach ($stmt->fetchAll() as $row) {
        $counts[$row['day']] = (int) $row['downloads'];
    }
    $series = [];
    for ($i = $days - 1; $i >= 0; $i--) {
        $day = date('Y-m-d', time() - $i * 86400);
        $series[] = ['day' => $day, 'downloads' => $counts[$day] ?? 0];
    }
    return $series;
}

/**
 * Everything the dashboard needs, for the tab currently being viewed.
 */
function admin_dashboard_data(): array
{
    $tab = current_tab();
    $data = [
        'tab' => $tab,
        'stats' => admin_stats(),
        'top' => admin_top_versions(),
        'series' => admin_download_series(14),
        'releases' => paginate([], 0, 1),
        'announcements' => paginate([], 0, 1),
        'downloads' => paginate([], 0, 1),
        'q' => (string) get_param('q', ''),
        'platform' => (string) get_param('platform', 'all'),
        'channel' => (string) get_param('channel', 'all'),
        'active' => (string) get_param('active', ''),
        'days' => max(1, min(365, (int) get_param('days', 30))),
        'db_size' => db_size_bytes(),
        'last_release' => null,
    ];
    $stmt = db()->prepare(
        'SELECT * FROM releases WHERE (published_at IS NULL OR published_at <= ?) ORDER BY published_at DESC, id DESC LIMIT 1'
    );
    $stmt->execute([now()]);
    $data['last_release'] = $stmt->fetch() ?: null;

    $hashStmt = db()->prepare('SELECT password_hash FROM admins WHERE id = ?');
    $hashStmt->execute([(int) ($_SESSION['admin_id'] ?? 0)]);
    $hash = $hashStmt->fetchColumn();
    $data['using_default_password'] = $hash ? password_verify(RB_ADMIN_PASS, (string) $hash) : false;

    switch ($tab) {
        case 'releases':
            $data['releases'] = admin_releases($data['q'], $data['platform'], $data['channel'], current_page());
            break;
        case 'announcements':
            $data['announcements'] = admin_announcements($data['q'], $data['active'], current_page());
            break;
        case 'downloads':
            $data['downloads'] = admin_downloads($data['q'], $data['platform'], $data['days'], current_page());
            break;
    }
    return $data;
}

/* ---------------- Releases ---------------- */

/**
 * Validate and normalize release input.
 *
 * @param array $input Raw request values.
 * @param array|null $existing Row being updated (null when creating).
 * @return array{0:?array,1:?string} [fields, error]
 */
function validate_release(array $input, ?array $existing): array
{
    $version = trim((string) ($input['version'] ?? $existing['version'] ?? ''));
    if (!is_valid_version($version)) {
        return [null, 'Version must look like 1.2.3 (digits and dots).'];
    }
    $url = trim((string) ($input['download_url'] ?? $existing['download_url'] ?? ''));
    if (!is_valid_http_url($url)) {
        return [null, 'Download URL must be a valid http(s) URL.'];
    }
    $channel = (string) ($input['channel'] ?? $existing['channel'] ?? 'stable');
    if (!in_array($channel, RB_ALLOWED_CHANNELS, true)) {
        return [null, 'Unknown channel.'];
    }
    $platform = (string) ($input['platform'] ?? $existing['platform'] ?? 'all');
    if (!in_array($platform, RB_ALLOWED_PLATFORMS, true)) {
        return [null, 'Unknown platform.'];
    }
    $fileSize = (int) ($input['file_size'] ?? $existing['file_size'] ?? 0);
    if ($fileSize < 0) {
        return [null, 'File size cannot be negative.'];
    }
    $minVersion = trim((string) ($input['min_version'] ?? $existing['min_version'] ?? ''));
    if ($minVersion !== '' && !is_valid_version($minVersion)) {
        return [null, 'Minimum version must look like 1.2.3, or be empty.'];
    }
    $publishedRaw = (string) ($input['published_at'] ?? $existing['published_at'] ?? '');
    $published = $publishedRaw === '' ? now() : normalize_datetime($publishedRaw);
    if ($published === null) {
        return [null, 'Publish time must be YYYY-MM-DD HH:MM[:SS].'];
    }

    return [[
        'version' => $version,
        'channel' => $channel,
        'platform' => $platform,
        'download_url' => $url,
        'file_size' => $fileSize,
        'notes' => trim((string) ($input['notes'] ?? $existing['notes'] ?? '')),
        'min_version' => $minVersion === '' ? null : $minVersion,
        'force_update' => !empty($input['force_update']) ? 1 : 0,
        'published_at' => $published,
    ], null];
}

function admin_release_create(): void
{
    [$fields, $error] = validate_release($_POST + read_json_input(), null);
    if ($fields === null) {
        json_response(422, ['error' => 'validation', 'message' => $error]);
    }
    $pdo = db();
    $pdo->prepare(<<<'SQL'
INSERT INTO releases (version, channel, platform, download_url, file_size, notes, min_version, force_update, published_at)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
SQL
    )->execute(array_values($fields));

    json_response(201, ['ok' => true, 'id' => (int) $pdo->lastInsertId(), 'message' => 'Release added.']);
}

function admin_release_update(): void
{
    $pdo = db();
    $id = (int) get_param('id', 0);
    $stmt = $pdo->prepare('SELECT * FROM releases WHERE id = ?');
    $stmt->execute([$id]);
    $row = $stmt->fetch();
    if (!$row) {
        json_response(404, ['error' => 'not_found', 'message' => 'Release not found.']);
    }
    [$fields, $error] = validate_release($_POST + read_json_input(), $row);
    if ($fields === null) {
        json_response(422, ['error' => 'validation', 'message' => $error]);
    }
    $pdo->prepare(<<<'SQL'
UPDATE releases SET version = ?, channel = ?, platform = ?, download_url = ?, file_size = ?,
       notes = ?, min_version = ?, force_update = ?, published_at = ?
WHERE id = ?
SQL
    )->execute([...array_values($fields), $id]);

    json_response(200, ['ok' => true, 'message' => 'Release updated.']);
}

function admin_release_delete(): void
{
    $id = (int) get_param('id', 0);
    $stmt = db()->prepare('DELETE FROM releases WHERE id = ?');
    $stmt->execute([$id]);
    if ($stmt->rowCount() === 0) {
        json_response(404, ['error' => 'not_found']);
    }
    json_response(200, ['ok' => true, 'message' => 'Release deleted.']);
}

/* ---------------- Announcements ---------------- */

/** @return array{0:?array,1:?string} */
function validate_announcement(array $input, ?array $existing): array
{
    $title = trim((string) ($input['title'] ?? $existing['title'] ?? ''));
    if ($title === '' || mb_strlen($title) > 200) {
        return [null, 'Title is required (max 200 characters).'];
    }
    $level = (string) ($input['level'] ?? $existing['level'] ?? 'info');
    if (!in_array($level, RB_ALLOWED_LEVELS, true)) {
        return [null, 'Unknown level.'];
    }
    $starts = normalize_datetime((string) ($input['starts_at'] ?? $existing['starts_at'] ?? ''));
    $ends = normalize_datetime((string) ($input['ends_at'] ?? $existing['ends_at'] ?? ''));
    if ($starts !== null && $ends !== null && strcmp($starts, $ends) > 0) {
        return [null, 'End time must be after the start time.'];
    }
    return [[
        'title' => $title,
        'body' => trim((string) ($input['body'] ?? $existing['body'] ?? '')),
        'level' => $level,
        'active' => array_key_exists('active', $input)
            ? (!empty($input['active']) ? 1 : 0)
            : (int) ($existing['active'] ?? 1),
        'starts_at' => $starts,
        'ends_at' => $ends,
    ], null];
}

function admin_announcement_create(): void
{
    [$fields, $error] = validate_announcement($_POST + read_json_input(), null);
    if ($fields === null) {
        json_response(422, ['error' => 'validation', 'message' => $error]);
    }
    $pdo = db();
    $pdo->prepare(<<<'SQL'
INSERT INTO announcements (title, body, level, active, starts_at, ends_at, created_at, updated_at)
VALUES (?, ?, ?, ?, ?, ?, ?, ?)
SQL
    )->execute([...array_values($fields), now(), now()]);

    json_response(201, ['ok' => true, 'id' => (int) $pdo->lastInsertId(), 'message' => 'Announcement published.']);
}

function admin_announcement_update(): void
{
    $pdo = db();
    $id = (int) get_param('id', 0);
    $stmt = $pdo->prepare('SELECT * FROM announcements WHERE id = ?');
    $stmt->execute([$id]);
    $row = $stmt->fetch();
    if (!$row) {
        json_response(404, ['error' => 'not_found', 'message' => 'Announcement not found.']);
    }
    [$fields, $error] = validate_announcement($_POST + read_json_input(), $row);
    if ($fields === null) {
        json_response(422, ['error' => 'validation', 'message' => $error]);
    }
    $pdo->prepare(<<<'SQL'
UPDATE announcements SET title = ?, body = ?, level = ?, active = ?, starts_at = ?, ends_at = ?, updated_at = ?
WHERE id = ?
SQL
    )->execute([...array_values($fields), now(), $id]);

    json_response(200, ['ok' => true, 'message' => 'Announcement updated.']);
}

function admin_announcement_delete(): void
{
    $id = (int) get_param('id', 0);
    $stmt = db()->prepare('DELETE FROM announcements WHERE id = ?');
    $stmt->execute([$id]);
    if ($stmt->rowCount() === 0) {
        json_response(404, ['error' => 'not_found']);
    }
    json_response(200, ['ok' => true, 'message' => 'Announcement deleted.']);
}

/* ---------------- Account ---------------- */

function admin_change_password(): void
{
    $cur = (string) get_param('current_password', '');
    $new = (string) get_param('new_password', '');
    $confirm = (string) get_param('confirm_password', $new);

    if (strlen($new) < 8) {
        json_response(422, ['error' => 'validation', 'message' => 'New password must be at least 8 characters.']);
    }
    if ($new !== $confirm) {
        json_response(422, ['error' => 'validation', 'message' => 'The two new passwords do not match.']);
    }
    if ($new === $cur) {
        json_response(422, ['error' => 'validation', 'message' => 'New password must differ from the current one.']);
    }

    $pdo = db();
    $stmt = $pdo->prepare('SELECT password_hash FROM admins WHERE id = ?');
    $stmt->execute([$_SESSION['admin_id']]);
    $row = $stmt->fetch();
    if (!$row || !password_verify($cur, $row['password_hash'])) {
        json_response(403, ['error' => 'bad_current_password', 'message' => 'Current password is incorrect.']);
    }
    $pdo->prepare('UPDATE admins SET password_hash = ? WHERE id = ?')
        ->execute([password_hash($new, PASSWORD_DEFAULT), $_SESSION['admin_id']]);

    session_regenerate_id(true);
    unset($_SESSION['csrf']);
    json_response(200, ['ok' => true, 'message' => 'Password updated.']);
}

/* ---------------- Exports ---------------- */

function admin_export_downloads(): void
{
    $days = max(1, min(3650, (int) get_param('days', 365)));
    $platform = (string) get_param('platform', 'all');

    $where = ['created_at >= ?'];
    $args = [date('Y-m-d H:i:s', time() - $days * 86400)];
    if ($platform !== 'all' && in_array($platform, RB_ALLOWED_PLATFORMS, true)) {
        $where[] = 'platform = ?';
        $args[] = $platform;
    }
    $stmt = db()->prepare(
        'SELECT created_at, version, platform, channel, ip, user_agent FROM download_log WHERE '
        . implode(' AND ', $where) . ' ORDER BY id DESC LIMIT 50000'
    );
    $stmt->execute($args);

    $filename = 'rarebox-downloads-' . date('Ymd-His') . '.csv';
    header('Content-Type: text/csv; charset=utf-8');
    header('Content-Disposition: attachment; filename="' . $filename . '"');
    header('Cache-Control: no-store');
    header('X-Content-Type-Options: nosniff');

    $out = fopen('php://output', 'w');
    // BOM so Excel opens UTF-8 (user agents contain non-ASCII) correctly.
    fwrite($out, "\xEF\xBB\xBF");
    fputcsv($out, ['created_at', 'version', 'platform', 'channel', 'ip', 'user_agent'], ',', '"', '');
    while ($row = $stmt->fetch()) {
        $row = array_map(static fn ($v) => is_scalar($v) ? (string) $v : '', $row);
        fputcsv($out, array_values($row), ',', '"', '');
    }
    fclose($out);
    exit;
}

function admin_backup(): void
{
    $tmp = sys_get_temp_dir() . '/rarebox-backup-' . bin2hex(random_bytes(6)) . '.sqlite';
    db()->exec('VACUUM INTO ' . db()->quote($tmp));

    header('Content-Type: application/vnd.sqlite3');
    header('Content-Disposition: attachment; filename="rarebox-' . date('Ymd-His') . '.sqlite"');
    header('Content-Length: ' . (string) filesize($tmp));
    header('Cache-Control: no-store');
    readfile($tmp);
    @unlink($tmp);
    exit;
}
