# RareBox Desktop — API & Admin Backend

Deployed at `rareboxnextapi.qiumingsanyu.top` (PHP 8.0+, nginx + PHP-FPM, SQLite).

Serves update checks, announcements, downloads and stats to the desktop app and
the static site, plus an admin dashboard to publish releases and announcements.

## Requirements

- PHP 8.0+ with `pdo_sqlite` (and `sqlite3` for tooling).
- Write access to `data/` for the web-server user (SQLite lives there).
- Nothing else: no composer, no build step.

## Layout

```
config.php                defaults (URLs, timezone, enums, cache, throttle)
config.local.php          optional local overrides, git-ignored
index.php                 front controller: routes /api/* and /admin/*
router.php                dev-only router for `php -S`
lib/http.php              JSON/ETag/cache helpers, params, validation
lib/version.php           semver-lite comparison
lib/db.php                PDO/SQLite bootstrap, schema, admin seeding
lib/auth.php              session, login throttling, CSRF
lib/api.php               public endpoints
lib/admin.php             dashboard queries, CRUD, CSV export, DB backup
lib/view.php              admin template helpers (escaping, pager, dates)
admin/login.php           login view
admin/dashboard.php       dashboard view (server-rendered tabs)
admin/release_form.php    create/edit release dialog
admin/announcement_form.php  create/edit announcement dialog
admin.css / admin.js      dashboard UI
data/app.sqlite           runtime DB, git-ignored
nginx.conf.example        server block
```

## Public API

| Method | Path | Notes |
|---|---|---|
| GET | `/api/health` | liveness + DB probe; `503` when SQLite is unusable |
| GET | `/api/version?platform=&channel=stable` | newest published release; `404` when none |
| GET | `/api/update/check?version=&platform=&channel=stable` | update decision + active announcements, never cached |
| GET | `/api/announcements` | active announcements (window respected) |
| GET | `/api/announcements?scope=history` | every published announcement, expired included, drafts excluded |
| GET | `/api/releases?platform=&channel=&limit=20` | changelog, grouped by version |
| GET | `/api/stats` | counters + 14-day series |
| GET | `/api/download?platform=&channel=stable` | logs the hit, then `302` to the asset (send `Accept: application/json` to get the URL instead) |

`/api/version`, `/api/announcements`, `/api/releases` and `/api/stats` send
`ETag` + `Cache-Control: public, max-age=60` (300 for stats) and answer `304`
to `If-None-Match`. `/api/update/check` depends on the caller's version, so it is
always `no-store`.

### Release selection

`latest_release()` resolves one row in this order:

1. exact channel + exact platform,
2. exact channel + `platform = 'all'`,
3. `stable` (only when the caller asked for `beta` / `nightly`) + exact platform,
4. `stable` + `all`.

A `stable` client can therefore never be handed a beta build — that was the old
query's bug. Rows with a `published_at` in the future are treated as scheduled
and skipped until their time comes.

### Update-check response

```json
{
  "update_available": true,
  "current_version": "0.1.0",
  "latest_version": "1.0.0",
  "channel": "stable",
  "platform": "windows",
  "platform_requested": "windows",
  "download_url": "https://…",
  "file_size": 123456,
  "notes": "…",
  "published_at": "2026-10-26 12:00:00",
  "force_update": false,
  "required_min_version": "0.9.0",
  "reasons": ["new_version", "below_minimum"],
  "announcements": [{ "id": 1, "title": "…", "body": "…", "level": "info" }]
}
```

`reasons` is `new_version`, `forced` and/or `below_minimum`. With no release at
all the endpoint returns `200` with `update_available: false` and
`reason: "no_release"`, which older clients already handle.

## Admin

Visit `/admin/login`. The seeded account is `admin` / `change-me-please`;
the dashboard shows a warning banner until you change it in **Settings**.

- **Overview** — download/unique-IP counters, 14-day chart, top versions, last published release, DB size.
- **Releases** — search + platform/channel filters, pagination, create/edit dialog, delete, scheduled rows badged.
- **Announcements** — search + draft/published filter, create/edit dialog, one-click publish/unpublish, scheduling window.
- **Downloads** — range/platform filters, paginated log, CSV export (UTF-8 BOM for Excel).
- **Settings** — password change, SQLite backup download (`VACUUM INTO`), endpoint reference.

Tabs are plain URLs (`/admin?tab=releases&page=2`), so the dashboard works with
JavaScript disabled; the dialogs and AJAX submit are progressive enhancement.

## Security notes

- CSRF token required for every POST (`X-Csrf-Token` header or `_csrf` field), rotated on login and password change.
- Sessions use `HttpOnly`, `SameSite=Lax`, `Secure` (when `RB_FORCE_HTTPS`), strict mode, and are regenerated on login.
- Failed logins are recorded per IP + username; after `RB_LOGIN_MAX_ATTEMPTS` (8) inside `RB_LOGIN_WINDOW_MINUTES` (15) the form refuses to check passwords. nginx adds a request-rate limit on top.
- Public API responses carry CORS `*` (read-only, anonymous); admin responses carry no CORS headers at all.
- Repeated `/api/download` hits from one IP for the same release inside 120 s are logged once, so counters stay honest.
- `data/`, `lib/`, `config.php`, `router.php` and `admin/*.php` are blocked by the server block; only `index.php` is reachable as code.

## Local development

```bash
cd website/rareboxnextapi.qiumingsanyu.top
php -S 127.0.0.1:8080 router.php
curl -s http://127.0.0.1:8080/api/health
```

For plain-HTTP development, create `config.local.php` next to `config.php`
(git-ignored, never deploy it):

```php
<?php return ['RB_FORCE_HTTPS' => false];
```

Point the static site at it with `window.RAREBOX_API = 'http://127.0.0.1:8080'`.

## Deploy

生产环境使用 nginx + php8.2-fpm；服务器地址、部署脚本与 `config.local.php` 不随仓库公开。

部署时请用安全的同步流程把 `website/rareboxnextapi.qiumingsanyu.top/` 同步到服务器，并确保：

1. `data/`、`config.local.php`、`router.php` 只存在于服务器，且 `data/` 归 `www-data:www-data`、权限 750；
2. 代码文件权限为 644/755，并逐个执行 `php -l`；
3. 修改 nginx 配置后执行 `nginx -t && systemctl reload nginx`；
4. 重载 `php8.2-fpm` 刷新 opcache；
5. 请求 `/api/health` 做健康检查。

数据库备份可用 SQLite 的 `VACUUM INTO` 生成一致性快照，或在后台 **Settings → Download database backup** 手动下载一份。


数据库结构在首次请求时幂等创建，旧库可直接沿用。
