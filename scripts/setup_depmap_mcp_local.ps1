param(
    [string]$RuntimeRoot = 'D:\New-PHD\depmap_0823\runtime',
    [string]$RepoRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
$venv = Join-Path $RuntimeRoot 'depmap-mcp-venv'
$pipTemp = Join-Path $RuntimeRoot 'pip-tmp'
New-Item -ItemType Directory -Force -Path $RuntimeRoot, $pipTemp | Out-Null

if (-not (Test-Path -LiteralPath (Join-Path $venv 'Scripts\python.exe'))) {
    python -m venv --system-site-packages $venv
}

$env:TEMP = $pipTemp
$env:TMP = $pipTemp
$python = Join-Path $venv 'Scripts\python.exe'
& $python -m pip install --disable-pip-version-check --no-cache-dir `
    -r (Join-Path $RepoRoot 'services\depmap_mcp\requirements.txt')

& $python -c "import fastapi, mcp, pyarrow; print('DepMap MCP runtime ready')"
