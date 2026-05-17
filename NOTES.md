Here is a practical deploy checklist for running this on a VPS. Because kakimail is a **TCP mail daemon** (not an HTTP app), Caddy cannot HTTP-reverse-proxy SMTP/IMAP traffic. You run kakimail directly on the mail ports and use Caddy **alongside** it for any HTTPS web frontend (webmail, admin dashboard, etc.).

---

### 1. DNS records you need

| Record | Host | Value / Target | Notes |
|--------|------|----------------|-------|
| **A** | `mail.example.com` | `<your VPS IPv4>` | Also add **AAAA** if you have IPv6. |
| **MX** | `example.com` (or `@`) | `10 mail.example.com` | Where the internet sends mail for your domain. |
| **PTR** | `<your VPS IPv4 reversed>` | `mail.example.com` | Set this with your VPS provider (Hetzner, DigitalOcean, etc.). Without reverse DNS, outbound mail often gets rejected. |
| **TXT** | `example.com` | `v=spf1 a:mail.example.com mx ~all` | Authorizes your VPS to send mail for the domain. |
| **TXT** | `_dmarc.example.com` | `v=DMARC1; p=quarantine; rua=mailto:admin@example.com` | Tells receivers what to do if SPF/DKIM fail. |
| **TXT** | `default._domainkey.example.com` | `v=DKIM1; k=rsa; p=<pubkey>...` | **Important:** kakimail currently **validates** incoming DKIM but does **not sign** outgoing mail yet. If you want to send to Gmail/etc. reliably, you still need to set this manually (or add signing later). |

---

### 2. VPS prerequisites

You need a machine with a static IP and ports 25/587/465/143/993 open.

```bash
# Debian/Ubuntu example
sudo apt update
sudo apt install -y sqlite3 libsqlite3-mod-pcre build-essential pkg-config libssl-dev
```

**Why `libsqlite3-mod-pcre`?** `database.rs` loads `/usr/lib/sqlite3/pcre.so`. If that extension is missing the server will crash on startup.

If your provider blocks port 25 (AWS, GCP, Azure, etc.), open a support ticket to unblock **outbound** port 25, or use a relay.

---

### 3. Build and run

```bash
# On the VPS (or build locally and scp the binary up)
git clone <repo> && cd kakimail
cargo build --release

# Set env vars (use a systemd service file or .env)
export SQLITE_URL="/var/lib/kakimail/kakimail.db"
export PORKBUN_API_KEY="..."
export PORKBUN_SECRET_API_KEY="..."
# Optional: if you have a Rust web frontend, build it too

./target/release/kakimail 0.0.0.0 25 587 143 993 465 mail.example.com
```

Arguments are:  
`SMTP_PORT SUBMISSION_PORT IMAP_PORT IMAPS_PORT SMTPS_PORT DOMAIN`

---

### 4. Systemd service

`/etc/systemd/system/kakimail.service`

```ini
[Unit]
Description=Kakimail mail server
After=network.target

[Service]
Type=simple
User=kakimail
Group=kakimail
WorkingDirectory=/opt/kakimail
Environment=SQLITE_URL=/var/lib/kakimail/kakimail.db
Environment=PORKBUN_API_KEY=...
Environment=PORKBUN_SECRET_API_KEY=...
Environment=RUST_LOG=info
ExecStart=/opt/kakimail/target/release/kakimail 0.0.0.0 25 587 143 993 465 mail.example.com
Restart=always

[Install]
WantedBy=multi-user.target
```

```bash
sudo useradd -r -s /bin/false kakimail
sudo mkdir -p /var/lib/kakimail /opt/kakimail
sudo chown kakimail:kakimail /var/lib/kakimail
sudo systemctl daemon-reload
sudo systemctl enable --now kakimail
```

---

### 5. Firewall

```bash
sudo ufw default deny incoming
sudo ufw allow 22/tcp
sudo ufw allow 25/tcp
sudo ufw allow 587/tcp
sudo ufw allow 465/tcp
sudo ufw allow 143/tcp
sudo ufw allow 993/tcp
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw enable
```

---

### 6. Where Caddy fits in

Caddy is an **HTTP/HTTPS** server. It cannot speak SMTP or IMAP, so you do **not** put kakimail “behind” Caddy in the usual reverse-proxy sense. Instead, run them side-by-side:

- **kakimail** binds directly to `0.0.0.0:25/587/465/143/993`.
- **Caddy** binds to `0.0.0.0:80/443` and serves your webmail app, admin dashboard, or static site.

#### Example Caddyfile

If you have a web frontend (e.g. a Svelte/React app) listening on `localhost:3000`:

```caddy
webmail.example.com {
    reverse_proxy localhost:3000
}
```

If you just want Caddy to serve a static landing page:

```caddy
mail.example.com {
    root * /var/www/mail
    file_server
}
```

#### If you *really* want Caddy in front of the mail ports

You would need the **layer-4 plugin** (`caddy-l4`) to do raw TCP proxying. This is usually unnecessary for a single VPS, but if you must:

1. Build Caddy with `xcaddy` and the `caddy-l4` module.
2. Proxy TCP ports internally, e.g. `0.0.0.0:25` -> `localhost:2525`, and run kakimail on the internal ports.

Unless you are trying to share one IP across multiple mail servers, it is simpler to let kakimail own those ports directly.

---

### 7. TLS / certificates

Right now kakimail fetches its own certificate from **Porkbun** on startup via the API. If you want to use **Caddy-managed Let’s Encrypt** certificates instead, you would need to edit `src/main.rs` to load a local PEM file instead of calling the Porkbun API. A quick workaround is to keep using Porkbun for the mail certs and let Caddy manage certs only for the web domain.

---

### 8. Quick sanity check after deploy

From your local machine:

```bash
# SMTP
openssl s_client -connect mail.example.com:465 -starttls smtp </dev/null

# IMAP
openssl s_client -connect mail.example.com:993
```

And test sending/receiving with the scripts in `python-test/`.

If you want, I can generate a `docker-compose.yml` or a Caddy-with-`caddy-l4` TCP proxy config next.
