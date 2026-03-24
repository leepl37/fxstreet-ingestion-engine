variable "aws_region" {
  description = "AWS Region to deploy to"
  type        = string
  default     = "ap-northeast-2" # Seoul Region
}

variable "project_name" {
  description = "Project name to be used for resources tagging"
  type        = string
  default     = "fxstreet-ingestion"
}

variable "environment" {
  description = "Deployment environment (e.g., dev, staging, prod)"
  type        = string
  default     = "dev"
}

variable "owner" {
  description = "Owner of the resources"
  type        = string
  default     = "data-team"
}

variable "webhook_secret_token" {
  description = "Secret token used by the Lambda to validate incoming structural FXStreet webhooks"
  type        = string
  sensitive   = true
}

variable "fxstreet_bearer_token" {
  description = "Optional FXStreet API bearer token. Required for non-test webhook/CLI real calls."
  type        = string
  default     = ""
  sensitive   = true
}

variable "ec2_instance_type" {
  description = "EC2 instance type for QuestDB. t3.micro is eligible for free tier in many regions."
  type        = string
  default     = "t3.micro"
}

variable "ssh_key_name" {
  description = "Optional: Name of an existing AWS SSH Key pair to access the QuestDB instance"
  type        = string
  default     = ""
}

variable "lambda_alarm_actions" {
  description = "Optional list of SNS topic ARNs for Lambda CloudWatch alarms."
  type        = list(string)
  default     = []
}

variable "admin_allowed_cidrs" {
  description = "Allowed CIDR blocks for QuestDB web console (9000) and SSH (22)."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}
