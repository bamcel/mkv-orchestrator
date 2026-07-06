# syntax=docker/dockerfile:1

FROM node:22-bookworm-slim AS web-build
WORKDIR /src/web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM mcr.microsoft.com/dotnet/sdk:10.0 AS dotnet-build
WORKDIR /src
COPY MKVOrchestrator.sln ./
COPY src/MKVOrchestrator.Core/MKVOrchestrator.Core.csproj src/MKVOrchestrator.Core/
COPY src/MKVOrchestrator.WebHost/MKVOrchestrator.WebHost.csproj src/MKVOrchestrator.WebHost/
RUN dotnet restore src/MKVOrchestrator.WebHost/MKVOrchestrator.WebHost.csproj
COPY src/MKVOrchestrator.Core/ src/MKVOrchestrator.Core/
COPY src/MKVOrchestrator.WebHost/ src/MKVOrchestrator.WebHost/
COPY --from=web-build /src/web/dist/ src/MKVOrchestrator.WebHost/wwwroot/
RUN dotnet publish src/MKVOrchestrator.WebHost/MKVOrchestrator.WebHost.csproj -c Release -o /app/publish --no-restore

FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg mkvtoolnix ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=dotnet-build /app/publish ./
ENV ASPNETCORE_URLS=http://+:8080 \
    MKVO_MEDIA_ROOT=/media \
    HOME=/config \
    XDG_CONFIG_HOME=/config \
    XDG_DATA_HOME=/config/.local/share
RUN mkdir -p /media /config
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD curl -fsS http://localhost:8080/api/health || exit 1
ENTRYPOINT ["dotnet", "MKVOrchestrator.WebHost.dll"]
