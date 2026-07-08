<#
.SYNOPSIS
    Verifies that a Windows machine has everything needed to build Nexo from
    source. Prints PASS / FAIL for each requirement with a fix hint, and exits
    non-zero if anything required is missing.

.DESCRIPTION
    Checks: Git, Rust (rustc + cargo), the MSVC Rust toolchain, Node.js, npm,
    the MSVC linker (link.exe / Visual Studio C++ build tools), and the WebView2
    runtime. This script only reads system state; it installs nothing and
    changes nothing. Compatible with Windows PowerShell 5.1 and PowerShell 7+.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\windows-check.ps1
#>

$ErrorActionPreference = 'Continue'

$script:failed = 0

function Write-Result {
    param(
        [string]$Name,
        [bool]$Ok,
        [string]$Detail = '',
        [string]$Fix = ''
    )
    if ($Ok) {
        Write-Host ("  [PASS] {0}" -f $Name) -ForegroundColor Green
        if ($Detail) { Write-Host ("         {0}" -f $Detail) -ForegroundColor DarkGray }
    }
    else {
        Write-Host ("  [FAIL] {0}" -f $Name) -ForegroundColor Red
        if ($Detail) { Write-Host ("         {0}" -f $Detail) -ForegroundColor DarkGray }
        if ($Fix) { Write-Host ("         Fix: {0}" -f $Fix) -ForegroundColor Yellow }
        $script:failed++
    }
}

function Test-Command {
    param([string]$Command)
    return [bool](Get-Command $Command -ErrorAction SilentlyContinue)
}

function Get-CommandVersion {
    param([string]$Command, [string[]]$CmdArgs = @('--version'))
    try {
        $out = & $Command @CmdArgs 2>$null | Select-Object -First 1
        return "$out".Trim()
    }
    catch {
        return ''
    }
}

Write-Host ''
Write-Host 'Nexo - Windows build environment check' -ForegroundColor Cyan
Write-Host '======================================'
Write-Host ''

# --- Git -------------------------------------------------------------------
$gitOk = Test-Command 'git'
$gitDetail = if ($gitOk) { Get-CommandVersion 'git' } else { 'git not found on PATH' }
Write-Result -Name 'Git' -Ok $gitOk -Detail $gitDetail `
    -Fix 'Install Git for Windows: https://git-scm.com/download/win (or: winget install Git.Git)'

# --- Rust: rustc + cargo ---------------------------------------------------
$rustcOk = Test-Command 'rustc'
$rustcDetail = if ($rustcOk) { Get-CommandVersion 'rustc' } else { 'rustc not found' }
Write-Result -Name 'Rust compiler (rustc)' -Ok $rustcOk -Detail $rustcDetail `
    -Fix 'Install Rust: https://rustup.rs (or: winget install Rustlang.Rustup)'

$cargoOk = Test-Command 'cargo'
$cargoDetail = if ($cargoOk) { Get-CommandVersion 'cargo' } else { 'cargo not found' }
Write-Result -Name 'Cargo' -Ok $cargoOk -Detail $cargoDetail `
    -Fix 'Install Rust: https://rustup.rs'

# --- Rust MSVC toolchain ---------------------------------------------------
$msvcToolchainOk = $false
$toolchainDetail = 'rustup not found'
if (Test-Command 'rustup') {
    $show = (& rustup show 2>$null) -join "`n"
    $hostLine = ($show -split "`n") | Where-Object { $_ -match 'Default host' } | Select-Object -First 1
    if ($hostLine) { $toolchainDetail = $hostLine.Trim() } else { $toolchainDetail = 'default host unknown' }
    $msvcToolchainOk = $show -match 'x86_64-pc-windows-msvc'
}
Write-Result -Name 'Rust MSVC toolchain' -Ok $msvcToolchainOk -Detail $toolchainDetail `
    -Fix 'rustup default stable-x86_64-pc-windows-msvc'

# --- Node.js + npm ---------------------------------------------------------
$nodeOk = Test-Command 'node'
$nodeDetail = if ($nodeOk) { Get-CommandVersion 'node' } else { 'node not found' }
Write-Result -Name 'Node.js' -Ok $nodeOk -Detail $nodeDetail `
    -Fix 'Install Node LTS: https://nodejs.org (or: winget install OpenJS.NodeJS.LTS)'

$npmOk = Test-Command 'npm'
$npmDetail = if ($npmOk) { Get-CommandVersion 'npm' } else { 'npm not found' }
Write-Result -Name 'npm' -Ok $npmOk -Detail $npmDetail `
    -Fix 'Comes with Node.js: https://nodejs.org'

# --- MSVC linker (Visual Studio C++ build tools) ---------------------------
# link.exe is only on PATH inside a Developer prompt, so also probe vswhere.
$linkOk = Test-Command 'link'
$linkDetail = 'link.exe on PATH'
if (-not $linkOk) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $vswhere) {
        $vc = & $vswhere -latest -products '*' `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath 2>$null
        if ($vc) {
            $linkOk = $true
            $linkDetail = "VC++ build tools found: $vc"
        }
        else {
            $linkDetail = 'Visual Studio found, but the C++ (VC.Tools) component is missing'
        }
    }
    else {
        $linkDetail = 'link.exe not on PATH and Visual Studio Installer not found'
    }
}
Write-Result -Name 'MSVC linker (link.exe / VC++ build tools)' -Ok $linkOk -Detail $linkDetail `
    -Fix 'Install "Build Tools for Visual Studio 2022" with the "Desktop development with C++" workload'

# --- WebView2 runtime ------------------------------------------------------
# Present when the Evergreen runtime registered a non-empty version under either
# HKLM (all users) or HKCU (per-user).
$wv2Version = $null
$wv2Keys = @(
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
)
foreach ($k in $wv2Keys) {
    $v = (Get-ItemProperty -Path $k -Name pv -ErrorAction SilentlyContinue).pv
    if ($v -and $v -ne '0.0.0.0') { $wv2Version = $v; break }
}
$wv2Ok = [bool]$wv2Version
$wv2Detail = if ($wv2Ok) { "version $wv2Version" } else { 'not detected' }
Write-Result -Name 'WebView2 runtime' -Ok $wv2Ok -Detail $wv2Detail `
    -Fix 'Install: winget install Microsoft.EdgeWebView2Runtime (the Nexo MSI also installs it automatically)'

# --- Summary ---------------------------------------------------------------
Write-Host ''
Write-Host '--------------------------------------'
if ($script:failed -eq 0) {
    Write-Host 'All checks passed. You can build Nexo:' -ForegroundColor Green
    Write-Host '  cd apps\desktop; npm install; npm run tauri build' -ForegroundColor Green
    exit 0
}
else {
    Write-Host ("{0} check(s) failed. Fix the items marked [FAIL] above, then re-run this script." -f $script:failed) -ForegroundColor Red
    Write-Host 'See docs\windows-development.md for full setup instructions.' -ForegroundColor Yellow
    exit 1
}
