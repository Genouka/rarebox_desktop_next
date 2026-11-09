<?php
/**
 * Front controller for PHP's built-in server (local development only).
 *
 *     php -S 127.0.0.1:8080 router.php
 *
 * Static files are served as-is; every other path goes through index.php, the
 * same way nginx `try_files` behaves. Production nginx never uses this file.
 */

declare(strict_types=1);

$path = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?: '/';
$file = realpath(__DIR__ . $path);

if ($path !== '/'
    && $file !== false
    && is_file($file)
    && str_starts_with($file, __DIR__)
    && !str_ends_with($file, '.php')
) {
    return false;
}

require __DIR__ . '/index.php';
