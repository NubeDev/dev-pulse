# dev-pulse — local dev convenience.
#
#   make build   # cargo build + pnpm install + pnpm build
#   make start   # backend + frontend, backgrounded
#   make kill    # stop both via PID files in .run/
#   make logs    # tail both logs
#   make status  # show whether each is running
#
# `make start` loads `.env` (gitignored, see .env.example) so the
# `secret://github/pat` handle resolves to $GITHUB_PAT.
#
# Vite is run on port 5180 (non-default) to avoid clashing with any
# other Vite project on the box. Both ports defined below; if you
# change them, update frontend/vite.config.ts (server.port + proxy
# targets) and config.local.toml ([server].listen) to match.

SHELL := /bin/bash

ROOT       := $(CURDIR)
RUN_DIR    := $(ROOT)/.run
LOG_DIR    := $(ROOT)/.run/logs
BACK_PID   := $(RUN_DIR)/backend.pid
FRONT_PID  := $(RUN_DIR)/frontend.pid
BACK_LOG   := $(LOG_DIR)/backend.log
FRONT_LOG  := $(LOG_DIR)/frontend.log

CONFIG_SRC := $(ROOT)/crates/dev-pulse/config.example.toml
CONFIG     := $(ROOT)/config.local.toml
ENV_FILE   := $(ROOT)/.env

BACK_PORT  := 8731
FRONT_PORT := 8732
COMPOSE    := $(ROOT)/crates/dp-store-pg/docker-compose.yml

.PHONY: build start kill stop restart status logs config seed-admin db migrate help

help:
	@echo "Targets: build | start | kill | restart | status | logs | config"

# ---------------------------------------------------------------- db

db:
	@docker compose -f $(COMPOSE) up -d
	@echo "waiting for postgres to be ready..."
	@until docker exec dev-pulse-postgres pg_isready -U dev-pulse -d dev_pulse >/dev/null 2>&1; do \
	  sleep 1; \
	done
	@echo "postgres ready on :5432"

# ---------------------------------------------------------------- build

build:
	cargo build -p dev-pulse
	pnpm install --frozen-lockfile=false
	pnpm --filter dev-pulse-frontend build

# ---------------------------------------------------------------- config

config: $(CONFIG)

$(CONFIG): $(CONFIG_SRC)
	@if [ ! -f $(CONFIG) ]; then \
	  cp $(CONFIG_SRC) $(CONFIG); \
	  echo "wrote $(CONFIG) — edit it if you need non-default values"; \
	fi

# ---------------------------------------------------------------- start

start: config migrate $(RUN_DIR) $(LOG_DIR)
	@if [ ! -f $(ENV_FILE) ]; then \
	  echo "no .env — copy .env.example to .env and fill in GITHUB_PAT first"; \
	  exit 1; \
	fi
	@if [ -f $(BACK_PID) ] && kill -0 $$(cat $(BACK_PID)) 2>/dev/null; then \
	  echo "backend already running (pid $$(cat $(BACK_PID)))"; \
	else \
	  echo "starting backend on :$(BACK_PORT) (log: $(BACK_LOG))"; \
	  ( set -a; . $(ENV_FILE); set +a; \
	    nohup cargo run -p dev-pulse -- serve --config $(CONFIG) \
	      >$(BACK_LOG) 2>&1 & echo $$! >$(BACK_PID) ); \
	fi
	@if [ -f $(FRONT_PID) ] && kill -0 $$(cat $(FRONT_PID)) 2>/dev/null; then \
	  echo "frontend already running (pid $$(cat $(FRONT_PID)))"; \
	else \
	  echo "starting frontend on :$(FRONT_PORT) (log: $(FRONT_LOG))"; \
	  ( cd $(ROOT)/frontend; \
	    nohup pnpm dev --port $(FRONT_PORT) --strictPort \
	      >$(FRONT_LOG) 2>&1 & echo $$! >$(FRONT_PID) ); \
	fi
	@echo ""
	@echo "  backend  → http://localhost:$(BACK_PORT)"
	@echo "  frontend → http://localhost:$(FRONT_PORT)"
	@echo ""
	@echo "  make logs    # tail both"
	@echo "  make kill    # stop both"

# ---------------------------------------------------------------- kill

kill stop:
	@for f in $(BACK_PID) $(FRONT_PID); do \
	  if [ -f $$f ]; then \
	    pid=$$(cat $$f); \
	    if kill -0 $$pid 2>/dev/null; then \
	      echo "killing $$(basename $$f .pid) (pid $$pid)"; \
	      kill $$pid 2>/dev/null || true; \
	      for i in 1 2 3 4 5; do \
	        kill -0 $$pid 2>/dev/null || break; \
	        sleep 1; \
	      done; \
	      kill -9 $$pid 2>/dev/null || true; \
	    fi; \
	    rm -f $$f; \
	  fi; \
	done
	@# Belt + braces: cargo run spawns a child `dev-pulse` binary
	@# whose pid isn't what nohup recorded. Sweep any stragglers
	@# that match our cargo manifest or the vite dev server.
	@pkill -f "target/debug/dev-pulse serve" 2>/dev/null || true
	@pkill -f "vite.*--port $(FRONT_PORT)"   2>/dev/null || true
	@echo "stopped."

restart: kill start

# ---------------------------------------------------------------- migrate

migrate: db
	cargo run -p dev-pulse -- migrate --config $(CONFIG)

# ---------------------------------------------------------------- seed-admin

seed-admin: config
	cargo run -p dev-pulse -- create-admin \
	  --config $(CONFIG) \
	  --email "dev@dev.com" \
	  --password "dev123456789"

# ---------------------------------------------------------------- introspection

status:
	@for name in backend frontend; do \
	  pidfile=$(RUN_DIR)/$$name.pid; \
	  if [ -f $$pidfile ] && kill -0 $$(cat $$pidfile) 2>/dev/null; then \
	    echo "$$name: running (pid $$(cat $$pidfile))"; \
	  else \
	    echo "$$name: stopped"; \
	  fi; \
	done

logs:
	@touch $(BACK_LOG) $(FRONT_LOG)
	tail -F $(BACK_LOG) $(FRONT_LOG)

# ---------------------------------------------------------------- dirs

$(RUN_DIR):
	@mkdir -p $(RUN_DIR)

$(LOG_DIR):
	@mkdir -p $(LOG_DIR)
