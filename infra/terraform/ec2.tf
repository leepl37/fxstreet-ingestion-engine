data "aws_ami" "amazon_linux_2023" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-2023.*-x86_64"]
  }
}

resource "aws_instance" "questdb" {
  ami           = data.aws_ami.amazon_linux_2023.id
  instance_type = var.ec2_instance_type
  subnet_id     = aws_subnet.public.id
  key_name      = var.ssh_key_name != "" ? var.ssh_key_name : null

  vpc_security_group_ids = [aws_security_group.questdb_sg.id]

  # User Data script that automatically installs Docker and runs QuestDB on boot
  user_data = <<-EOF
              #!/bin/bash
              dnf update -y
              dnf install -y docker
              systemctl enable docker
              systemctl start docker
              usermod -aG docker ec2-user

              # Pull and run the QuestDB container
              docker run -d \
                --name questdb \
                -p 9000:9000 \
                -p 9009:9009 \
                -p 8812:8812 \
                -p 9003:9003 \
                --restart always \
                questdb/questdb:latest
              EOF

  # Ensure the instance is replaced if the user data script changes
  user_data_replace_on_change = true

  tags = {
    Name = "${var.project_name}-questdb"
  }
}
