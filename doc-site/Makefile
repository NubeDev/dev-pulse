# dev-pulse doc-site — the Nextra (Next.js + MDX) end-user documentation site.
#
# Reader-facing docs only. The source of truth for the published docs lives in
# ./content (authored here as MDX/MD). There is no backend to bring up:
# `make dev` serves a Next.js dev server, and `make build` produces a fully
# static export in ./out (deployable to any static host; `postbuild` adds the
# Pagefind search index).
#
# New here? TL;DR:
#   make setup   one-time: pnpm install
#   make dev     dev server with HMR → http://localhost:3010
#   make build   static export → ./out/ (+ Pagefind search index)
#   make start   serve the production build on :3010
#   make clean   drop build output (.next/ + out/)
#   make kill    free the dev/start port if a previous run was left running
#   make help    list targets

# Override the dev/start port, e.g. `make dev PORT=3000`.
PORT ?= 3010

# For a sub-path deploy (GitHub Pages project site, etc.) set BASE_PATH=/dev-pulse.
# Empty by default so `make dev`/`make build` serve from "/".
BASE_PATH ?=

# Point the navbar + "Edit this page" links at the repo (env-only, optional).
REPO_URL ?= https://github.com/NubeIO/dev-pulse

.PHONY: help setup dev build start clean kill

help:
	@echo "dev-pulse doc-site targets:"
	@echo "  make setup   pnpm install"
	@echo "  make dev     dev server → http://localhost:$(PORT)"
	@echo "  make build   static export → ./out/ (+ Pagefind index)"
	@echo "  make start   serve the build on :$(PORT)"
	@echo "  make kill    free port $(PORT) (kill any orphaned next dev/start)"
	@echo "  make clean   rm -rf .next out"
	@echo ""
	@echo "vars: PORT=$(PORT)  BASE_PATH=$(BASE_PATH)  REPO_URL=$(REPO_URL)"
	@echo "      e.g. make build BASE_PATH=/dev-pulse"

# `--ignore-workspace`: the dev-pulse root is a pnpm workspace that pins React 18
# for the frontend. The doc-site is intentionally NOT a workspace member — it
# installs standalone with its own (React 19) deps in ./node_modules.
setup:
	pnpm install --ignore-workspace
	@echo "setup done — now: make dev"

dev:
	pnpm exec next dev --port $(PORT)

# `pnpm build` runs `next build` (static export) then `postbuild` (Pagefind index).
# BASE_PATH is empty by default → serves from "/"; set BASE_PATH=/dev-pulse for a sub-path.
build:
	DOCS_BASE_PATH="$(BASE_PATH)" DOCS_REPO_URL="$(REPO_URL)" pnpm --ignore-workspace build

start:
	pnpm exec next start --port $(PORT)

clean:
	rm -rf .next out
	@echo "cleaned .next out"

# Free the dev/start port if a previous `make dev`/`make start` was left running.
# Same shape as the repo-root kill: SIGTERM whatever holds the port, then wait for
# it to actually let go (with a SIGKILL fallback) so the next `make dev` doesn't
# hit EADDRINUSE.
kill:
	-@fuser -TERM -k $(PORT)/tcp 2>/dev/null || true
	-@pkill -TERM -f '[n]ext dev' 2>/dev/null || true
	-@pkill -TERM -f '[n]ext start' 2>/dev/null || true
	-@pkill -TERM -f '[n]ext-server' 2>/dev/null || true
	@i=0; \
	while fuser $(PORT)/tcp >/dev/null 2>&1; do \
		i=$$((i+1)); \
		if [ $$i -ge 50 ]; then \
			fuser -KILL -k $(PORT)/tcp 2>/dev/null || true; \
			pkill -KILL -f '[n]ext dev' 2>/dev/null || true; \
			pkill -KILL -f '[n]ext start' 2>/dev/null || true; \
			pkill -KILL -f '[n]ext-server' 2>/dev/null || true; \
			break; \
		fi; \
		sleep 0.1; \
	done
	@echo "freed port $(PORT) and killed any orphaned next dev/start"
