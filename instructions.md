# Atlas Network Integration

Run `atlas-hardfork-planner` as a small HTTP service next to the Atlas network services. It should be able to reach a Teth node JSON-RPC endpoint, and Atlas clients should read the published schedule from:

```text
http://<planner-host>:28882/atlas-protocol-schedule.json
```

Use the Atlas chain ID in both the schedule file and the service config. The default schedule in this repository uses `chainId: 42069`.

## Required Runtime Configuration

```env
LISTEN_HOST=0.0.0.0
LISTEN_PORT=28882
HTML_TITLE="Atlas Hardfork Planner"
SCHEDULE_PATH=/data/atlas-protocol-schedule.json
CHAIN_ID=42069
RPC_URL=http://teth:8545
RPC_POLL_SECONDS=10
RPC_TIMEOUT_MS=5000
ADMIN_BEARER_KEY=<strong random admin token>
```

`RPC_URL` must point at the Teth node used by the Atlas network. On startup the planner calls `eth_chainId`; if the RPC chain does not match the schedule chain ID, or the check cannot complete, the service exits instead of publishing an unverified schedule.

## Docker Compose Example

```yaml
services:
  protocol-schedule:
    image: ghcr.io/atlas-chain/atlas-hardfork-planner:main
    ports:
      - "28882:28882"
    environment:
      LISTEN_HOST: "0.0.0.0"
      LISTEN_PORT: "28882"
      HTML_TITLE: "Atlas Hardfork Planner"
      SCHEDULE_PATH: /data/atlas-protocol-schedule.json
      CHAIN_ID: "42069"
      RPC_URL: http://teth:8545
      RPC_POLL_SECONDS: "10"
      RPC_TIMEOUT_MS: "5000"
      ADMIN_BEARER_KEY: ${ADMIN_BEARER_KEY}
    volumes:
      - ./data:/data
    restart: unless-stopped
```

Place the active schedule at `./data/atlas-protocol-schedule.json`. Keep this file backed up; admin changes are persisted there before they are published in memory.

## Operating Notes

- Leave `ADMIN_BEARER_KEY` unset to run in read-only mode.
- Set `ADMIN_BEARER_KEY` only for trusted operators, and access the UI over a trusted network or TLS-terminated proxy.
- Configure Atlas nodes or tooling to consume `/atlas-protocol-schedule.json`.
- Use `/healthz` for a lightweight liveness check and `/status` for current version, chain ID, current block, and retained release history.
