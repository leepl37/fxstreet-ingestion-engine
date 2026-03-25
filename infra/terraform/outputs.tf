output "questdb_public_ip" {
  description = "The public IP address of the newly provisioned QuestDB server"
  value       = aws_instance.questdb.public_ip
}

output "questdb_web_console_url" {
  description = "Click here to open the QuestDB Web Console"
  value       = "http://${aws_instance.questdb.public_ip}:9000"
}

output "webhook_lambda_public_url" {
  description = "The Public Function URL of the Webhook Lambda. Provide this URL to FXStreet!"
  value       = aws_lambda_function_url.webhook_url.function_url
}

output "webhook_lambda_errors_alarm_name" {
  description = "CloudWatch alarm name for webhook Lambda Errors metric."
  value       = aws_cloudwatch_metric_alarm.webhook_lambda_errors.alarm_name
}

output "webhook_lambda_throttles_alarm_name" {
  description = "CloudWatch alarm name for webhook Lambda Throttles metric."
  value       = aws_cloudwatch_metric_alarm.webhook_lambda_throttles.alarm_name
}
