param(
    [Parameter(Mandatory = $true)]
    [string] $DacpacPath,

    [string] $Server = "localhost",
    [int] $Port = 1433,
    [string] $Database = "Croniq",
    [string] $User = "sa",
    [string] $Password = "P@ssw0rd1234",
    [switch] $AllowDataLoss
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path $DacpacPath)) {
    throw "Dacpac not found: $DacpacPath"
}

$dacpacFull = (Resolve-Path $DacpacPath).Path

$arguments = @(
    "/Action:Publish",
    "/SourceFile:$dacpacFull",
    "/TargetServerName:$Server,$Port",
    "/TargetDatabaseName:$Database",
    "/TargetUser:$User",
    "/TargetPassword:$Password",
    "/p:BlockOnPossibleDataLoss=$([bool](!$AllowDataLoss))",
    "/p:DropObjectsNotInSource=False"
)

$sqlPackagePath = Get-Command sqlpackage -ErrorAction SilentlyContinue
if (-not $sqlPackagePath) {
    throw "sqlpackage CLI not found. Install via 'dotnet tool install --global microsoft.sqlpackage' or download from Microsoft."
}

& $sqlPackagePath.Source $arguments
