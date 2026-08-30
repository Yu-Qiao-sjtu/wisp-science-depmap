param(
    [int]$LocalPort = 18876,
    [int]$RemotePort = 8876,
    [string]$SshHost = "guotosky"
)

$createdNew = $false
$tunnelMutex = [Threading.Mutex]::new($true, "Local\WispScienceDepMapTunnel", [ref]$createdNew)
if (-not $createdNew) {
    exit 0
}

try {
    $sshPath = (Get-Command ssh.exe -ErrorAction Stop).Source
    $arguments = @(
        "-N",
        "-o", "BatchMode=yes",
        "-o", "ExitOnForwardFailure=yes",
        "-o", "ServerAliveInterval=30",
        "-o", "ServerAliveCountMax=3",
        "-L", "127.0.0.1:${LocalPort}:127.0.0.1:${RemotePort}",
        $SshHost
    )
    while ($true) {
        & $sshPath @arguments
        Start-Sleep -Seconds 5
    }
}
finally {
    $tunnelMutex.ReleaseMutex()
    $tunnelMutex.Dispose()
}
