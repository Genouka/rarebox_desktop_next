# 从 jsDelivr 下载本项目使用的 Material Symbols Rounded 图标（开发脚本）。
# 运行应用不需要网络；assets/icons 已随仓库提供。
$icons = @(
    'watch', 'dashboard', 'apps', 'screenshot_monitor', 'terminal', 'subject', 'folder',
    'settings', 'battery_full', 'battery_charging_full', 'storage', 'memory', 'speed',
    'refresh', 'usb', 'wifi', 'download', 'upload', 'delete', 'play_arrow', 'stop_circle',
    'restart_alt', 'android', 'search', 'close', 'remove', 'check', 'check_circle',
    'content_copy', 'open_in_new', 'folder_open', 'add', 'info', 'error', 'warning',
    'chevron_right', 'arrow_back', 'power_settings_new', 'videocam', 'lan', 'cable',
    'system_update', 'bug_report', 'data_object', 'file_download', 'file_upload', 'image',
    'more_vert', 'tune', 'dark_mode', 'light_mode', 'link', 'link_off', 'sync',
    'hourglass_empty', 'build', 'description', 'home', 'list', 'crop_free', 'aspect_ratio',
    'smartphone', 'usb_off', 'wifi_off', 'play_circle', 'keyboard', 'code', 'help',
    'verified', 'security', 'bolt', 'monitor_heart', 'devices', 'settings_remote',
    'install_desktop', 'adb', 'data_usage', 'create_new_folder', 'arrow_upward',
    'expand_more', 'expand_less', 'keyboard_arrow_down', 'keyboard_arrow_up', 'more_horiz',
    'launch', 'cleaning_services', 'sensors', 'wifi_tethering', 'add_circle', 'remove_circle',
    'toggle_on', 'toggle_off', 'edit', 'save', 'folder_off', 'drive_file_move'
)

$outDir = Join-Path $PSScriptRoot '..\assets\icons'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$failed = [System.Collections.Generic.List[string]]::new()
foreach ($icon in $icons) {
    $target = Join-Path $outDir "$icon.svg"
    if (Test-Path $target) {
        continue
    }
    $downloaded = $false
    for ($attempt = 1; $attempt -le 3 -and -not $downloaded; $attempt++) {
        try {
            $uri = "https://cdn.jsdelivr.net/npm/@material-symbols/svg-400@latest/rounded/$icon.svg"
            Invoke-WebRequest -Uri $uri -UseBasicParsing -TimeoutSec 20 -OutFile $target -ErrorAction Stop
            $downloaded = $true
        }
        catch {
            Start-Sleep -Milliseconds 600
        }
    }
    if (-not $downloaded) {
        $failed.Add($icon)
    }
}

$count = (Get-ChildItem $outDir -Filter *.svg).Count
Write-Output "图标总数: $count"
if ($failed.Count -gt 0) {
    Write-Output "下载失败: $($failed -join ', ')"
}
