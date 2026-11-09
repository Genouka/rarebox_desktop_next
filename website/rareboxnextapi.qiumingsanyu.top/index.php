<?php
/**
 * Front controller for RareBox Desktop API + Admin backend.
 *
 * All requests are routed here via nginx `try_files` / rewrite. Nothing is
 * written outside `data/` (git-ignored SQLite DB). Keep this file thin.
 */

declare(strict_types=1);

require_once __DIR__ . '/config.php';

date_default_timezone_set(RB_TIMEZONE);

require_once __DIR__ . '/lib/http.php';
require_once __DIR__ . '/lib/version.php';
require_once __DIR__ . '/lib/db.php';
require_once __DIR__ . '/lib/auth.php';
require_once __DIR__ . '/lib/api.php';
require_once __DIR__ . '/lib/admin.php';
require_once __DIR__ . '/lib/view.php';

use function RareBox\method;
use function RareBox\request_path;
use function RareBox\is_api_request;
use function RareBox\json_response;
use function RareBox\cors_headers;
use function RareBox\redirect;
use function RareBox\get_param;
use function RareBox\is_logged_in;
use function RareBox\require_admin;
use function RareBox\attempt_login;
use function RareBox\logout;
use function RareBox\csrf_token;
use function RareBox\csrf_valid;
use function RareBox\api_health;
use function RareBox\api_version;
use function RareBox\api_update_check;
use function RareBox\api_announcements;
use function RareBox\api_releases;
use function RareBox\api_stats;
use function RareBox\api_download;
use function RareBox\admin_dashboard_data;
use function RareBox\admin_release_create;
use function RareBox\admin_release_update;
use function RareBox\admin_release_delete;
use function RareBox\admin_announcement_create;
use function RareBox\admin_announcement_update;
use function RareBox\admin_announcement_delete;
use function RareBox\admin_change_password;
use function RareBox\admin_export_downloads;
use function RareBox\admin_backup;
use function RareBox\client_ip;
use function RareBox\admin_username;
use function RareBox\ensure_session;

/** Plain-text/HTML security headers for anything that is not the JSON API. */
function page_security_headers(): void
{
    header('X-Content-Type-Options: nosniff');
    header('X-Frame-Options: DENY');
    header('Referrer-Policy: no-referrer');
    header('X-Robots-Tag: noindex, nofollow');
    header("Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'");
}

set_exception_handler(static function (\Throwable $e): void {
    error_log('[rarebox-api] ' . $e::class . ': ' . $e->getMessage() . ' @ ' . $e->getFile() . ':' . $e->getLine());
    if (!headers_sent()) {
        if (is_api_request()) {
            json_response(500, ['error' => 'server_error', 'message' => 'Unexpected server error.']);
        }
        http_response_code(500);
        header('Content-Type: text/html; charset=utf-8');
        page_security_headers();
        echo '<!DOCTYPE html><meta charset="utf-8"><title>500</title>'
            . '<p style="font:15px system-ui;padding:40px">Unexpected server error.</p>';
    }
    exit;
});

header_remove('X-Powered-By');

cors_headers();

if (method() === 'OPTIONS') {
    http_response_code(204);
    header('Allow: GET, HEAD, POST, OPTIONS');
    exit;
}

$path = request_path();
$segments = explode('/', trim($path, '/'));
$top = $segments[0] ?? '';

/* ---------------- Public API ---------------- */

if ($top === 'api') {
    $ep = $segments[1] ?? '';
    switch ($ep) {
        case 'health':
            api_health();
            break;
        case 'version':
            api_version();
            break;
        case 'announcements':
            api_announcements();
            break;
        case 'releases':
            api_releases();
            break;
        case 'stats':
            api_stats();
            break;
        case 'download':
            api_download();
            break;
        case 'update':
            if (($segments[2] ?? '') === 'check') {
                api_update_check();
            }
            json_response(404, ['error' => 'unknown_endpoint']);
            break;
        default:
            json_response(404, [
                'error' => 'unknown_endpoint',
                'endpoints' => [
                    '/api/health',
                    '/api/version',
                    '/api/update/check',
                    '/api/announcements',
                    '/api/releases',
                    '/api/stats',
                    '/api/download',
                ],
            ]);
    }
}

/* ---------------- Admin: auth ---------------- */

if ($path === '/admin/login') {
    ensure_session();
    if (is_logged_in()) {
        redirect('/admin');
    }
    $error = null;
    if (method() === 'POST') {
        if (!csrf_valid()) {
            $error = 'Your session expired. Reload the page and try again.';
        } else {
            $result = attempt_login(
                (string) get_param('username', ''),
                (string) get_param('password', '')
            );
            if ($result['ok']) {
                redirect('/admin');
            }
            $error = $result['error'];
            if ($result['retry_after'] !== null) {
                header('Retry-After: ' . $result['retry_after']);
            }
        }
    }
    page_security_headers();
    require __DIR__ . '/admin/login.php';
    exit;
}

if ($path === '/admin/logout') {
    logout();
    redirect('/admin/login');
}

/* ---------------- Admin: protected area ---------------- */

if ($top === 'admin') {
    require_admin();

    if ($path === '/admin/export') {
        admin_export_downloads();
    }
    if ($path === '/admin/backup') {
        admin_backup();
    }

    if (method() === 'POST') {
        if (!csrf_valid()) {
            json_response(403, ['error' => 'csrf', 'message' => 'Security token expired. Reload the page.']);
        }
        $action = (string) get_param('action', '');
        switch ($action) {
            case 'release_create':
                admin_release_create();
                break;
            case 'release_update':
                admin_release_update();
                break;
            case 'release_delete':
                admin_release_delete();
                break;
            case 'announcement_create':
                admin_announcement_create();
                break;
            case 'announcement_update':
                admin_announcement_update();
                break;
            case 'announcement_delete':
                admin_announcement_delete();
                break;
            case 'change_password':
                admin_change_password();
                break;
            default:
                json_response(400, ['error' => 'unknown_action']);
        }
    }

    if ($path !== '/admin' && $path !== '/admin/') {
        json_response(404, ['error' => 'not_found']);
    }

    $data = admin_dashboard_data();
    $csrf = csrf_token();
    $admin_user = admin_username();
    $client_ip = client_ip();
    page_security_headers();
    require __DIR__ . '/admin/dashboard.php';
    exit;
}

/* ---------------- Everything else ---------------- */

if (is_api_request()) {
    json_response(404, ['error' => 'not_found']);
}

redirect(RB_SITE_URL, 302);
