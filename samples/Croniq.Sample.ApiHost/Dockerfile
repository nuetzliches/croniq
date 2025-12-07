# syntax=docker/dockerfile:1.7
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src
COPY . .
RUN dotnet restore samples/Croniq.Sample.ApiHost/Croniq.Sample.ApiHost.csproj
RUN dotnet publish samples/Croniq.Sample.ApiHost/Croniq.Sample.ApiHost.csproj -c Release -o /app/publish /p:UseAppHost=false

FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime
WORKDIR /app
COPY --from=build /app/publish .
ENV ASPNETCORE_URLS=http://+:8080
ENTRYPOINT ["dotnet", "Croniq.Sample.ApiHost.dll"]
