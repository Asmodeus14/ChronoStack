# Minimal image of Polar's backend (server/), built from source: Python 3.14 + uv,
# plus the email renderer built as in server/Dockerfile (stage build-emails-bin).
# Skips the production-only stages (Tailscale, IPInfo, fonts, backoffice assets).
FROM node:24-slim AS build-emails-bin
ENV PNPM_HOME="/pnpm" PATH="/pnpm:$PATH"
RUN corepack enable
WORKDIR /app
COPY emails/pnpm-lock.yaml emails/package.json emails/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY emails/src src/
COPY emails/tsconfig.json emails/tsdown.config.mjs ./
RUN pnpm run build

FROM python:3.14-slim-bookworm
COPY --from=ghcr.io/astral-sh/uv:0.13.4 /uv /usr/local/bin/uv
RUN apt-get update && apt-get install -y --no-install-recommends git build-essential libpq-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app/server
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PROJECT_ENVIRONMENT=/app/.venv PATH="/app/.venv/bin:$PATH"
COPY pyproject.toml uv.lock .python-version ./
RUN uv sync --frozen --no-install-project
COPY . .
RUN uv sync --frozen
COPY --from=build-emails-bin /app/bin/react-email-pkg /app/server/
ENV POLAR_EMAIL_RENDERER_BINARY_PATH=/app/server/react-email-pkg
