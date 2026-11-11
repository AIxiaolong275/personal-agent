# Personal Agent — start local services and open the web UI.
# Safe to run from any directory. Does not touch any installed Codex/cc-switch.

$root = "C:\personal agent"

function Test-Port([int]$Port) {
    try {
        $client = New-Object System.Net.Sockets.TcpClient
        $client.Connect("127.0.0.1", $Port)
        $client.Close()
        return $true
    } catch {
        return $false
    }
}

if (-not (Test-Port 15722)) {
    Start-Process -FilePath "$root\pa-proxy\target\debug\pa-proxy.exe" `
        -WorkingDirectory $root -WindowStyle Hidden
}

if (-not (Test-Port 15723)) {
    Start-Process -FilePath "$root\pa-web\target\debug\pa-web.exe" `
        -WorkingDirectory $root -WindowStyle Hidden
}

Start-Sleep -Milliseconds 800
Start-Process "http://127.0.0.1:15723"
