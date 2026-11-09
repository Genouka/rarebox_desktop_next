<?php
/**
 * Admin session, login throttling, and CSRF protection.
 */

declare(strict_types=1);

namespace RareBox;

use function RareBox\db;

/**
 * Start the admin session on demand.
 *
 * The public API never opens a session: no cookie, no `session_start` lock and
 * no Set-Cookie header for anonymous update checks.
 */
function ensure_session(): void
{
    if (session_status() === PHP_SESSION_ACTIVE) {
        return;
    }
    ini_set('session.use_strict_mode', '1');
    ini_set('session.use_only_cookies', '1');
    ini_set('session.cookie_httponly', '1');
    session_name(RB_SESSION_NAME);
    session_set_cookie_params([
        'lifetime' => 0,
        'path' => '/',
        'domain' => '',
        'secure' => (bool) RB_FORCE_HTTPS,
        'httponly' => true,
        'samesite' => 'Lax',
    ]);
    session_start();
}

function is_logged_in(): bool
{
    ensure_session();
    return !empty($_SESSION['admin_id']);
}

function admin_username(): string
{
    ensure_session();
    return (string) ($_SESSION['admin_user'] ?? 'admin');
}

/* ---------------- Login throttling ---------------- */

function failed_attempts(string $ip, string $username): int
{
    $stmt = db()->prepare(
        'SELECT COUNT(*) FROM login_attempts WHERE ip = ? AND username = ? AND created_at >= ?'
    );
    $stmt->execute([$ip, $username, date('Y-m-d H:i:s', time() - RB_LOGIN_WINDOW_MINUTES * 60)]);
    return (int) $stmt->fetchColumn();
}

function record_failed_attempt(string $ip, string $username): void
{
    $pdo = db();
    $pdo->prepare('INSERT INTO login_attempts (ip, username, created_at) VALUES (?, ?, ?)')
        ->execute([$ip, $username, now()]);
    // Keep the table small; the window is the only thing that matters.
    $pdo->prepare('DELETE FROM login_attempts WHERE created_at < ?')
        ->execute([date('Y-m-d H:i:s', time() - 86400 * 7)]);
}

function clear_failed_attempts(string $ip, string $username): void
{
    db()->prepare('DELETE FROM login_attempts WHERE ip = ? AND username = ?')->execute([$ip, $username]);
}

/**
 * Check credentials.
 *
 * @return array{ok:bool,error:?string,retry_after:?int}
 */
function attempt_login(string $username, string $password): array
{
    ensure_session();
    $ip = client_ip();
    $username = trim($username);

    $used = failed_attempts($ip, $username);
    if ($used >= RB_LOGIN_MAX_ATTEMPTS) {
        return [
            'ok' => false,
            'error' => sprintf('Too many failed attempts. Try again in %d minutes.', RB_LOGIN_WINDOW_MINUTES),
            'retry_after' => RB_LOGIN_WINDOW_MINUTES * 60,
        ];
    }

    $stmt = db()->prepare('SELECT id, username, password_hash FROM admins WHERE username = ?');
    $stmt->execute([$username]);
    $row = $stmt->fetch();

    // Always run a hash verification so a missing user and a wrong password
    // take the same time.
    $hash = $row['password_hash'] ?? '$2y$10$usesomesillystringfoeWQoUj8rI0XKx4FqMfgY1lYyC8Xy7lq5Yq3G';
    $valid = password_verify($password, $hash) && is_array($row);

    if (!$valid) {
        record_failed_attempt($ip, $username);
        return ['ok' => false, 'error' => 'Invalid username or password.', 'retry_after' => null];
    }

    if (password_needs_rehash($row['password_hash'], PASSWORD_DEFAULT)) {
        db()->prepare('UPDATE admins SET password_hash = ? WHERE id = ?')
            ->execute([password_hash($password, PASSWORD_DEFAULT), $row['id']]);
    }

    clear_failed_attempts($ip, $username);
    session_regenerate_id(true);
    $_SESSION['admin_id'] = (int) $row['id'];
    $_SESSION['admin_user'] = $row['username'];
    unset($_SESSION['csrf']);
    $_SESSION['last_seen'] = time();
    return ['ok' => true, 'error' => null, 'retry_after' => null];
}

function logout(): void
{
    ensure_session();
    $_SESSION = [];
    if (ini_get('session.use_cookies')) {
        $params = session_get_cookie_params();
        setcookie(session_name(), '', [
            'expires' => time() - 42000,
            'path' => $params['path'],
            'domain' => $params['domain'],
            'secure' => $params['secure'],
            'httponly' => $params['httponly'],
            'samesite' => $params['samesite'] ?? 'Lax',
        ]);
    }
    session_destroy();
}

/** Redirect anonymous visitors to the login form; JSON clients get a 401. */
function require_admin(): void
{
    if (is_logged_in()) {
        return;
    }
    if (PHP_SAPI === 'cli') {
        return;
    }
    $wantsJson = str_contains(strtolower((string) ($_SERVER['HTTP_ACCEPT'] ?? '')), 'application/json')
        || strtoupper($_SERVER['HTTP_X_REQUESTED_WITH'] ?? '') === 'XMLHTTPREQUEST'
        || method() === 'POST';
    if ($wantsJson) {
        json_response(401, ['error' => 'unauthorized', 'message' => 'Session expired. Sign in again.']);
    }
    redirect('/admin/login');
}

function csrf_token(): string
{
    ensure_session();
    if (empty($_SESSION['csrf'])) {
        $_SESSION['csrf'] = bin2hex(random_bytes(32));
    }
    return $_SESSION['csrf'];
}

/** Validate the CSRF token from header (X-Csrf-Token) or POST/JSON field. */
function csrf_valid(): bool
{
    ensure_session();
    $sent = $_SERVER['HTTP_X_CSRF_TOKEN'] ?? ($_POST['_csrf'] ?? get_param('_csrf', ''));
    if (!is_string($sent) || $sent === '') {
        return false;
    }
    $expected = $_SESSION['csrf'] ?? '';
    return $expected !== '' && hash_equals($expected, $sent);
}
