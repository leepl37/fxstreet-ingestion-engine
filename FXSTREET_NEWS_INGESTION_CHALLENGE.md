# Backend Engineering Challenge: FXStreet News Ingestion Engine

## Objective

Build a production-ready ingestion engine that captures real-time economic calendar events from the FXStreet API and stores them in a QuestDB instance.

Your solution must handle real-time updates via webhooks and provide a mechanism for historical data backfilling.

## Technical Stack Requirements

- **Language:** Rust (safety, concurrency, and performance)
- **Infrastructure:** Terraform (AWS Provider)
- **Compute:** AWS Lambda (Rust runtime)
- **Database:** QuestDB (time-series optimized)
- **Entry Point:** AWS Lambda Function URL or API Gateway (webhook listener)

## Core Requirements

### 1) Ingestion Engine (Streaming)

- Implement a Rust-based Lambda function to process incoming `POST` requests from the FXStreet webhook.
- **Internal schema management:** On startup (or via a controlled initialization path), ensure the QuestDB table exists using `CREATE TABLE IF NOT EXISTS`.
- **Efficiency:** Use the QuestDB InfluxDB Line Protocol (ILP) for high-performance writes.

### 2) Historical Backfill

Implement dedicated backfill logic that allows a user to specify a date range and fetch historical news events from the FXStreet REST API.

This can be implemented as either:

- A separate CLI tool within the Rust workspace, or
- A specific invocation mode for the Lambda

## Infrastructure as Code (Terraform)

The entire project must be deployable via a single `terraform apply`, including:

- **Networking:** VPC, subnets, and security groups (ensuring Lambda can reach QuestDB)
- **Database:** A QuestDB instance (running on EC2 or via a container on ECS/Fargate)
- **Compute:** AWS Lambda function, IAM roles, and webhook trigger
- **Secrets:** Secure handling of FXStreet API credentials

## Evaluation Criteria

We are looking for engineering maturity, not just functional code. Evaluation will focus on:

- **Readability & patterning:** Clean Rust code, effective error handling (e.g., `anyhow` / `thiserror`), and proper crate organization
- **Resiliency:** Handling API rate limits, database downtime, and retries
- **Observability:** Clear logging and status reporting
- **Terraform quality:** Proper use of variables, outputs, and resource tagging
- **Efficiency:** Optimized serialization/deserialization (`serde`) and connection management

## Deliverables

- **Source code:** A Git repository containing the Rust project and Terraform files
- **README.md** with clear instructions on:
  - How to configure FXStreet credentials
  - How to deploy the infrastructure
  - How to execute the backfill process

## Reference Links

- [FXStreet API](https://docs.fxstreet.com/api/calendar/)
- [QuestDB Rust Client](https://github.com/questdb/questdb-rs)
- [AWS Lambda Rust Runtime](https://github.com/awslabs/aws-lambda-rust-runtime)
