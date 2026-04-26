#!/bin/bash
# MEXC Ghost Hunter - Automated VPS Installer
# Supports: Ubuntu 20.04+, Debian 11+

set -e

echo "═══════════════════════════════════════════════════════"
echo "  MEXC Ghost Hunter - Automated VPS Installer"
echo "═══════════════════════════════════════════════════════"
echo ""

# Check if running as root
if [[ $EUID -ne 0 ]]; then
   echo "❌ This script must be run as root (use: sudo ./install.sh)"
   exit 1
fi

# Detect OS
if [ -f /etc/os-release ]; then
    . /etc/os-release
    OS=$ID
else
    echo "❌ Cannot detect operating system"
    exit 1
fi

echo "✅ Detected OS: $OS $VERSION_ID"

# Update package manager
echo ""
echo "📦 Updating package manager..."
apt-get update > /dev/null 2>&1
apt-get install -y curl git > /dev/null 2>&1
echo "✅ Package manager updated"

# Install Docker
echo ""
echo "🐳 Installing Docker..."
if ! command -v docker &> /dev/null; then
    curl -fsSL https://get.docker.com -o get-docker.sh
    sh get-docker.sh > /dev/null 2>&1
    rm get-docker.sh
    echo "✅ Docker installed"
else
    echo "✅ Docker already installed"
fi

# Install Docker Compose
echo ""
echo "🐳 Installing Docker Compose..."
if ! command -v docker-compose &> /dev/null; then
    curl -L "https://github.com/docker/compose/releases/latest/download/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
    chmod +x /usr/local/bin/docker-compose
    echo "✅ Docker Compose installed"
else
    echo "✅ Docker Compose already installed"
fi

# Clone repository
echo ""
echo "📥 Cloning MEXC Ghost Hunter repository..."
if [ ! -d "/opt/mexc" ]; then
    git clone https://github.com/MatrixYAS/MEXC.git /opt/mexc
    echo "✅ Repository cloned to /opt/mexc"
else
    echo "✅ Repository already exists at /opt/mexc"
    cd /opt/mexc && git pull
    echo "✅ Repository updated"
fi

# Setup data directory
echo ""
echo "📁 Setting up data directory..."
mkdir -p /opt/mexc/data /opt/mexc/backups
chown -R nobody:nogroup /opt/mexc/data /opt/mexc/backups
echo "✅ Data directory created"

# Setup .env file
echo ""
echo "🔐 Setting up environment configuration..."
if [ ! -f "/opt/mexc/.env" ]; then
    cp /opt/mexc/.env.example /opt/mexc/.env
    echo "✅ Created .env from template"
    echo ""
    echo "⚠️  IMPORTANT: Edit /opt/mexc/.env and set ADMIN_PASSWORD"
    echo "   Command: nano /opt/mexc/.env"
    echo ""
else
    echo "✅ .env file already exists"
fi

# Setup firewall
echo ""
echo "🔥 Configuring firewall (UFW)..."
if ! command -v ufw &> /dev/null; then
    apt-get install -y ufw > /dev/null 2>&1
fi

if ! ufw status | grep -q "Status: active"; then
    ufw --force enable > /dev/null 2>&1
    ufw default deny incoming > /dev/null 2>&1
    ufw default allow outgoing > /dev/null 2>&1
    echo "✅ UFW enabled"
else
    echo "✅ UFW already enabled"
fi

# Allow ports
ufw allow 22/tcp > /dev/null 2>&1
ufw allow 80/tcp > /dev/null 2>&1
ufw allow 443/tcp > /dev/null 2>&1
echo "✅ Firewall rules configured (SSH, HTTP, HTTPS)"

# Build Docker image
echo ""
echo "🔨 Building Docker image (this may take 5-10 minutes)..."
cd /opt/mexc
docker-compose -f docker-compose.prod.yml build 2>&1 | tail -20
echo "✅ Docker image built"

# Start application
echo ""
echo "🚀 Starting MEXC Ghost Hunter..."
cd /opt/mexc
docker-compose -f docker-compose.prod.yml up -d
sleep 5
echo "✅ Application started"

# Verify
echo ""
echo "🔍 Verifying installation..."
if docker exec mexc-ghost-hunter curl -s http://127.0.0.1:8080/api/health > /dev/null; then
    echo "✅ Health check passed"
else
    echo "⚠️  Health check failed. Check logs: docker logs mexc-ghost-hunter"
fi

# Summary
echo ""
echo "═══════════════════════════════════════════════════════"
echo "  ✅ Installation Complete!"
echo "═══════════════════════════════════════════════════════"
echo ""
echo "📝 Next Steps:"
echo ""
echo "1. Edit environment configuration:"
echo "   sudo nano /opt/mexc/.env"
echo "   ⚠️  Set a strong ADMIN_PASSWORD"
echo ""
echo "2. Restart application (after editing .env):"
echo "   cd /opt/mexc"
echo "   docker-compose -f docker-compose.prod.yml restart"
echo ""
echo "3. Check status:"
echo "   docker logs -f mexc-ghost-hunter"
echo ""
echo "4. Access application:"
echo "   http://localhost:8080"
echo ""
echo "5. (Optional) Setup Nginx + SSL:"
echo "   See DEPLOYMENT.md for instructions"
echo ""
echo "📚 Documentation: /opt/mexc/DEPLOYMENT.md"
echo ""
echo "═══════════════════════════════════════════════════════"
echo ""
