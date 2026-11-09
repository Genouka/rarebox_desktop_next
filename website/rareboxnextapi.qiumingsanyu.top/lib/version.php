<?php
/**
 * Semantic-ish version comparison.
 *
 * Compares dot-separated numeric segments (e.g. 1.2.10 > 1.2.9).
 * Any non-numeric suffix (e.g. "-beta.1") is stripped before comparison.
 * Returns -1, 0, or 1.
 */

declare(strict_types=1);

namespace RareBox;

function normalize_version(string $v): string
{
    return preg_replace('/[^0-9.].*$/', '', trim($v)) ?: '0';
}

function version_compare_semver(string $a, string $b): int
{
    $pa = array_values(array_filter(explode('.', normalize_version($a)), static fn ($x) => $x !== ''));
    $pb = array_values(array_filter(explode('.', normalize_version($b)), static fn ($x) => $x !== ''));
    $pa = $pa === [] ? ['0'] : $pa;
    $pb = $pb === [] ? ['0'] : $pb;

    $n = max(count($pa), count($pb));
    for ($i = 0; $i < $n; $i++) {
        $xa = (int) ($pa[$i] ?? 0);
        $xb = (int) ($pb[$i] ?? 0);
        if ($xa !== $xb) {
            return $xa <=> $xb;
        }
    }
    return 0;
}

/** True when `$candidate` is strictly newer than `$current`. */
function version_is_newer(string $candidate, string $current): bool
{
    return version_compare_semver($candidate, $current) > 0;
}
