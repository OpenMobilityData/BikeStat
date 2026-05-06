# BikeStat — Server Deploy Handoff
**Date:** 2026-05-06
**Audience:** the next Claude Code session running on the server, or the user reading directly.

---

## 1. What BikeStat is

A Rust/WebAssembly client-side web app (Leptos 0.7 CSR) that aggregates and visualizes traffic-count data for a small, curated set of locations in Côte-des-Neiges–Notre-Dame-de-Grâce, Montréal. There is **no backend service**: lighttpd serves a `dist/` directory of static files. The repo lives at `git@github.com:OpenMobilityData/BikeStat.git`.

Domain to set up: **bikestat.org** (registered, no DNS pointed yet).

The project memory at `~/.claude/projects/-Users-rhoge-Desktop-BikeStat/memory/` covers stack, file layout, source registry, and conventions. Read it first.

---

## 2. State at handoff

### Done locally
- Build produces `dist/` via `trunk build --release`. Bundle is fingerprint-hashed.
- The VdM cyclistes CSV is now served same-origin from `data/cyclistes.csv` (was upstream URL). The hourly cron script is in `scripts/refresh-vdm.sh` — downloads upstream, applies a coarse `bourret|girouard` regex pre-filter, atomically replaces the served file, writes a freshness string to `data/status.txt`. Filtered payload is ~11 MB raw / **~316 KB gzipped**.
- Telraam Excel files (segments 9794, 10045) live under `static/data/telraam/<id>/<year>.xlsx` — copied verbatim into `dist/data/...` by Trunk's `copy-dir`.
- CDN-NDG eco-counter Excel for Terrebonne @ Kensington at `static/data/cdn-ndg/terrebonne-kensington/2025-07-26_2025-11-15.xlsx`.
- Header shows a quiet "VdM data: …" indicator that reads `data/status.txt`. Validates response shape so SPA-fallback HTML doesn't leak into the UI.
- Latest commit on `main` is `9cc7234`.

### Still to do (this handoff)
1. Decide / find the right server paths and `lighttpd` setup.
2. DNS: point `bikestat.org` (and optionally `www.bikestat.org`) at the server.
3. Configure `lighttpd` virtual host for the domain, with gzip + correct mimetypes.
4. HTTPS via Let's Encrypt.
5. Install `scripts/refresh-vdm.sh` + cron entry.
6. Initial fetch so the app works before the first cron tick.
7. Smoke test, then have testers try it.

---

## 3. Decisions to make first

Before any commands, answer these on the server:

- **Linux distro & version?** (`cat /etc/os-release`) — affects package names + lighttpd config style.
- **Lighttpd version?** (`lighttpd -v`) — `mod_deflate` is the modern compression module (1.4.56+). Older installs use `mod_compress`. Both are covered below.
- **Existing vhosts on this server?** (`ls /etc/lighttpd/conf-enabled/`) — does another site already use port 443? If so, BikeStat shares the cert/listener via SNI.
- **Where do other sites live?** Pick a base path. Common choices: `/var/www/bikestat/` or `/srv/bikestat/`. The doc below uses `/var/www/bikestat/` — substitute as needed.
- **Build on the server, or build locally and rsync?** If the server has Rust + `trunk` already, server-build is fine and lets you `git pull && rebuild`. Otherwise build locally and `rsync dist/`. The handoff covers server-build below since the user mentioned cloning on the server.

---

## 4. Server prerequisites

Install once:

```bash
# Debian/Ubuntu — adjust for other distros
sudo apt update
sudo apt install -y lighttpd curl git build-essential pkg-config libssl-dev

# Rust (rustup, latest stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustup target add wasm32-unknown-unknown

# Trunk + wasm-bindgen-cli (used by trunk)
cargo install --locked trunk
```

Confirm versions:
```bash
rustc --version    # expect 1.75+
trunk --version    # expect 0.21+
lighttpd -v
```

---

## 5. Clone + first build

