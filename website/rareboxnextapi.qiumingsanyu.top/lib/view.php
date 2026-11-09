<?php
/**
 * Small view helpers shared by the admin templates.
 */

declare(strict_types=1);

namespace RareBox;

function e($value): string
{
    return htmlspecialchars((string) $value, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8');
}

function fmt_bytes(int $bytes): string
{
    if ($bytes <= 0) {
        return '0 B';
    }
    $units = ['B', 'KB', 'MB', 'GB'];
    $value = (float) $bytes;
    $unit = 0;
    while ($value >= 1024 && $unit < count($units) - 1) {
        $value /= 1024;
        $unit++;
    }
    return $unit === 0 ? $bytes . ' B' : number_format($value, 1) . ' ' . $units[$unit];
}

function fmt_datetime(?string $value): string
{
    if ($value === null || $value === '') {
        return '—';
    }
    $ts = strtotime($value);
    return $ts === false ? (string) $value : date('Y-m-d H:i', $ts);
}

/** "in 3 days" / "2 hours ago" for scheduled announcement windows. */
function fmt_relative(?string $value): string
{
    if ($value === null || $value === '') {
        return '';
    }
    $ts = strtotime($value);
    if ($ts === false) {
        return '';
    }
    $diff = $ts - time();
    $abs = abs($diff);
    $unit = 'minute';
    $n = (int) round($abs / 60);
    if ($abs >= 86400) {
        $unit = 'day';
        $n = (int) round($abs / 86400);
    } elseif ($abs >= 3600) {
        $unit = 'hour';
        $n = (int) round($abs / 3600);
    }
    $label = $n . ' ' . $unit . ($n === 1 ? '' : 's');
    return $diff >= 0 ? 'in ' . $label : $label . ' ago';
}

/**
 * Build an admin URL for a tab, keeping only the filters relevant to it.
 *
 * @param array<string,string|int|null> $params
 */
function admin_url(string $tab, array $params = []): string
{
    $params = array_filter($params, static fn ($v) => $v !== null && $v !== '');
    $params['tab'] = $tab;
    return '/admin?' . http_build_query($params);
}

/** Pagination URLs for the given tab + filters. */
function page_url(string $tab, array $filters, int $page): string
{
    return admin_url($tab, $filters + ['page' => $page]);
}

/** Render a `« 1 2 3 »` pager for a paginated block. */
function pager(array $p, string $tab, array $filters): void
{
    if (($p['pages'] ?? 1) <= 1) {
        echo '<p class="muted small">' . (int) ($p['total'] ?? 0) . ' item(s)</p>';
        return;
    }
    $page = (int) $p['page'];
    $pages = (int) $p['pages'];
    echo '<nav class="pager">';
    if ($page > 1) {
        echo '<a href="' . e(page_url($tab, $filters, $page - 1)) . '">‹</a>';
    }
    $start = max(1, $page - 2);
    $end = min($pages, $start + 4);
    $start = max(1, $end - 4);
    for ($i = $start; $i <= $end; $i++) {
        $cls = $i === $page ? ' class="on"' : '';
        echo '<a href="' . e(page_url($tab, $filters, $i)) . '"' . $cls . '>' . $i . '</a>';
    }
    if ($page < $pages) {
        echo '<a href="' . e(page_url($tab, $filters, $page + 1)) . '">›</a>';
    }
    echo '<span class="muted small">' . (int) $p['total'] . ' item(s)</span></nav>';
}
