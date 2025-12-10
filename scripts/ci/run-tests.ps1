param(
    [Parameter(Mandatory = $true)]
    [string] $Project,

    [Parameter(Mandatory = $true)]
    [string] $DisplayName,

    [string] $Configuration = "Release",
    [string] $ResultsDirectory = "TestResults",
    [string] $CoverageDirectory = "coverage"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function New-SafeName {
    param([string] $Name)
    return ($Name -replace '[^A-Za-z0-9._-]', '_')
}

$resultsPath = Join-Path -Path (Get-Location) -ChildPath $ResultsDirectory
$coveragePath = Join-Path -Path (Get-Location) -ChildPath $CoverageDirectory

New-Item -Path $resultsPath -ItemType Directory -Force | Out-Null
New-Item -Path $coveragePath -ItemType Directory -Force | Out-Null

$sanitizedName = New-SafeName -Name $DisplayName
$coverageOutput = Join-Path -Path $coveragePath -ChildPath $sanitizedName

$trxFile = "$sanitizedName.trx"

Write-Host "::group::Running tests for $DisplayName"
& dotnet test $Project `
    --configuration $Configuration `
    --no-build `
    --logger "trx;LogFileName=$trxFile" `
    --results-directory $ResultsDirectory `
    -p:CollectCoverage=true `
    -p:CoverletOutput="../../coverage/$sanitizedName/" `
    -p:CoverletOutputFormat=cobertura
Write-Host "::endgroup::"