```bash
sudo mkdir -p /var/www
sudo chown $USER:$USER /var/www                   # so you can git clone here
cd /var/www
git clone git@github.com:OpenMobilityData/BikeStat.git bikestat
cd bikestat

# Populate the dev-side CSV with a one-time fetch (real cron picks up later)
mkdir -p static/data
curl -fsSL --max-time 120 \
    -A "Mozilla/5.0 (BikeStat-cron)" \
    "https://donnees.montreal.ca/dataset/142ff2e9-7d0a-47d6-b4f6-dfeb97041daf/resource/a8e463ab-d334-4714-81d5-8da0310d80c0/download/cyclistes.csv" \
  | { head -1; grep -iE 'bourret|girouard'; } > static/data/cyclistes.csv

# Build
trunk build --release
ls dist/                                          # should contain index.html, *.wasm, *.js, data/, static/
```

The lighttpd document root will point at `/var/www/bikestat/dist`.

> **Note on permissions:** lighttpd runs as `www-data` on Debian-derived distros. It must be able to *read* `/var/www/bikestat/dist` (and traverse parent dirs). It must be able to *write* `dist/data/cyclistes.csv` and `dist/data/status.txt` since the cron writes there. Either run the cron as `www-data`, or `chown -R www-data:www-data dist/data/` and run the cron as a user in that group.

---

## 6. DNS

At your registrar's DNS panel for `bikestat.org`:

| Type  | Name | Value                       | TTL   |
|-------|------|-----------------------------|-------|
| A     | @    | _your server's IPv4_        | 3600  |
| AAAA  | @    | _your server's IPv6_ (opt.) | 3600  |
| CNAME | www  | bikestat.org                | 3600  |

After saving, verify:

```bash
dig +short bikestat.org A
dig +short www.bikestat.org CNAME
```

Both should return your server's IP (or the apex domain for www). Propagation: usually under 5 min, can take up to an hour.

---

## 7. Lighttpd config

### 7a. Make sure the modules are loaded

```bash
sudo lighty-enable-mod accesslog
sudo lighty-enable-mod deflate     # or 'compress' on older lighttpd
sudo lighty-enable-mod redirect
```

### 7b. MIME types — ensure `.wasm` and `.csv` are mapped

Check `/etc/lighttpd/conf-available/05-mimetype.conf` (Debian path may differ). It should include:

```conf
".wasm" => "application/wasm",
".csv"  => "text/csv",
".xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
```

If any are missing, add them.

### 7c. Vhost for bikestat.org

Create `/etc/lighttpd/conf-available/30-bikestat.conf`:

```conf
$HTTP["host"] =~ "^(www\.)?bikestat\.org$" {
    server.document-root = "/var/www/bikestat/dist"

    # Redirect www → apex (HTTPS-aware, see HTTPS section below)
    $HTTP["host"] == "www.bikestat.org" {
        url.redirect = ( "" => "https://bikestat.org${url.path}" )
    }

    # Compression — the VdM CSV is the headline win here.
    deflate.mimetypes = (
        "text/csv",
        "text/plain",
        "text/html",
        "text/css",
        "application/javascript",
        "application/json",
        "application/wasm",
        "image/svg+xml"
    )

    # Cache headers: app shell is fingerprinted by trunk so cache aggressively;
    # data files refresh hourly so allow short cache.
    $HTTP["url"] =~ "^/(.*\.(wasm|js|css)|favicon-)" {
        setenv.add-response-header = ( "Cache-Control" => "public, max-age=31536000, immutable" )
    }
    $HTTP["url"] =~ "^/data/" {
        setenv.add-response-header = ( "Cache-Control" => "public, max-age=300" )
    }
}
```

> Note: do NOT add an SPA `index.html` fallback. BikeStat is a single-route app; missing files should return 404 so the freshness-string fetch fails cleanly when the cron hasn't run yet.

Enable and test:

```bash
sudo ln -sf ../conf-available/30-bikestat.conf /etc/lighttpd/conf-enabled/
sudo lighttpd -tt -f /etc/lighttpd/lighttpd.conf
sudo systemctl reload lighttpd
```

### 7d. First HTTP smoke test

