<?php
/**
 * Admin dashboard view.
 *
 * Tabs are server-rendered from `?tab=` so the page works without JavaScript;
 * the inline dialogs and AJAX submit are progressive enhancement.
 *
 * @var array  $data
 * @var string $csrf
 * @var string $admin_user
 * @var string $client_ip
 */

use function RareBox\e;
use function RareBox\fmt_bytes;
use function RareBox\fmt_datetime;
use function RareBox\fmt_relative;
use function RareBox\admin_url;
use function RareBox\pager;
use function RareBox\page_url;

$tab = $data['tab'];
$stats = $data['stats'];
$platforms = ['all', 'windows', 'macos', 'linux', 'android'];
$channels = ['all', 'stable', 'beta', 'nightly'];
$levels = ['info', 'warning', 'critical'];

$filters = array_filter([
    'q' => $data['q'],
    'platform' => $data['platform'] !== 'all' ? $data['platform'] : '',
    'channel' => $data['channel'] !== 'all' ? $data['channel'] : '',
    'active' => $data['active'],
    'days' => $tab === 'downloads' ? $data['days'] : '',
], static fn ($v) => $v !== '' && $v !== null);

$tabs = [
    'overview' => 'Overview',
    'releases' => 'Releases',
    'announcements' => 'Announcements',
    'downloads' => 'Downloads',
    'settings' => 'Settings',
];

/** Inline SVG bar chart of the last 14 days (attributes only — CSP-safe). */
function render_chart(array $series): string
{
    $max = 1;
    foreach ($series as $point) {
        $max = max($max, (int) $point['downloads']);
    }
    $n = max(1, count($series));
    $gap = 1.5;
    $barWidth = (100 - $gap * ($n - 1)) / $n;

    $bars = '';
    foreach ($series as $i => $point) {
        $value = (int) $point['downloads'];
        $height = $value === 0 ? 0.8 : max(2.0, ($value / $max) * 100);
        $x = $i * ($barWidth + $gap);
        $y = 100 - $height;
        $title = $point['day'] . ' · ' . $value;
        $bars .= '<rect x="' . round($x, 2) . '" y="' . round($y, 2) . '" width="' . round($barWidth, 2)
            . '" height="' . round($height, 2) . '" rx="1.2" class="bar"><title>' . e($title) . '</title></rect>';
    }
    $labels = '';
    foreach ($series as $i => $point) {
        if ($i % 2 !== 0 && $i !== $n - 1) {
            continue;
        }
        $x = $i * ($barWidth + $gap) + $barWidth / 2;
        $labels .= '<text x="' . round($x, 2) . '" y="112" class="axis">' . e(substr($point['day'], 5)) . '</text>';
    }
    return '<svg class="chart" viewBox="-1 -2 102 118" preserveAspectRatio="none" role="img" aria-label="Downloads per day">'
        . $bars . $labels . '</svg>';
}
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="robots" content="noindex,nofollow">
    <meta name="csrf" content="<?= e($csrf) ?>">
    <title>RareBox Admin — <?= e($tabs[$tab] ?? 'Dashboard') ?></title>
    <link rel="stylesheet" href="/admin.css">
</head>
<body class="dashboard">
<header class="topbar">
    <a class="brand" href="/admin"><span class="brand-mark small">RB</span> RareBox Admin</a>
    <nav class="tabs">
        <?php foreach ($tabs as $key => $label): ?>
            <a href="<?= e(admin_url($key)) ?>" class="<?= $key === $tab ? 'on' : '' ?>"><?= e($label) ?></a>
        <?php endforeach; ?>
    </nav>
    <span class="spacer"></span>
    <span class="muted small"><?= e($admin_user) ?> · <?= e($client_ip) ?></span>
    <a class="btn btn-ghost btn-sm" href="/admin/logout">Log out</a>
</header>

<?php if (!empty($data['using_default_password'])): ?>
    <div class="banner warn">
        You are still using the default password. <a href="<?= e(admin_url('settings')) ?>">Change it now</a>.
    </div>
<?php endif; ?>

