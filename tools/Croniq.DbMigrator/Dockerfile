# syntax=docker/dockerfile:1.7
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,id=nuget-packages,target=/root/.nuget/packages \
	--mount=type=cache,id=nuget-http-cache,target=/root/.local/share/NuGet/v3-cache \
	dotnet restore tools/Croniq.DbMigrator/Croniq.DbMigrator.csproj
RUN --mount=type=cache,id=nuget-packages,target=/root/.nuget/packages \
	--mount=type=cache,id=nuget-http-cache,target=/root/.local/share/NuGet/v3-cache \
	dotnet publish tools/Croniq.DbMigrator/Croniq.DbMigrator.csproj -c Release -o /app/publish /p:UseAppHost=false --no-restore

FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime
WORKDIR /app
COPY --from=build /app/publish .
ENTRYPOINT ["dotnet", "Croniq.DbMigrator.dll"]
