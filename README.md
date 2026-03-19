# FXStreet News Ingestion Engine

This repository contains the implementation for a Rust-based
ingestion engine with AWS Lambda, QuestDB, and Terraform.

## Definition of Done (DoD)

- [x] Core: event models (`FxEventRaw` / `EconomicEvent`) and API field mapping verified; `cargo test -p core` passes
- [x] Webhook `POST` request is received and stored in QuestDB
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

## Next Steps

1. ~~Implement shared models and configuration in `crates/core`.~~ ✓
2. ~~Implement QuestDB writer and table bootstrap logic.~~ ✓
3. Implement webhook Lambda flow in `crates/lambda`.
4. Implement backfill CLI flow in `crates/cli`.
5. Implement Terraform infrastructure in `infra/terraform`.
