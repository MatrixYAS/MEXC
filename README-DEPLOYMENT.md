# Quick Deployment Guide

## ⚡ 1-Minute Start

### Option 1: Automated (Recommended)
```bash
sudo bash <(curl -s https://raw.githubusercontent.com/MatrixYAS/MEXC/main/scripts/install.sh)
```

### Option 2: Manual
```bash
# SSH to VPS
ssh root@your-vps-ip

# Clone & setup
cd /opt
git clone https://github.com/MatrixYAS/MEXC.git mexc
cd mexc
cp .env.example .env

# EDIT .env - Set strong ADMIN_PASSWORD
nano .env

# Deploy
docker-compose -f docker-compose.prod.yml up -d

# Verify
curl http://127.0.0.1:8080/api/health
```

## 🔗 Access Application

- **Local Network**: `http://vps-ip:8080`
- **With Nginx**: `https://your-domain.com`
- **Login Password**: Set in `.env` as `ADMIN_PASSWORD`

## 📊 Monitor

```bash
# View logs
docker logs -f mexc-ghost-hunter

# Check health
curl http://127.0.0.1:8080/api/health

# Restart
docker-compose -f docker-compose.prod.yml restart
```

## 🔐 Security Essentials

1. ✅ Set strong `ADMIN_PASSWORD` in `.env`
2. ✅ Never commit `.env` to git
3. ✅ Use Nginx + SSL (see DEPLOYMENT.md)
4. ✅ Enable firewall (UFW)
5. ✅ Regular backups

## 📖 Full Guide

See `DEPLOYMENT.md` for complete instructions including:
- Nginx + SSL setup
- Firewall configuration
- Backup procedures
- Troubleshooting

---

**Your app is production-ready!** 🚀
