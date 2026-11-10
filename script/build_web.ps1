#!/usr/bin/env pwsh
# 构建浏览器版（WebUSB）。
#
# 用法：pwsh ./script/build_web.ps1
# 产物：dist/web/（静态站点；需要 HTTPS 或 localhost，WebUSB 才可用）
#
# 依赖：
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.130
$ErrorActionPreference = "Stop"

Set-Location (Join-Path $PSScriptRoot "..")

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$out = "dist/web"

cargo build --release --locked -p rarebox-web --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { throw "cargo build 失败" }

if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force -Path $out | Out-Null

wasm-bindgen --target web --out-dir $out --out-name rarebox_web target/wasm32-unknown-unknown/release/rarebox_web.wasm
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen 失败" }

Copy-Item -Recurse -Force "web/*" $out
New-Item -ItemType Directory -Force -Path "$out/icons" | Out-Null
Copy-Item -Force "assets/icons/*.svg" "$out/icons/"

Write-Host "已生成 $out (RareBox Web $version)"
Write-Host "本地预览：python -m http.server 8080 --directory $out"
