# syntax=docker/dockerfile:1.7
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,id=nuget-packages,target=/root/.nuget/packages \
	--mount=type=cache,id=nuget-http-cache,target=/root/.local/share/NuGet/v3-cache \
	dotnet restore samples/Croniq.Sample.WorkerHost/Croniq.Sample.WorkerHost.csproj
RUN --mount=type=cache,id=nuget-packages,target=/root/.nuget/packages \
	--mount=type=cache,id=nuget-http-cache,target=/root/.local/share/NuGet/v3-cache \
	dotnet publish samples/Croniq.Sample.WorkerHost/Croniq.Sample.WorkerHost.csproj -c Release -o /app/publish /p:UseAppHost=false --no-restore

FROM mcr.microsoft.com/dotnet/runtime:10.0 AS runtime
WORKDIR /app
RUN apt-get update \
	&& apt-get install -y --no-install-recommends gpgv \
	&& rm -rf /var/lib/apt/lists/*
COPY --from=build /app/publish .
ENTRYPOINT ["dotnet", "Croniq.Sample.WorkerHost.dll"]
