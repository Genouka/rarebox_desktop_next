<?php
/**
 * HTTP helpers: JSON responses, input parsing, redirects, CORS, caching.
 */

declare(strict_types=1);

namespace RareBox;

function method(): string
{
    return strtoupper($_SERVER['REQUEST_METHOD'] ?? 'GET');
}

function request_path(): string
{
    $path = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?: '/';
    return '/' . ltrim($path, '/');
}

function is_api_request(): bool
{
    return str_starts_with(request_path(), '/api/');
}

function cors_headers(): void
{
    // The public API is read-only and anonymous, so it is safe to open up.
    // Admin responses are same-origin only and get no CORS headers at all.
    if (!is_api_request()) {
        return;
    }
    header('Access-Control-Allow-Origin: *');
    header('Access-Control-Allow-Methods: GET, HEAD, OPTIONS');
    header('Access-Control-Allow-Headers: Content-Type, If-None-Match');
    header('Access-Control-Max-Age: 86400');
    header('Vary: Origin');
}

/**
 * Emit a JSON response.
 *
 * @param array<string,string> $headers Extra headers (name => value).
 */
function json_response(int $code, $data, array $headers = []): void
{
    http_response_code($code);
    header('Content-Type: application/json; charset=utf-8');
    header('X-Content-Type-Options: nosniff');
    if (!isset($headers['Cache-Control'])) {
        header('Cache-Control: no-store');
    }
    foreach ($headers as $name => $value) {
        header($name . ': ' . $value);
    }
    cors_headers();
    echo json_encode($data, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE);
    exit;
}

/**
 * Emit a cacheable JSON response with a strong ETag.
 *
 * Browsers and the static site send `If-None-Match` back on the next call, so a
 * repeat visitor costs a 304 with no body instead of a full payload.
 */
function json_cache(int $code, $data, int $maxAge): void
{
    $body = json_encode($data, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE);
    $etag = '"' . sha1((string) $body) . '"';

    header('ETag: ' . $etag);
    $sent = trim((string) ($_SERVER['HTTP_IF_NONE_MATCH'] ?? ''));
    if ($sent !== '' && ($sent === $etag || $sent === 'W/' . $etag || str_contains($sent, $etag))) {
        http_response_code(304);
        header('Cache-Control: public, max-age=' . $maxAge);
        header('X-Content-Type-Options: nosniff');
        cors_headers();
        exit;
    }

    http_response_code($code);
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: public, max-age=' . $maxAge);
    header('X-Content-Type-Options: nosniff');
    cors_headers();
    echo $body;
    exit;
}

function redirect(string $url, int $code = 302): void
{
    header('Location: ' . $url, true, $code);
    exit;
}

function read_json_input(): array
{
    static $decoded = null;
    if ($decoded !== null) {
        return $decoded;
    }
    $raw = file_get_contents('php://input');
    if ($raw === '' || $raw === false) {
        return $decoded = [];
    }
    $dec = json_decode($raw, true);
    return $decoded = (is_array($dec) ? $dec : []);
}

function get_param(string $key, $default = null)
{
    if (isset($_GET[$key]) && $_GET[$key] !== '') {
        return $_GET[$key];
    }
    if (isset($_POST[$key]) && $_POST[$key] !== '') {
        return $_POST[$key];
    }
    $json = read_json_input();
    if (isset($json[$key]) && $json[$key] !== '') {
        return $json[$key];
    }
    return $default;
}

function has_param(string $key): bool
{
    if (array_key_exists($key, $_GET) || array_key_exists($key, $_POST)) {
        return true;
    }
    $json = read_json_input();
    return array_key_exists($key, $json);
}

function client_ip(): string
{
    foreach (['HTTP_X_FORWARDED_FOR', 'HTTP_CLIENT_IP', 'REMOTE_ADDR'] as $key) {
        if (!empty($_SERVER[$key])) {
            $v = trim(explode(',', (string) $_SERVER[$key])[0]);
            if ($v !== '') {
                return $v;
            }
        }
    }
    return 'unknown';
}

function user_agent(): string
{
    return substr((string) ($_SERVER['HTTP_USER_AGENT'] ?? ''), 0, 300);
}

/**
 * Validate a release/version string: digits and dots, at least one dot.
 * Suffixes such as `-beta.1` are accepted because the comparator strips them.
 */
function is_valid_version(string $v): bool
{
    return $v !== '' && strlen($v) <= 40 && preg_match('/^\d+(\.\d+){0,3}([-+][0-9A-Za-z.\-]{1,20})?$/', $v) === 1;
}

function is_valid_http_url(string $url): bool
{
    if ($url === '' || strlen($url) > 2048) {
        return false;
    }
    $parts = parse_url($url);
    return is_array($parts)
        && in_array(strtolower($parts['scheme'] ?? ''), ['http', 'https'], true)
        && !empty($parts['host']);
}

/** Accept `YYYY-MM-DD HH:MM:SS`, `YYYY-MM-DD HH:MM` or `YYYY-MM-DD`; return canonical or null. */
function normalize_datetime(string $value): ?string
{
    $value = trim($value);
    if ($value === '') {
        return null;
    }
    $formats = ['Y-m-d H:i:s', 'Y-m-d H:i', 'Y-m-d\TH:i', 'Y-m-d\TH:i:s', 'Y-m-d'];
    foreach ($formats as $fmt) {
        $dt = \DateTimeImmutable::createFromFormat($fmt, $value);
        if ($dt instanceof \DateTimeImmutable) {
            return $dt->format('Y-m-d H:i:s');
        }
    }
    return null;
}

function now(): string
{
    return date('Y-m-d H:i:s');
}

function absolute_url(string $path): string
{
    return rtrim(RB_API_URL, '/') . '/' . ltrim($path, '/');
}