```bash
curl -I http://bikestat.org/
# expect 200 OK, Content-Type: text/html
curl -I http://bikestat.org/data/cyclistes.csv
# expect 200 OK, Content-Encoding: gzip (if Accept-Encoding sent)
```

Open `http://bikestat.org/` in a browser — chart should render with Telraam + CDN-NDG immediately, VdM after a couple seconds.

---

## 8. HTTPS via Let's Encrypt

Lighttpd's ACME story is awkward — easiest path is `certbot` with the standalone or webroot plugin.

```bash
sudo apt install -y certbot
sudo certbot certonly --webroot -w /var/www/bikestat/dist \
    -d bikestat.org -d www.bikestat.org \
    --agree-tos -m you@example.com
```

Combine cert + key for lighttpd (it wants a single PEM):

```bash
sudo mkdir -p /etc/lighttpd/ssl
sudo bash -c 'cat /etc/letsencrypt/live/bikestat.org/privkey.pem \
                  /etc/letsencrypt/live/bikestat.org/fullchain.pem \
              > /etc/lighttpd/ssl/bikestat.pem'
sudo chmod 600 /etc/lighttpd/ssl/bikestat.pem
```

Add HTTPS config — append to `/etc/lighttpd/conf-available/30-bikestat.conf`:

```conf
$SERVER["socket"] == ":443" {
    ssl.engine                  = "enable"
    ssl.pemfile                 = "/etc/lighttpd/ssl/bikestat.pem"
    ssl.ca-file                 = "/etc/letsencrypt/live/bikestat.org/chain.pem"
    ssl.openssl.ssl-conf-cmd    = ("MinProtocol" => "TLSv1.2",
                                    "Options" => "-SessionTicket")
    # All the same vhost rules apply via SNI matching:
    $HTTP["host"] =~ "^(www\.)?bikestat\.org$" {
        server.document-root = "/var/www/bikestat/dist"
        # … re-paste the deflate.mimetypes + cache-control rules from above,
        #   or factor into an include file.
    }
}

# Force HTTP → HTTPS for bikestat.org
$HTTP["scheme"] == "http" {
    $HTTP["host"] =~ "^(www\.)?bikestat\.org$" {
        url.redirect = ( "" => "https://bikestat.org${url.path}" )
    }
}
```

Then:
```bash
sudo lighttpd -tt -f /etc/lighttpd/lighttpd.conf
sudo systemctl reload lighttpd
```

Test:
```bash
curl -I https://bikestat.org/
curl -I http://bikestat.org/    # should be 301 → https
```

Auto-renew: `certbot` installs a systemd timer by default. Verify with `systemctl list-timers | grep certbot`. After each renewal you must rebuild `/etc/lighttpd/ssl/bikestat.pem` — easiest is a deploy hook:

```bash
sudo tee /etc/letsencrypt/renewal-hooks/deploy/lighttpd-bikestat.sh <<'EOF'
#!/usr/bin/env bash
set -e
cat /etc/letsencrypt/live/bikestat.org/privkey.pem \
    /etc/letsencrypt/live/bikestat.org/fullchain.pem \
  > /etc/lighttpd/ssl/bikestat.pem
chmod 600 /etc/lighttpd/ssl/bikestat.pem
systemctl reload lighttpd
EOF
sudo chmod +x /etc/letsencrypt/renewal-hooks/deploy/lighttpd-bikestat.sh
```

---

## 9. Install the cron job

```bash
sudo install -m 0755 -o www-data -g www-data \
    /var/www/bikestat/scripts/refresh-vdm.sh \
    /opt/bikestat/refresh-vdm.sh

sudo tee /etc/cron.d/bikestat-vdm >/dev/null <<'EOF'
# Refresh BikeStat VdM CSV at minute 7 every hour.
BIKESTAT_DATA_DIR=/var/www/bikestat/dist/data
7 * * * * www-data /opt/bikestat/refresh-vdm.sh >> /var/log/bikestat-refresh.log 2>&1
EOF

# Ensure the data dir is writable by www-data
sudo chown -R www-data:www-data /var/www/bikestat/dist/data
sudo chmod 0755 /var/www/bikestat/dist/data

# Trigger one immediate run to populate status.txt
sudo -u www-data BIKESTAT_DATA_DIR=/var/www/bikestat/dist/data \
    /opt/bikestat/refresh-vdm.sh
sudo tail /var/log/bikestat-refresh.log    # may not exist yet if first run was clean
ls -la /var/www/bikestat/dist/data/
```

