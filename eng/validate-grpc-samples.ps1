$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

Write-Host "Validating gRPC samples (syntax/compile-only where possible)..." -ForegroundColor Cyan

function Has-Command($name) {
    try { Get-Command $name -ErrorAction Stop | Out-Null; return $true } catch { return $false }
}

$protoPath = Join-Path $repoRoot "src/Croniq.Rpc.Client/Protos/scheduler.proto"
if (-not (Test-Path $protoPath)) {
    throw "scheduler.proto not found at $protoPath"
}

# Node: syntax check (does not require dependencies)
if (Has-Command "node") {
    Write-Host "Node: syntax check" -ForegroundColor Yellow
    node --check "samples/grpc-client-node/index.js"
} else {
    Write-Warning "Node.js not found, skipping Node sample check."
}

# Python: only run if generated stubs are present
$pyStub = Join-Path $repoRoot "samples/grpc-client-python/scheduler_pb2.py"
if ((Test-Path $pyStub) -and (Has-Command "python")) {
    Write-Host "Python: py_compile client.py" -ForegroundColor Yellow
    python -m py_compile "samples/grpc-client-python/client.py"
} else {
    Write-Host "Python: skipping (stubs missing or python not available)" -ForegroundColor DarkYellow
}

# Go: only run if generated stubs are present
$goStub = Join-Path $repoRoot "samples/grpc-client-go/scheduler.pb.go"
if ((Test-Path $goStub) -and (Has-Command "go")) {
    Write-Host "Go: go build" -ForegroundColor Yellow
    go build ./samples/grpc-client-go
} else {
    Write-Host "Go: skipping (stubs missing or Go toolchain not available)" -ForegroundColor DarkYellow
}

Write-Host "Validation complete." -ForegroundColor Green
