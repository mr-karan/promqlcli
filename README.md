# promqlcli

Rust CLI for querying Prometheus/VictoriaMetrics endpoints, with basic auth or bearer token support.

## Build

```bash
cargo build --release
```

Binary path: `target/release/prometheus-metrics`

## Configuration

Environment variables (override with CLI flags):

- `PROMQL_BASE_URL` (required, e.g. `https://prometheus.example.com`)
- `PROMQL_AUTH` (basic auth in `user:password` form)
- `PROMQL_USER` / `PROMQL_PASS`
- `PROMQL_BEARER` (bearer token, takes precedence)

## Usage

```bash
prometheus-metrics --help
```

### Instant query

```bash
PROMQL_BASE_URL=https://prometheus.example.com \
  prometheus-metrics query 'rate(process_cpu_seconds_total[5m]) * 100' --result --pretty
```

### Range query

```bash
PROMQL_BASE_URL=https://prometheus.example.com \
  prometheus-metrics range 'gnatsd_slow_consumer_count' \
  --start 2026-01-22T03:45:00Z \
  --end 2026-01-22T04:30:00Z \
  --step 60s \
  --result
```

### List jobs

```bash
PROMQL_BASE_URL=https://prometheus.example.com prometheus-metrics jobs --lines
```

### List metric names (filter)

```bash
PROMQL_BASE_URL=https://prometheus.example.com prometheus-metrics metrics --filter haproxy --lines
```

### Find series

```bash
PROMQL_BASE_URL=https://prometheus.example.com \
  prometheus-metrics series --match 'node_cpu_seconds_total{job="ec2-kite"}' \
  --start 2026-01-22T03:45:00Z \
  --end 2026-01-22T04:30:00Z
```

## Notes

- VictoriaMetrics expects UTC timestamps.
- Use `--result` to print `.data.result` directly for `query` and `range`.
- Use `--lines` for list endpoints to print one value per line.