<main class="wrap">
<?php if ($tab === 'overview'): ?>
    <section class="cards">
        <div class="stat">
            <div class="num"><?= (int) $stats['downloads'] ?></div>
            <div class="lbl">Downloads</div>
            <div class="sub"><?= (int) $stats['downloads_24h'] ?> in last 24h</div>
        </div>
        <div class="stat">
            <div class="num"><?= (int) $stats['unique_ips'] ?></div>
            <div class="lbl">Unique IPs</div>
            <div class="sub">all time</div>
        </div>
        <div class="stat">
            <div class="num"><?= (int) $stats['releases'] ?></div>
            <div class="lbl">Releases</div>
            <div class="sub"><?= $data['last_release'] ? 'latest v' . e($data['last_release']['version']) : 'nothing published' ?></div>
        </div>
        <div class="stat">
            <div class="num"><?= (int) $stats['active_announcements'] ?></div>
            <div class="lbl">Live announcements</div>
            <div class="sub"><?= (int) $stats['announcements'] ?> total</div>
        </div>
    </section>

    <div class="grid two">
        <section class="panel">
            <div class="panel-head">
                <h2>Downloads · last 14 days</h2>
                <a class="muted small" href="<?= e(admin_url('downloads')) ?>">Details →</a>
            </div>
            <?= render_chart($data['series']) ?>
        </section>

        <section class="panel">
            <div class="panel-head">
                <h2>Top versions</h2>
                <a class="muted small" href="/admin/export?days=365">Export CSV</a>
            </div>
            <?php if (!$data['top']): ?>
                <p class="empty">No downloads recorded yet.</p>
            <?php else: ?>
                <table class="tbl">
                    <thead><tr><th>Version</th><th>Platform</th><th class="num">Downloads</th></tr></thead>
                    <tbody>
                    <?php foreach ($data['top'] as $row): ?>
                        <tr>
                            <td><code><?= e($row['version']) ?></code></td>
                            <td><?= e($row['platform']) ?></td>
                            <td class="num"><?= (int) $row['downloads'] ?></td>
                        </tr>
                    <?php endforeach; ?>
                    </tbody>
                </table>
            <?php endif; ?>
        </section>
    </div>

    <div class="grid two">
        <section class="panel">
            <h2>System</h2>
            <dl class="kv">
                <dt>API base</dt><dd><code><?= e(rtrim(RB_API_URL, '/')) ?></code></dd>
                <dt>Website</dt><dd><a href="<?= e(RB_SITE_URL) ?>"><?= e(RB_SITE_URL) ?></a></dd>
                <dt>PHP</dt><dd><?= e(PHP_VERSION) ?></dd>
                <dt>SQLite</dt><dd><?= e(fmt_bytes((int) $data['db_size'])) ?></dd>
                <dt>Timezone</dt><dd><?= e(RB_TIMEZONE) ?> · <?= e(date('Y-m-d H:i')) ?></dd>
                <dt>Update channel</dt><dd>stable</dd>
            </dl>
        </section>

        <section class="panel">
            <h2>Quick start</h2>
            <p class="muted small">Publish a release and the desktop app picks it up on its next check;
                the website pulls it live from the same API.</p>
            <div class="quick">
                <a class="btn" href="<?= e(admin_url('releases')) ?>">Add a release</a>
                <a class="btn" href="<?= e(admin_url('announcements')) ?>">Post an announcement</a>
                <a class="btn" href="/api/version?platform=windows" target="_blank" rel="noopener">Test API</a>
            </div>
        </section>
    </div>

<?php elseif ($tab === 'releases'): ?>
    <section class="panel">
        <div class="panel-head">
            <h2>Releases</h2>
            <button class="btn btn-primary btn-sm" data-open="release-dialog">New release</button>
        </div>

        <form class="filters" method="get" action="/admin">
            <input type="hidden" name="tab" value="releases">
            <input name="q" value="<?= e($data['q']) ?>" placeholder="Search version / URL / notes">
            <select name="platform">
                <?php foreach ($platforms as $p): ?>
                    <option value="<?= e($p) ?>" <?= $data['platform'] === $p ? 'selected' : '' ?>><?= e($p) ?></option>
                <?php endforeach; ?>
            </select>
            <select name="channel">
                <?php foreach ($channels as $c): ?>
                    <option value="<?= e($c) ?>" <?= $data['channel'] === $c ? 'selected' : '' ?>><?= e($c) ?></option>
                <?php endforeach; ?>
            </select>
            <button class="btn" type="submit">Filter</button>
            <a class="btn btn-ghost" href="<?= e(admin_url('releases')) ?>">Reset</a>
        </form>

        <?php if (!$data['releases']['items']): ?>
            <p class="empty">No releases match. Add one to start serving updates.</p>
        <?php else: ?>
            <div class="table-wrap">
                <table class="tbl">
                    <thead>
                    <tr><th>Version</th><th>Channel</th><th>Platform</th><th>Size</th><th>Min</th><th>Force</th><th>Published</th><th></th></tr>
                    </thead>
                    <tbody>
                    <?php foreach ($data['releases']['items'] as $r): ?>
                        <?php $scheduled = ($r['published_at'] ?? '') > date('Y-m-d H:i:s'); ?>
                        <tr>
                            <td>
                                <strong><?= e($r['version']) ?></strong>
                                <div class="trunc muted small" title="<?= e($r['download_url']) ?>"><?= e($r['download_url']) ?></div>
                            </td>
                            <td><span class="pill"><?= e($r['channel']) ?></span></td>
                            <td><?= e($r['platform']) ?></td>
                            <td><?= (int) $r['file_size'] > 0 ? e(fmt_bytes((int) $r['file_size'])) : '—' ?></td>
                            <td><?= $r['min_version'] ? e($r['min_version']) : '—' ?></td>
                            <td><?= $r['force_update'] ? '<span class="pill danger">forced</span>' : '—' ?></td>
                            <td class="muted">
                                <?= e(fmt_datetime($r['published_at'])) ?>
                                <?php if ($scheduled): ?><span class="pill warn">scheduled</span><?php endif; ?>
                            </td>
                            <td class="actions">
                                <button class="btn btn-sm" data-edit-release="<?= e(json_encode($r, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE)) ?>">Edit</button>
                                <button class="btn btn-sm btn-danger" data-delete="release" data-id="<?= (int) $r['id'] ?>">Delete</button>
                            </td>
                        </tr>
                    <?php endforeach; ?>
                    </tbody>
                </table>
            </div>
            <?php pager($data['releases'], 'releases', $filters); ?>
        <?php endif; ?>
    </section>

    <?php $dialog_id = 'release-dialog'; require __DIR__ . '/release_form.php'; ?>

