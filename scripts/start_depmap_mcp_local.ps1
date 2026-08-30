param(
    [ValidateSet('streamable-http', 'stdio')]
    [string]$Transport = 'streamable-http',
    [string]$HostAddress = '127.0.0.1',
    [int]$Port = 8877,
    [string]$KnowledgeRoot = 'D:\New-PHD\depmap_0823\knowledge',
    [string]$RuntimeRoot = 'D:\New-PHD\depmap_0823\runtime',
    [string]$Rscript = 'C:\Program Files\R\R-4.6.1\bin\x64\Rscript.exe'
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$python = Join-Path $RuntimeRoot 'depmap-mcp-venv\Scripts\python.exe'
$queryScript = Join-Path $repoRoot 'skills\depmap-knowledge-query\scripts\query_depmap_kb.R'
$processTemp = Join-Path $RuntimeRoot 'process-tmp'

foreach ($path in @($python, $KnowledgeRoot, $queryScript, $Rscript)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Required DepMap MCP path is missing: $path"
    }
}
New-Item -ItemType Directory -Force -Path $processTemp | Out-Null

$env:DEPMAP_KNOWLEDGE_ROOT = $KnowledgeRoot
$env:DEPMAP_QUERY_SCRIPT = $queryScript
$env:DEPMAP_RELEASE = '26Q1'
$env:DEPMAP_QUERY_TIMEOUT_SECONDS = '120'
$env:DEPMAP_MAX_CONCURRENCY = '2'
$env:RSCRIPT = $Rscript
$env:OMP_NUM_THREADS = '4'
$env:ARROW_NUM_THREADS = '4'
$env:R_DATATABLE_NUM_PROCS_PERCENT = '10'
$env:TEMP = $processTemp
$env:TMP = $processTemp

Push-Location $repoRoot
try {
    & $python -m services.depmap_mcp --transport $Transport --host $HostAddress --port $Port
}
finally {
    Pop-Location
}
