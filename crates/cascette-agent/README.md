# cascette-agent

Local HTTP agent service compatible with Blizzard Agent.exe (version 3.13.3).

Provides a REST API on port 1120 for managing game product installations,
updates, repairs, and verification.

## Usage

```bash
# Start with default settings (port 1120)
cascette-agent

# Custom port
cascette-agent --port=8080

# Custom database path
cascette-agent --db_path=/path/to/agent.db

# Debug logging
cascette-agent --loglevel=debug
```

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/agent` | Agent info |
| GET | `/game` | List products |
| GET | `/game/{product}` | Product details |
| POST | `/install/{product}` | Start installation |
| POST | `/update/{product}` | Start update |
| POST | `/repair/{product}` | Start repair |
| POST | `/uninstall/{product}` | Start uninstall |
| POST | `/backfill/{product}` | Start backfill |
| GET | `/version` | Version info |
| GET | `/hardware` | System info |
| GET/POST | `/gamesession/{product}` | Game sessions |
| GET/POST | `/download` | Download config |
| GET/POST | `/option` | Product options |
| GET | `/size_estimate` | Install size |
| GET | `/health` | Health check |
| GET | `/metrics` | Prometheus metrics |
