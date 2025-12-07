# syntax=docker/dockerfile:1.7
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src
COPY . .
RUN dotnet restore samples/Croniq.Sample.WorkerHost/Croniq.Sample.WorkerHost.csproj
RUN dotnet publish samples/Croniq.Sample.WorkerHost/Croniq.Sample.WorkerHost.csproj -c Release -o /app/publish /p:UseAppHost=false

FROM mcr.microsoft.com/dotnet/runtime:10.0 AS runtime
WORKDIR /app
COPY --from=build /app/publish .
ENTRYPOINT ["dotnet", "Croniq.Sample.WorkerHost.dll"]
