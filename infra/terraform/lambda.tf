# 1. SSM Parameter to securely store FXStreet Webhook Token in AWS
resource "aws_ssm_parameter" "webhook_secret" {
  name        = "/${var.project_name}/${var.environment}/webhook_secret_token"
  description = "Secret token for FXStreet Webhook Lambda to authenticate requests"
  type        = "SecureString"
  value       = var.webhook_secret_token
}

# 2. IAM Role for Lambda
data "aws_iam_policy_document" "lambda_assume_role" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "lambda_role" {
  name               = "${var.project_name}-lambda-role"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume_role.json
}

# Standard Lambda execution policy (for CloudWatch logs)
resource "aws_iam_role_policy_attachment" "lambda_basic_execution" {
  role       = aws_iam_role.lambda_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

# Give Lambda access to read SSM parameters (if needed by code dynamically)
data "aws_iam_policy_document" "lambda_ssm" {
  statement {
    actions   = ["ssm:GetParameter"]
    resources = [aws_ssm_parameter.webhook_secret.arn]
  }
}

resource "aws_iam_role_policy" "lambda_ssm_access" {
  name   = "${var.project_name}-ssm-access"
  role   = aws_iam_role.lambda_role.id
  policy = data.aws_iam_policy_document.lambda_ssm.json
}

# 3. Create the Deployment Package (zip file)
# Note: Users MUST run `cargo lambda build --release --arm64` before terraform apply.
data "archive_file" "lambda_zip" {
  type        = "zip"
  source_file = "${path.module}/../../target/lambda/lambda/bootstrap"
  output_path = "${path.module}/lambda.zip"
}

# 4. Lambda Function deployment
resource "aws_lambda_function" "webhook_lambda" {
  filename         = data.archive_file.lambda_zip.output_path
  source_code_hash = data.archive_file.lambda_zip.output_base64sha256
  function_name    = "${var.project_name}-webhook"
  role             = aws_iam_role.lambda_role.arn
  handler          = "bootstrap"       # Required for provided runtime
  runtime          = "provided.al2023" # Custom Rust runtime built into AL2023
  architectures    = ["arm64"]
  timeout          = 30

  environment {
    variables = {
      RUST_LOG             = "info"
      # Switch to "real" once FXStreet bearer token is available.
      # For testing without changing this, use X-Test-Mode: true header.
      FXSTREET_MODE        = "real"
      # FXSTREET_BEARER_TOKEN = "<add when token received from FXStreet>"
      FXSTREET_API_BASE    = "https://calendar-api.fxstreet.com/en/api/v1"
      QUESTDB_HOST         = aws_instance.questdb.public_ip
      QUESTDB_ILP_PORT     = "9009"
      WEBHOOK_SECRET_TOKEN = var.webhook_secret_token
    }
  }
}

# 5. Lambda Function URL (Public Endpoint with no IAM Auth, protected by our Secret Token header)
resource "aws_lambda_function_url" "webhook_url" {
  function_name      = aws_lambda_function.webhook_lambda.function_name
  authorization_type = "NONE"
}

resource "aws_lambda_permission" "public_function_url" {
  statement_id           = "FunctionURLAllowPublicAccess"
  action                 = "lambda:InvokeFunctionUrl"
  function_name          = aws_lambda_function.webhook_lambda.function_name
  principal              = "*"
  function_url_auth_type = "NONE"
}
