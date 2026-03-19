# FXStreet News Ingestion Engine

This repository contains the implementation for a Rust-based
ingestion engine with AWS Lambda, QuestDB, and Terraform.

## Definition of Done (DoD)

- [x] Core: event models (`FxEventRaw` / `EconomicEvent`) and API field mapping verified; `cargo test -p core` passes
- [ ] Webhook `POST` request is received and stored in QuestDB
- [ ] Backfill CLI stores historical events for a given date range
- [ ] Full infrastructure is deployed with one `terraform apply`
- [ ] Retry behavior and logs make failures diagnosable
- [ ] Another engineer can reproduce the setup with this README only

## Current Project Structure

```text
project-root/
├─ Cargo.toml
├─ README.md
├─ crates/
│  ├─ core/
│  ├─ lambda/
│  └─ cli/
└─ infra/
   └─ terraform/
```

## Local Testing (Webhook Lambda)

To test the webhook Lambda locally without deploying to AWS:

1. Install `cargo-lambda` (e.g., `brew tap cargo-lambda/cargo-lambda && brew install cargo-lambda`).
2. Start the local emulator in the project root:
   ```bash
   FXSTREET_MODE=mock WEBHOOK_SECRET_TOKEN="my-secret-key" cargo lambda watch
   ```
3. Send a test POST request in another terminal:
   ```bash
   curl -v -X POST http://127.0.0.1:9000/lambda-url/lambda \
     -H "Content-Type: application/json" \
     -H "X-Webhook-Token: my-secret-key" \
     -d '{"eventDateId": "test-uuid"}'
   ```

## Local Testing (Backfill CLI)

To test the backfill CLI locally with a dry-run (mock mode: no external token required):

```bash
FXSTREET_MODE=mock cargo run -p cli -- --from 2026-03-01T00:00:00Z --to 2026-03-10T00:00:00Z --page-size 10 --dry-run
```

For real FXStreet calls, switch to:
```bash
FXSTREET_MODE=real FXSTREET_BEARER_TOKEN="<token>" FXSTREET_API_BASE="https://calendar-api.fxstreet.com/en/api/v1"
```

## Next Steps

1. ~~Implement shared models and configuration in `crates/core`.~~ ✓
2. ~~Implement QuestDB writer and table bootstrap logic.~~ ✓
3. ~~Implement webhook Lambda flow in `crates/lambda`.~~ ✓
4. ~~Implement backfill CLI flow in `crates/cli`.~~ ✓
5. Implement Terraform infrastructure in `infra/terraform`.