<?php elseif ($tab === 'announcements'): ?>
    <section class="panel">
        <div class="panel-head">
            <h2>Announcements</h2>
            <button class="btn btn-primary btn-sm" data-open="announcement-dialog">New announcement</button>
        </div>

        <form class="filters" method="get" action="/admin">
            <input type="hidden" name="tab" value="announcements">
            <input name="q" value="<?= e($data['q']) ?>" placeholder="Search title / body">
            <select name="active">
                <option value="">All</option>
                <option value="1" <?= $data['active'] === '1' ? 'selected' : '' ?>>Published</option>
                <option value="0" <?= $data['active'] === '0' ? 'selected' : '' ?>>Drafts</option>
            </select>
            <button class="btn" type="submit">Filter</button>
            <a class="btn btn-ghost" href="<?= e(admin_url('announcements')) ?>">Reset</a>
        </form>

        <?php if (!$data['announcements']['items']): ?>
            <p class="empty">No announcements yet. They show up in the app and on the website.</p>
        <?php else: ?>
            <div class="table-wrap">
                <table class="tbl">
                    <thead><tr><th>Title</th><th>Level</th><th>Status</th><th>Window</th><th>Updated</th><th></th></tr></thead>
                    <tbody>
                    <?php foreach ($data['announcements']['items'] as $a): ?>
                        <?php
                        $live = (int) $a['active'] === 1
                            && ($a['starts_at'] === null || $a['starts_at'] <= date('Y-m-d H:i:s'))
                            && ($a['ends_at'] === null || $a['ends_at'] >= date('Y-m-d H:i:s'));
                        ?>
                        <tr>
                            <td>
                                <strong><?= e($a['title']) ?></strong>
                                <div class="trunc muted small"><?= e(mb_substr($a['body'], 0, 90)) ?></div>
                            </td>
                            <td><span class="pill <?= $a['level'] === 'critical' ? 'danger' : ($a['level'] === 'warning' ? 'warn' : '') ?>"><?= e($a['level']) ?></span></td>
                            <td>
                                <?php if ((int) $a['active'] === 0): ?>
                                    <span class="pill">draft</span>
                                <?php elseif ($live): ?>
                                    <span class="pill ok">live</span>
                                <?php else: ?>
                                    <span class="pill warn">scheduled / expired</span>
                                <?php endif; ?>
                            </td>
                            <td class="muted small">
                                <?= $a['starts_at'] ? e(fmt_datetime($a['starts_at'])) . ' <span class="muted">(' . e(fmt_relative($a['starts_at'])) . ')</span>' : 'always' ?>
                                →
                                <?= $a['ends_at'] ? e(fmt_datetime($a['ends_at'])) : 'never' ?>
                            </td>
                            <td class="muted"><?= e(fmt_datetime($a['updated_at'] ?: $a['created_at'])) ?></td>
                            <td class="actions">
                                <button class="btn btn-sm" data-edit-announcement="<?= e(json_encode($a, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE)) ?>">Edit</button>
                                <button class="btn btn-sm" data-toggle-announcement="<?= e(json_encode(['id' => $a['id'], 'active' => (int) $a['active']], JSON_UNESCAPED_SLASHES)) ?>">
                                    <?= (int) $a['active'] === 1 ? 'Unpublish' : 'Publish' ?>
                                </button>
                                <button class="btn btn-sm btn-danger" data-delete="announcement" data-id="<?= (int) $a['id'] ?>">Delete</button>
                            </td>
                        </tr>
                    <?php endforeach; ?>
                    </tbody>
                </table>
            </div>
            <?php pager($data['announcements'], 'announcements', $filters); ?>
        <?php endif; ?>
    </section>

    <?php $dialog_id = 'announcement-dialog'; require __DIR__ . '/announcement_form.php'; ?>

