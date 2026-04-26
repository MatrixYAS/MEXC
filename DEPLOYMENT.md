# MEXC Ghost Hunter - VPS Deployment Guide

> **Complete guide for deploying MEXC Ghost Hunter to production VPS**

---

## 📋 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Quick Start (Automated)](#quick-start-automated)
3. [Manual Installation](#manual-installation)
4. [Configuration](#configuration)
5. [Nginx Reverse Proxy + SSL](#nginx-reverse-proxy--ssl)
6. [Monitoring & Maintenance](#monitoring--maintenance)
7. [Backup & Recovery](#backup--recovery)
8. [Troubleshooting](#troubleshooting)

---

## Prerequisites

### VPS Requirements
- **OS**: Ubuntu 20.04+ or Debian 11+
- **CPU**: 2 cores minimum
- **RAM**: 512MB minimum (1GB recommended)
- **Storage**: 10GB minimum
- **Network**: Static IP (recommended)
- **Ports**: 22 (SSH), 80 (HTTP), 443 (HTTPS)

### Software Requirements
- Docker (installed via script)
- Docker Compose v2+ (installed via script)
- Git (usually pre-installed)

---

## Quick Start (Automated)

### 1. SSH to VPS
```bash
ssh root@your-vps-ip
```

### 2. Run Installer
```bash
curl -O https://raw.githubusercontent.com/MatrixYAS/MEXC/main/scripts/install.sh
chmod +x install.sh
sudo ./install.sh
```

### 3. Set Admin Password
```bash
cd /opt/mexc
nano .env
# Edit ADMIN_PASSWORD to something strong
# Ctrl+X, Y, Enter to save
```

### 4. Start Application
```bash
docker-compose -f docker-compose.prod.yml up -d
```

### 5. Verify
```bash
curl http://127.0.0.1:8080/api/health
```

✅ **Done! App is running.**

---

## Manual Installation

### 1. Update System
```bash
sudo apt update && sudo apt upgrade -y
```

### 2. Install Docker
```bash
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh
sudo usermod -aG docker $USER
```

### 3. Install Docker Compose
```bash
sudo curl -L "https://github.com/docker/compose/releases/latest/download/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
sudo chmod +x /usr/local/bin/docker-compose
docker-compose --version
```

### 4. Clone Repository
```bash
cd /opt
sudo git clone https://github.com/MatrixYAS/MEXC.git
cd MEXC
sudo chown -R $USER:$USER .
```

### 5. Setup Data Directory
```bash
mkdir -p /opt/mexc/data
chmod 755 /opt/mexc/data
```

### 6. Configure Environment
```bash
cp .env.example .env
nano .env
# Edit ADMIN_PASSWORD (minimum 8 characters, use: openssl rand -base64 24)
```

### 7. Build & Deploy
```bash
docker-compose -f docker-compose.prod.yml build
docker-compose -f docker-compose.prod.yml up -d
```

### 8. Monitor Startup
```bash
docker logs -f mexc-ghost-hunter
# Wait for: ✅ Server listening on http://0.0.0.0:8080
# Press Ctrl+C to exit
```

### 9. Verify Health
```bash
curl http://127.0.0.1:8080/api/health
# Should return: {"status":"healthy","uptime_ms":...
```

---

## Configuration

### Edit `.env` File
```bash
cd /opt/mexc
nano .env
```

### Important Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ADMIN_PASSWORD` | **REQUIRED** | Strong password for login |
| `PORT` | 8080 | Internal port (keep 8080 for nginx) |
| `RUST_LOG` | info | Log level: trace, debug, info, warn, error |
| `MIN_PROFIT_THRESHOLD` | 0.0015 | Minimum 0.15% profit to report |
| `TARGET_VOLUME_USD` | 1000 | Volume per leg simulation |
| `MIN_VOLUME_24H` | 500000 | Minimum coin volume filter |

### Apply Changes
```bash
cd /opt/mexc
docker-compose -f docker-compose.prod.yml up -d
docker logs -f mexc-ghost-hunter
```

---

## Nginx Reverse Proxy + SSL

### 1. Install Nginx
```bash
sudo apt install -y nginx
```

### 2. Install Certbot (Let's Encrypt SSL)
```bash
sudo apt install -y certbot python3-certbot-nginx
```

### 3. Create Nginx Config
```bash
sudo nano /etc/nginx/sites-available/mexc
```

**Paste this** (replace `your-domain.com`):
```nginx
server {
    server_name your-domain.com;
    listen 80;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 86400;
    }
}
```

### 4. Enable Site
```bash
sudo ln -s /etc/nginx/sites-available/mexc /etc/nginx/sites-enabled/
sudo nginx -t  # Test config
sudo systemctl restart nginx
```

### 5. Get SSL Certificate
```bash
sudo certbot --nginx -d your-domain.com
# Follow prompts, choose auto-redirect to HTTPS
```

### 6. Auto-Renew SSL
```bash
sudo systemctl enable certbot.timer
sudo systemctl start certbot.timer
```

### 7. Access Application
```
https://your-domain.com
```

---

## Firewall Setup (UFW)

### 1. Enable UFW
```bash
sudo ufw enable
```

### 2. Allow SSH (CRITICAL!)
```bash
sudo ufw allow 22/tcp
```

### 3. Allow HTTP/HTTPS
```bash
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
```

### 4. Check Status
```bash
sudo ufw status
```

Expected:
```
Status: active

To                         Action      From
--                         ------      ----
22/tcp                     ALLOW       Anywhere
80/tcp                     ALLOW       Anywhere
443/tcp                    ALLOW       Anywhere
```

---

## Monitoring & Maintenance

### Check Status
```bash
# Container running?
docker ps | grep mexc

# Recent logs
docker logs mexc-ghost-hunter --tail 50

# Real-time logs
docker logs -f mexc-ghost-hunter

# Health check
curl http://localhost:8080/api/health | jq

# Resource usage
docker stats mexc-ghost-hunter
```

### Restart Application
```bash
cd /opt/mexc
docker-compose -f docker-compose.prod.yml restart
```

### Stop Application
```bash
cd /opt/mexc
docker-compose -f docker-compose.prod.yml down
```

### Update Application
```bash
cd /opt/mexc
git pull
docker-compose -f docker-compose.prod.yml up -d --build
```

---

## Backup & Recovery

### Automated Daily Backup
```bash
# Create backup script
sudo nano /opt/mexc/backup.sh
```

**Paste this**:
```bash
#!/bin/bash
BACKUP_DIR="/opt/mexc/backups"
DATE=$(date +%Y%m%d_%H%M%S)
mkdir -p $BACKUP_DIR

docker exec mexc-ghost-hunter tar czf - /data | gzip > $BACKUP_DIR/mexc_$DATE.tar.gz
echo "Backup created: $BACKUP_DIR/mexc_$DATE.tar.gz"

# Keep only last 30 days
find $BACKUP_DIR -name "mexc_*.tar.gz" -mtime +30 -delete
```

```bash
sudo chmod +x /opt/mexc/backup.sh
```

### Add to Crontab (Daily at 2 AM)
```bash
sudo crontab -e
```

**Add this line**:
```
0 2 * * * /opt/mexc/backup.sh
```

### Manual Backup
```bash
cd /opt/mexc
docker exec mexc-ghost-hunter tar czf - /data > backup_$(date +%Y%m%d).tar.gz
ls -lh backup_*.tar.gz
```

### Restore Backup
```bash
cd /opt/mexc
docker-compose -f docker-compose.prod.yml down
tar xzf backup_20260426.tar.gz
docker-compose -f docker-compose.prod.yml up -d
```

---

## Troubleshooting

### Container won't start
```bash
# Check logs
docker logs mexc-ghost-hunter

# Rebuild
docker-compose -f docker-compose.prod.yml build --no-cache
docker-compose -f docker-compose.prod.yml up -d
```

### Health check failing
```bash
# Test manually
curl http://127.0.0.1:8080/api/health

# Check if port is open
netstat -tuln | grep 8080

# Check app logs
docker logs mexc-ghost-hunter --tail 100
```

### High memory usage
```bash
# Check resource limits
docker stats mexc-ghost-hunter

# Increase in docker-compose.prod.yml if needed:
# deploy.resources.limits.memory: 1G

# Restart with new limits
docker-compose -f docker-compose.prod.yml up -d
```

### Database locked error
```bash
# SQLite can have lock issues. Restart:
docker-compose -f docker-compose.prod.yml restart

# Or rebuild database:
docker-compose -f docker-compose.prod.yml down
rm /opt/mexc/data/mexc.db
docker-compose -f docker-compose.prod.yml up -d
```

### Can't connect via domain
```bash
# Test Nginx config
sudo nginx -t

# Check Nginx logs
sudo tail -f /var/log/nginx/error.log

# Restart Nginx
sudo systemctl restart nginx

# Test connection
curl -v https://your-domain.com
```

### ADMIN_PASSWORD not recognized
```bash
# Make sure .env is set
cat /opt/mexc/.env | grep ADMIN_PASSWORD

# Restart application
docker-compose -f docker-compose.prod.yml down
docker-compose -f docker-compose.prod.yml up -d

# Check startup logs
docker logs mexc-ghost-hunter
```

---

## Support & Resources

- **GitHub Issues**: https://github.com/MatrixYAS/MEXC/issues
- **Docker Docs**: https://docs.docker.com
- **Nginx Docs**: https://nginx.org/en/docs
- **Let's Encrypt**: https://letsencrypt.org

---

**Last updated**: 2026-04-26
