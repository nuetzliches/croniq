param(
    [string] $ContainerName = "croniq-sql-edge",
    [string] $SaPassword = "P@ssw0rd1234",
    [int] $HostPort = 1433,
    [switch] $UseDockerCompose,
    [string] $Database = "Croniq",
    [string] $DacpacPath,
    [switch] $AllowDataLoss
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($UseDockerCompose) {
    & docker compose -f infra/docker/docker-compose.sql.yml up -d
    if ($DacpacPath) {
        & pwsh ./scripts/ci/deploy-dacpac.ps1 -DacpacPath $DacpacPath -Database $Database -Port $HostPort -Password $SaPassword -AllowDataLoss:$AllowDataLoss
    }
    return
}

function Ensure-RunningContainer {
    param(
        [string] $Name,
        [string] $Image = "mcr.microsoft.com/azure-sql-edge:latest"
    )

    $existing = & docker ps -a --filter "name=$Name" --format "{{.Names}}"
    if ($existing -and $existing -contains $Name) {
        & docker start $Name | Out-Null
        return
    }

        & docker run -d --name $Name -e "ACCEPT_EULA=1" -e "MSSQL_SA_PASSWORD=$SaPassword" -p "$HostPort:1433" $Image | Out-Null
}

Ensure-RunningContainer -Name $ContainerName
Write-Host "SQL Edge container '$ContainerName' listening on port $HostPort"

    if ($DacpacPath) {
        Write-Host "Deploying dacpac '$DacpacPath' to $Database"
        & pwsh ./scripts/ci/deploy-dacpac.ps1 -DacpacPath $DacpacPath -Database $Database -Port $HostPort -Password $SaPassword -AllowDataLoss:$AllowDataLoss
    }
