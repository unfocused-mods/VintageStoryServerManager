FROM rust:1.96-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends libdbus-1-dev pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY apps/desktop apps/desktop
RUN cargo build --locked --release -p vsm --bins

FROM node:22-bookworm-slim AS frontend
WORKDIR /src
COPY package.json package-lock.json ./
COPY apps/ui apps/ui
RUN npm ci && npm run build

FROM mcr.microsoft.com/dotnet/runtime:8.0-noble AS runtime8
FROM mcr.microsoft.com/dotnet/runtime:10.0-noble AS runtime
COPY --from=runtime8 /usr/share/dotnet/shared/Microsoft.NETCore.App /usr/share/dotnet/shared/Microsoft.NETCore.App
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates openssh-client restic libdbus-1-3 curl util-linux && rm -rf /var/lib/apt/lists/*
RUN useradd --system --home /var/lib/vsm/data --shell /usr/sbin/nologin vintagestory
ENV VSM_GAME_USER=vintagestory
COPY --from=builder /src/target/release/vsm /usr/local/bin/vsm
COPY --from=builder /src/target/release/vsm-agent /usr/local/bin/vsm-agent
COPY --from=frontend /src/apps/ui/dist /opt/vsm/ui
ENV VSM_AGENT_BINARY=/usr/local/bin/vsm-agent
WORKDIR /var/lib/vsm
EXPOSE 4317
ENTRYPOINT ["vsm"]
CMD ["serve", "--data", "/var/lib/vsm", "--ui", "/opt/vsm/ui", "--bind", "0.0.0.0:4317", "--public"]