You should now see `cyclistes.csv` (~11 MB) and `status.txt` (a one-liner with the timestamp).

---

## 10. Smoke-test checklist

After everything's wired, in a fresh browser tab:

- [ ] https://bikestat.org/ renders the chart layout, dark theme.
- [ ] Telraam + CDN-NDG sources appear in the sidebar within ~2s.
- [ ] VdM Bourret/Girouard sources appear within ~5s.
- [ ] The header shows a "VdM data: 2026-05-06 …" string at top right.
- [ ] Clicking through preset buttons (All dates, 2025, Year-on-Year) updates the chart.
- [ ] Open dev-tools → Network → reload. The largest payload should be the `*.wasm` (~1–2 MB compressed). `cyclistes.csv` should show ~316 KB transferred with `Content-Encoding: gzip`.
- [ ] `curl -I https://bikestat.org/data/cyclistes.csv` responds with `Last-Modified` reflecting the most recent cron tick.
- [ ] After a fresh hour, `Last-Modified` advances and the in-page "VdM data: …" label updates after a reload.

---

## 11. Ongoing maintenance

### Updating the app
```bash
cd /var/www/bikestat
git pull
trunk build --release
# dist/ is now refreshed. lighttpd serves it directly — no reload needed.
# (Cache-Control: immutable on fingerprinted assets means clients pick up
#  the new wasm/js automatically when index.html references new hashes.)
```

If the rebuild blows away the gitignored `dist/data/cyclistes.csv` and `dist/data/status.txt`, the next cron tick (within an hour) restores them. If you want them back immediately:

```bash
sudo -u www-data BIKESTAT_DATA_DIR=/var/www/bikestat/dist/data \
    /opt/bikestat/refresh-vdm.sh
```

For Telraam / CDN-NDG xlsx — those live under `static/data/` and ship with each `trunk build`, so just commit the new file in the repo, push, pull, rebuild.

### Adding a new VdM location
Update `MONTREAL_LOCATION_FILTER` in `src/data/sources.rs` *and* add the corresponding street name(s) to `FILTER_RE` at the top of `scripts/refresh-vdm.sh`. The shell filter must remain a superset of the parser filter; if it's narrower, the new location will load empty.

### Logs
- App access: `/var/log/lighttpd/access.log` (whatever lighttpd's accesslog module writes).
- Cron: `/var/log/bikestat-refresh.log` (per the cron entry above).

---

## 12. Loose ends to revisit later

- The Telraam compass-direction strings are still placeholder `A→B`/`B→A`. Look up segments 9794 and 10045 on the Telraam segment map and update `TELRAAM_ANNOTATIONS` in `src/data/sources.rs`.
- No analytics. Add `mod_accesslog` parsing or a small JS pixel later if you want hit counts.
- No structured backend. If usage grows or VdM/Telraam APIs become necessary, the workspace-split + ingest-binary architecture is sketched in chat history (search "workspace split"); not needed now.

---

## 13. If something breaks

1. `sudo lighttpd -tt -f /etc/lighttpd/lighttpd.conf` — config validation.
2. `sudo journalctl -u lighttpd -n 100` — recent server logs.
3. `tail /var/log/bikestat-refresh.log` — cron output.
4. From a client, dev tools → Network — look at status codes and content types.
5. Verify the cron-written files exist and are non-empty:
   ```bash
   ls -la /var/www/bikestat/dist/data/cyclistes.csv /var/www/bikestat/dist/data/status.txt
   ```
6. If `status.txt` shows raw HTML in the browser, lighttpd is doing an SPA fallback for missing files — check the vhost config does **not** include any `url.rewrite-if-not-file → /index.html` rule.
