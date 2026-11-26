<#
.SYNOPSIS
Runs the title with the switches chosen for an RTX 3060 Ti (8 GB, Wave32),
16 GB of RAM and a 6-core i5-12400F.

.DESCRIPTION
-Profile Quality (the default): the Series X profile at its own resolution,
with only the switches that save memory or time without changing a pixel:

    VICTORY_LEVELS_ON_DEMAND=1   only the texture levels the title loaded get
                                 memory on the card (exact, about -3 GB)
    VICTORY_PLACED_TEX=1         placed textures in 64 MiB heaps (#903)
    VICTORY_TEXTURE_BACK=1       water textures read back for the title's CPU
                                 (boats float; on by default anyway)
    VICTORY_CARD_SYNC_BATCH=1    send-and-hash batching (#1227)
    VICTORY_ASYNC_SUBMIT=1       a thread of its own submits
    VICTORY_QUEUE2=1             fetches of copies on the card on a copy queue
    VICTORY_LOOKUP_CACHE=1       a thread remembers its buffer lookups
                                 (#1237 with #1245)
    VICTORY_BIND_CACHE=1         the same render targets are not set again
    VICTORY_DECLARE_ONCE=1       a range shaders write is declared once
    VICTORY_RAM_TRIM_MB=2048     below 2 GB of free RAM or commit, the reuse
                                 pools are emptied (Windows error 1455)
    VICTORY_NO_WATCHDOG=1        the title's 30-second freeze kill stays away
                                 while shaders compile

-Profile Survival8GB: Quality plus VICTORY_TEXTURE_DROP_MIPS=1, for dense
open-world scenes that overflow 8 GB: large textures lose their sharpest
level. A visible loss, so not part of Quality.

-SeriesS: the Series S profile (--console series-s) with the title's own
temporal upscaler (VICTORY_OWN_UPSCALER=1) instead of its FSR.

Switches already set in the environment are kept, and -Switches overrides
everything, so one run can differ from the profile in one place:
    .\scripts\run-3060ti.ps1 -Switches 'VICTORY_LOOKUP_CACHE=0'

Not set by any profile, and why:
- VICTORY_DYNAMIC_SLOTS: more dynamic descriptor slots on NVIDIA's capped
  view heap mean fewer of the title's slots kept in place, and a view a
  shader picks by index past those reads a dynamic slot. Use
  -DynamicSlots 524288 only when the log says "no free dynamic descriptor
  slots" (dropped draws), and compare the picture.
- VICTORY_AMBIENT: a stand-in for the indirect light while rays do not run
  (-Ambient 0.6 for the flat one, -Ambient sh for the one estimated from the
  frame). It changes the lighting, so it is asked for by hand.
- VICTORY_RENDER_SCALE: renders below the output size; softer.

.EXAMPLE
.\scripts\run-3060ti.ps1
.\scripts\run-3060ti.ps1 -Profile Survival8GB
.\scripts\run-3060ti.ps1 -SeriesS -Ambient sh
.\scripts\run-3060ti.ps1 -DryRun
#>
[CmdletBinding()]
param(
    # Quality (Series X, exact switches only) or Survival8GB (adds dropped
    # texture levels for scenes that overflow 8 GB).
    [ValidateSet('Quality', 'Survival8GB')]
    [string]$Profile = 'Quality',
    # The Series S profile, with the title's own upscaler.
    [switch]$SeriesS,
    # The title's executable (default: the one .exe in game_dir).
    [string]$Exe,
    # The emulator to run (default: target\release\victory.exe).
    [string]$Victory,
    # VICTORY_DYNAMIC_SLOTS (0: not set; see the description).
    [int]$DynamicSlots = 0,
    # VICTORY_RAM_TRIM_MB (0: not set).
    [int]$RamTrimMB = 2048,
    # VICTORY_AMBIENT: a level for the flat stand-in, or sh (not set if empty).
    [string]$Ambient = '',
    # More switches, or other values: @{ NAME = 'value' } or 'NAME=value'
    # (several as a list or separated by commas).
    [object]$Switches = @{},
    # Print what would run, and do not run it.
    [switch]$DryRun,
    # Arguments for the title, after --.
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TitleArgs
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$mode = $Profile
$victoryExe = if ($Victory) { (Resolve-Path $Victory).Path } else { Join-Path $root 'target\release\victory.exe' }
if (-not (Test-Path -LiteralPath $victoryExe)) {
    throw "no build at $victoryExe (cargo build --release -p victory, or -Victory <path>)"
}

if ($Switches -isnot [hashtable]) {
    $table = @{}
    foreach ($pair in @($Switches) | ForEach-Object { "$_" -split '[,;]' } | Where-Object { $_ }) {
        $name, $value = $pair -split '=', 2
        if ($null -eq $value) { throw "a switch is NAME=VALUE, not '$pair'" }
        $table[$name.Trim()] = $value.Trim()
    }
    $Switches = $table
}

# The profile, in order.
$profileSwitches = [ordered]@{
    VICTORY_LEVELS_ON_DEMAND = '1'
    VICTORY_PLACED_TEX       = '1'
    VICTORY_TEXTURE_BACK     = '1'
    VICTORY_CARD_SYNC_BATCH  = '1'
    VICTORY_ASYNC_SUBMIT     = '1'
    VICTORY_QUEUE2           = '1'
    VICTORY_LOOKUP_CACHE     = '1'
    VICTORY_BIND_CACHE       = '1'
    VICTORY_DECLARE_ONCE     = '1'
    VICTORY_NO_WATCHDOG      = '1'
}
if ($RamTrimMB -gt 0) { $profileSwitches['VICTORY_RAM_TRIM_MB'] = "$RamTrimMB" }
if ($DynamicSlots -gt 0) { $profileSwitches['VICTORY_DYNAMIC_SLOTS'] = "$DynamicSlots" }
if ($Ambient) { $profileSwitches['VICTORY_AMBIENT'] = $Ambient }
if ($mode -eq 'Survival8GB') { $profileSwitches['VICTORY_TEXTURE_DROP_MIPS'] = '1' }
if ($SeriesS) { $profileSwitches['VICTORY_OWN_UPSCALER'] = '1' }

$applied = [ordered]@{}
foreach ($name in $profileSwitches.Keys) {
    $already = [Environment]::GetEnvironmentVariable($name, 'Process')
    if ($null -ne $already -and -not $Switches.ContainsKey($name)) {
        $applied[$name] = "$already (kept from the environment)"
        continue
    }
    $applied[$name] = $profileSwitches[$name]
}
foreach ($name in $Switches.Keys) {
    $applied[$name] = "$($Switches[$name])"
}

# The configuration as `victory config` resolves it.
if (-not $Exe) {
    Push-Location $root
    try { $config = & $victoryExe config } finally { Pop-Location }
    $gameDir = $null
    foreach ($line in $config) {
        if ($line -match '^game_dir\s*:\s*(.+?)(\s+\(does not exist\))?$') { $gameDir = $Matches[1] }
    }
    if (-not $gameDir -or $gameDir -eq '(not set)') { throw 'no game_dir configured (victory.toml or VICTORY_GAME_DIR), and no -Exe' }
    $exes = @(Get-ChildItem -LiteralPath $gameDir -Filter '*.exe' -File)
    if ($exes.Count -ne 1) { throw "game_dir holds $($exes.Count) executables; name one with -Exe" }
    $Exe = $exes[0].FullName
}

$arguments = @('run', $Exe, '--execute')
if ($SeriesS) { $arguments += @('--console', 'series-s') }
if ($TitleArgs) { $arguments += @('--') + $TitleArgs }

$console = if ($SeriesS) { 'Series S' } else { 'Series X' }
Write-Host "RTX 3060 Ti profile: $mode, $console"
foreach ($name in $applied.Keys) { Write-Host ("  {0} = {1}" -f $name, $applied[$name]) }
Write-Host ("  {0} {1}" -f $victoryExe, ($arguments -join ' '))
if ($DryRun) { return }

foreach ($name in $applied.Keys) {
    $value = "$($applied[$name])" -replace ' \(kept from the environment\)$', ''
    [Environment]::SetEnvironmentVariable($name, $value, 'Process')
}
Push-Location $root
try {
    & $victoryExe @arguments
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