<?php elseif ($tab === 'downloads'): ?>
    <section class="panel">
        <div class="panel-head">
            <h2>Downloads</h2>
            <a class="btn btn-sm" href="/admin/export?days=<?= (int) $data['days'] ?>&platform=<?= e($data['platform']) ?>">Export CSV</a>
        </div>

        <form class="filters" method="get" action="/admin">
            <input type="hidden" name="tab" value="downloads">
            <input name="q" value="<?= e($data['q']) ?>" placeholder="Search version / IP / user agent">
            <select name="platform">
                <?php foreach ($platforms as $p): ?>
                    <option value="<?= e($p) ?>" <?= $data['platform'] === $p ? 'selected' : '' ?>><?= e($p) ?></option>
                <?php endforeach; ?>
            </select>
            <select name="days">
                <?php foreach ([7, 30, 90, 365] as $d): ?>
                    <option value="<?= $d ?>" <?= (int) $data['days'] === $d ? 'selected' : '' ?>>last <?= $d ?> days</option>
                <?php endforeach; ?>
            </select>
            <button class="btn" type="submit">Filter</button>
            <a class="btn btn-ghost" href="<?= e(admin_url('downloads')) ?>">Reset</a>
        </form>

        <div class="sparkline"><?= render_chart($data['series']) ?></div>

        <?php if (!$data['downloads']['items']): ?>
            <p class="empty">No downloads in this range.</p>
        <?php else: ?>
            <div class="table-wrap">
                <table class="tbl">
                    <thead><tr><th>Time</th><th>Version</th><th>Platform</th><th>Channel</th><th>IP</th><th>User agent</th></tr></thead>
                    <tbody>
                    <?php foreach ($data['downloads']['items'] as $d): ?>
                        <tr>
                            <td class="muted"><?= e(fmt_datetime($d['created_at'])) ?></td>
                            <td><code><?= e($d['version']) ?></code></td>
                            <td><?= e($d['platform']) ?></td>
                            <td><?= e($d['channel']) ?></td>
                            <td><code><?= e($d['ip']) ?></code></td>
                            <td class="trunc muted small" title="<?= e($d['user_agent']) ?>"><?= e($d['user_agent']) ?></td>
                        </tr>
                    <?php endforeach; ?>
                    </tbody>
                </table>
            </div>
            <?php pager($data['downloads'], 'downloads', $filters); ?>
        <?php endif; ?>
    </section>

<?php else: ?>
    <div class="grid two">
        <section class="panel">
            <h2>Change password</h2>
            <form class="stack" data-ajax>
                <input type="hidden" name="_csrf" value="<?= e($csrf) ?>">
                <input type="hidden" name="action" value="change_password">
                <label>Current password
                    <input type="password" name="current_password" autocomplete="current-password" required>
                </label>
                <label>New password
                    <input type="password" name="new_password" autocomplete="new-password" minlength="8" required>
                </label>
                <label>Repeat new password
                    <input type="password" name="confirm_password" autocomplete="new-password" minlength="8" required>
                </label>
                <div class="row">
                    <button class="btn btn-primary" type="submit">Update password</button>
                    <span class="result" role="status"></span>
                </div>
            </form>
        </section>

        <section class="panel">
            <h2>Backup</h2>
            <p class="muted small">Downloads a consistent snapshot of the SQLite database
                (releases, announcements, download log, admin hashes).</p>
            <a class="btn" href="/admin/backup">Download database backup</a>

            <h2 class="mt">API endpoints</h2>
            <ul class="endpoints">
                <li><code>/api/version?platform=windows</code></li>
                <li><code>/api/update/check?version=0.1.0&amp;platform=windows</code></li>
                <li><code>/api/announcements</code> · <code>?scope=history</code></li>
                <li><code>/api/releases?limit=20</code></li>
                <li><code>/api/stats</code> · <code>/api/health</code></li>
                <li><code>/api/download?platform=windows</code></li>
            </ul>
            <p class="muted small">All public endpoints send ETag + short cache headers; the download
                endpoint redirects to the asset and records the hit.</p>
        </section>
    </div>
<?php endif; ?>
</main>

<script src="/admin.js"></script>
</body>
</html>
