$ErrorActionPreference = "Stop"
Push-Location $PSScriptRoot
try {
    # One three-wisp mark, with platform-specific tile geometry. Legacy ICNS
    # needs its own rounded tile and transparent margin. Store/mobile assets
    # use the full square; Windows/Linux use a rounded tile without the margin.
    $fullBleed = Resolve-Path "icons/app-icon.svg"
    $rounded = Resolve-Path "icons/app-icon-rounded.svg"
    $macos = Resolve-Path "icons/app-icon-macos.svg"
    $desktopNames = @(
        "icon.ico", "icon.png", "32x32.png", "64x64.png",
        "128x128.png", "128x128@2x.png"
    )
    $latestSource = ($fullBleed, $rounded, $macos, $PSCommandPath |
        Get-Item | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1).LastWriteTimeUtc
    $need = $false
    foreach ($name in ($desktopNames + @("icon.icns", "source.png"))) {
        $output = Join-Path "icons" $name
        if (-not (Test-Path $output) -or (Get-Item $output).LastWriteTimeUtc -lt $latestSource) {
            $need = $true
        }
    }
    if (-not $need) { return }

    $fullOut = Join-Path $PSScriptRoot "icons/.gen-full"
    $roundOut = Join-Path $PSScriptRoot "icons/.gen-rounded"
    $macOut = Join-Path $roundOut "macos"
    Remove-Item $fullOut, $roundOut -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $fullOut | Out-Null
    New-Item -ItemType Directory -Path $roundOut | Out-Null
    New-Item -ItemType Directory -Path $macOut | Out-Null
    cargo tauri icon $fullBleed -o $fullOut
    if ($LASTEXITCODE -ne 0) { throw "Failed to generate square icons" }
    cargo tauri icon $rounded -o $roundOut
    if ($LASTEXITCODE -ne 0) { throw "Failed to generate rounded icons" }
    cargo tauri icon $macos -o $macOut
    if ($LASTEXITCODE -ne 0) { throw "Failed to generate macOS icons" }

    Copy-Item -Path (Join-Path $fullOut "*") -Destination "icons" -Recurse -Force
    foreach ($name in $desktopNames) {
        Copy-Item -Force (Join-Path $roundOut $name) (Join-Path "icons" $name)
    }
    Copy-Item -Force (Join-Path $macOut "icon.icns") "icons/icon.icns"
    cargo tauri icon $fullBleed --png 1024 -o $fullOut
    if ($LASTEXITCODE -ne 0) { throw "Failed to generate source PNG" }
    Copy-Item -Force (Join-Path $fullOut "1024x1024.png") "icons/source.png"
    Remove-Item $fullOut, $roundOut -Recurse -Force
} finally {
    Pop-Location
}
