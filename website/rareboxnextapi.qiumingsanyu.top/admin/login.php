<?php
/**
 * Admin login view.
 *
 * @var string|null $error
 */

use function RareBox\e;
use function RareBox\csrf_token;
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="robots" content="noindex,nofollow">
    <title>RareBox Admin — Sign in</title>
    <link rel="stylesheet" href="/admin.css">
</head>
<body class="auth">
<main class="auth-card">
    <div class="brand-mark" aria-hidden="true">RB</div>
    <h1>RareBox Admin</h1>
    <p class="muted">Releases, announcements &amp; download stats</p>

    <?php if (!empty($error)): ?>
        <div class="alert alert-error" role="alert"><?= e($error) ?></div>
    <?php endif; ?>

    <form method="post" action="/admin/login" autocomplete="on">
        <input type="hidden" name="_csrf" value="<?= e(csrf_token()) ?>">
        <label>Username
            <input type="text" name="username" autocomplete="username" required autofocus>
        </label>
        <label>Password
            <input type="password" name="password" autocomplete="current-password" required>
        </label>
        <button type="submit" class="btn btn-primary btn-block">Sign in</button>
    </form>

    <p class="muted small">First run uses <code>admin</code> / <code>change-me-please</code>.
        Change it right after signing in — the dashboard will keep reminding you.</p>
</main>
</body>
</html>
