# FXStreet News Ingestion Engine

Rust-based ingestion engine for FXStreet economic calendar data.
It supports:
- real-time webhook ingestion (`AWS Lambda -> QuestDB`)
- historical backfill (`CLI -> FXStreet REST API -> QuestDB`)
- infrastructure provisioning (`Terraform on AWS`)

## Definition of Done (DoD)

- [x] Core: event models (`FxEventRaw` / `EconomicEvent`) and API field mapping verified; `cargo test -p core` passes
- [x] Webhook POST request is received and stored in QuestDB (test mode validated)
- [x] Backfill CLI stores historical events for a given date range (`--test` flag validated)
- [x] Full infrastructure is deployed with one `terraform apply`
- [x] Retry behavior and logs make failures diagnosable (`input` / `api` / `db`)
- [x] Another engineer can reproduce the setup with this README only

## Repository Layout

```text
project-root/
├─ crates/
│  ├─ core/      # shared models, FXStreet client, QuestDB writer, errors
│  ├─ lambda/    # webhook ingestion Lambda
│  └─ cli/       # historical backfill tool
└─ infra/
   └─ terraform/ # AWS infrastructure
```

## Prerequisites

- Rust toolchain
- `cargo-lambda` (for local Lambda testing)
- Reachable QuestDB instance (`<QUESTDB_HOST>:9009`)
- (Optional, real mode) FXStreet bearer token

## Local Test

No FXStreet credentials required. Use built-in test modes.

### 1) Webhook Lambda

Start local runtime:

```bash
cargo lambda watch -p lambda \
  --env-var WEBHOOK_SECRET_TOKEN=my-secret-key,QUESTDB_HOST=<QUESTDB_HOST>,QUESTDB_ILP_PORT=9009
```

Send test event using `X-Test-Mode: true` header (inserts dummy data, no FXStreet API call):

```bash
curl -i -X POST http://127.0.0.1:9000/lambda-url/lambda \
  -H "Content-Type: application/json" \
  -H "X-Webhook-Token: my-secret-key" \
  -H "X-Test-Mode: true" \
  -d '{}'
```

Expected result: `HTTP 200` and body `OK (test mode)`.

### 2) Backfill CLI

Insert single dummy event directly into QuestDB (no FXStreet API call):

```bash
QUESTDB_HOST=<QUESTDB_HOST> QUESTDB_ILP_PORT=9009 \
cargo run -p cli -- --from 2026-03-01T00:00:00Z --to 2026-03-10T00:00:00Z --test
```

Dry-run local test (no FXStreet API call, no DB write):

```bash
QUESTDB_HOST=<QUESTDB_HOST> QUESTDB_ILP_PORT=9009 \
cargo run -p cli -- --from 2026-03-01T00:00:00Z --to 2026-03-10T00:00:00Z --test --dry-run
```

`<QUESTDB_HOST>` example:
- local Docker QuestDB: `127.0.0.1`
- AWS EC2 QuestDB: `terraform output -raw questdb_public_ip`

## Real FXStreet Mode

For live API calls, set credentials and run the CLI:

```bash
FXSTREET_BEARER_TOKEN="<token>" \
FXSTREET_API_BASE="https://calendar-api.fxstreet.com/en/api/v1" \
QUESTDB_HOST=<QUESTDB_HOST> QUESTDB_ILP_PORT=9009 \
cargo run -p cli -- --from 2026-03-01T00:00:00Z --to 2026-03-10T00:00:00Z --page-size 10
```

## Terraform Deploy (AWS)

```bash
cargo lambda build --release --arm64 -p lambda  # build Lambda binary first
cd infra/terraform
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars: see terraform.tfvars.example for reference
# Required secrets in terraform.tfvars:
# webhook_secret_token  = "<webhook-shared-secret>"
# fxstreet_bearer_token = "<fxstreet-bearer-token>"
terraform init
terraform apply
terraform output   # prints questdb_public_ip and webhook_lambda_public_url
```

## Notes

- Retry: transient failures only (network, `429`, `5xx`) with backoff.
- Fast-fail: non-retryable errors (`400/401/403/404`).
- Log categories: `input`, `api`, `db`.
- If local webhook returns `500`, first check missing env vars (`QUESTDB_HOST`, `QUESTDB_ILP_PORT`).
- **Lambda test mode**: add `X-Test-Mode: true` header to any authenticated POST → inserts dummy event directly into QuestDB (no FXStreet API call needed).
- **CLI test mode**: add `--test` flag → inserts single dummy event and exits immediately.
- **Real path vs test fallback**: non-test requests use the real FXStreet API path; test mode exists only as a verification fallback when credentials/webhook delivery are unavailable.
- Without `FXSTREET_BEARER_TOKEN`, non-test API calls will fail: Lambda returns `500`, CLI returns `Configuration error: Missing FXSTREET_BEARER_TOKEN`.
- **Function URL note**: if direct external `curl` to Function URL returns `403` in your AWS account, validate webhook behavior with `aws lambda invoke` (handler logic verified).
- **Networking trade-off**: current setup uses QuestDB public IP reachability for simplicity; production-hardening would place Lambda inside VPC and restrict QuestDB ingress to private CIDRs/security groups.

## Next Steps

1. ~~Implement shared models and configuration in `crates/core`.~~ ✓
2. ~~Implement QuestDB writer and table bootstrap logic.~~ ✓
3. ~~Implement webhook Lambda flow in `crates/lambda`.~~ ✓
4. ~~Implement backfill CLI flow in `crates/cli`.~~ ✓
5. ~~Implement Terraform infrastructure in `infra/terraform`.~~ ✓
