# VPC
resource "aws_vpc" "main" {
  cidr_block           = "10.0.0.0/16"
  enable_dns_support   = true
  enable_dns_hostnames = true
  tags = {
    Name = "${var.project_name}-vpc"
  }
}

# Public Subnet
resource "aws_subnet" "public" {
  vpc_id                  = aws_vpc.main.id
  cidr_block              = "10.0.1.0/24"
  map_public_ip_on_launch = true
  availability_zone       = "${var.aws_region}a"
  tags = {
    Name = "${var.project_name}-public-subnet"
  }
}

# Internet Gateway
resource "aws_internet_gateway" "igw" {
  vpc_id = aws_vpc.main.id
  tags = {
    Name = "${var.project_name}-igw"
  }
}

# Route Table for Public Subnet
resource "aws_route_table" "public_rt" {
  vpc_id = aws_vpc.main.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.igw.id
  }
  tags = {
    Name = "${var.project_name}-public-rt"
  }
}

resource "aws_route_table_association" "public_rta" {
  subnet_id      = aws_subnet.public.id
  route_table_id = aws_route_table.public_rt.id
}

# Security Group for QuestDB EC2
resource "aws_security_group" "questdb_sg" {
  name        = "${var.project_name}-questdb-sg"
  description = "Allow required inbound traffic for QuestDB"
  vpc_id      = aws_vpc.main.id

  ingress {
    description = "QuestDB HTTP Web Console"
    from_port   = 9000
    to_port     = 9000
    protocol    = "tcp"
    # Restrict admin surface (web console) via terraform variable.
    cidr_blocks = var.admin_allowed_cidrs
  }

  ingress {
    description = "QuestDB ILP (Ingestion)"
    from_port   = 9009
    to_port     = 9009
    protocol    = "tcp"
    # Currently open to allow Lambda (outside VPC) access via public IP.
    # Production: move Lambda into VPC and restrict to "10.0.0.0/16" only.
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "SSH Access"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    # Restrict SSH to operator CIDRs.
    cidr_blocks = var.admin_allowed_cidrs
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}
