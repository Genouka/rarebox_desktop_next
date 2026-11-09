#!/usr/bin/env pwsh
# 构建 Windows 发布包。
#
# 用法：pwsh ./script/package_windows.ps1
# 产物：dist/RareBox-Desktop-<版本>-windows-x64.zip
$ErrorActionPreference = "Stop"

Set-Location (Join-Path $PSScriptRoot "..")

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$stage = "stage/RareBox-Desktop-$version-windows-x64"

cargo build --release --bin rarebox-desktop

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path "$stage/assets" | Out-Null
Copy-Item "target/release/rarebox-desktop.exe" "$stage/"
Copy-Item -Recurse "assets/icons" "$stage/assets/icons"

New-Item -ItemType Directory -Force -Path "dist" | Out-Null
$zip = "dist/RareBox-Desktop-$version-windows-x64.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$stage/*" -DestinationPath $zip

Write-Host "已生成 $zip"
