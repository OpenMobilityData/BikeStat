# BikeStat — Server Deploy Playbook (As-Built)
**Deployed:** 2026-05-06 to `bikestat.org` on a small Ubuntu 24.04 VPS.
**Audience:** the next person (or Claude session) doing this from scratch.

This is the as-built version of the deploy. It records the path that actually
worked, including the gotchas we hit and the choices we ended up making
*differently* from the original plan. The git history of this file shows the
planning-vs-actual diff if you're curious.

---

## 1. What BikeStat is

A Rust/WebAssembly client-side web app (Leptos 0.7 CSR) that aggregates and
visualizes traffic-count data for a small, curated set of locations in
Côte-des-Neiges–Notre-Dame-de-Grâce, Montréal. There is **no backend service**:
lighttpd serves a `dist/` directory of static files. The repo lives at
`git@github.com:OpenMobilityData/BikeStat.git` (public, HTTPS clone works
without auth). Domain: **bikestat.org** (Namecheap, BasicDNS).

The project memory at `~/.claude/projects/-Users-rhoge-Desktop-BikeStat/memory/`
covers stack, file layout, source registry, and conventions. Read it first if
you'll be modifying the app.

---

## 2. Architecture decisions made during deploy

These are the decisions we landed on after running into reality. Written down
because they affect every step that follows.

- **Build on the developer's Mac, not on the server.** The server is a small
  VPS with limited RAM. `cargo install --locked trunk` swap-thrashed and
  effectively froze the box for 10+ minutes before we Ctrl-C'd. Don't repeat
  that. Build with `trunk build --release` on a beefy machine, ship `dist/`
  via rsync.
- **Repo on the server lives at `~/GitHub/BikeStat/`** — only for the cron
  script (`scripts/refresh-vdm.sh`) and to track future updates with
  `git pull`. **No Rust toolchain on the server.**
- **Document root is `/var/www/bikestat/`** (not `/var/www/bikestat/dist/`).
  We rsync the *contents* of `dist/` into `/var/www/bikestat/`, owned by the
  deploy user (`rhoge` here).
- **Cron runs as the deploy user, via user crontab** — not as `www-data` from
  `/etc/cron.d/`. The data directory is owned by the deploy user, so a user
  cron writes there without permission gymnastics.
- **Per-vhost `deflate.mimetypes` doesn't override the global list cleanly**
  in lighttpd 1.4.74 — extend the global `20-deflate.conf` list instead.
- **No SPA fallback.** BikeStat is a single-route app; missing files return
  404 so the freshness-string fetch fails cleanly when the cron hasn't run
  yet. (If you add an `url.rewrite-if-not-file → /index.html`, a missing
  `status.txt` causes the entire HTML to be displayed as plain text in the
  app's header — we hit this in dev.)

---

## 3. Server prerequisites

Ubuntu 24.04 LTS (Noble Numbat). **Skip Rust + build tools** — we build on
the Mac.

```bash
sudo apt update
sudo apt install -y lighttpd curl certbot dnsutils tree git
```

Notes:
- `dnsutils` provides `dig`/`host`; not installed by default on Ubuntu 24.04.
- `certbot` for Let's Encrypt; we don't need its lighttpd plugin (we use the
  webroot plugin against the existing lighttpd).
- `git` is usually present, but include it for safety.

---

## 4. DNS (Namecheap-specific)

Important Namecheap-only cleanup — must be done **before** adding A records,
or the apex domain will redirect to a parking page no matter what:

1. Open the domain's "Domain" tab.
2. Under **Redirect Domain**, delete the auto-generated row
   `bikestat.org → http://www.bikestat.org/` (trash icon).
3. Under **Other Domain Settings → Parking Page**, click **TURN OFF**.

Then go to **Advanced DNS → HOST RECORDS** and add:

| Type           | Host  | Value             | TTL       |
|----------------|-------|-------------------|-----------|
| A Record       | `@`   | _your server IPv4_ | Automatic |
| CNAME Record   | `www` | `bikestat.org.`   | Automatic |
| AAAA Record    | `@`   | _your IPv6_ (opt) | Automatic |

Delete any leftover Namecheap defaults pointing `@` or `www` at parking infra.

Get the server IP from the server itself:
```bash
curl -4 ifconfig.me; echo
```

Verify propagation (works from anywhere, including your Mac):
```bash
host bikestat.org
host www.bikestat.org
```
Both should return the server IP. Namecheap propagates in under a minute.

---

## 5. Build on the Mac

```bash
cd ~/Desktop/BikeStat
source ~/.cargo/env       # if trunk isn't on PATH
trunk build --release
ls dist/                  # bikestat-*.wasm  bikestat-*.js  data/  index.html  ...
```

Note: `dist/data/cyclistes.csv` will be present locally (we keep a dev copy
in `static/data/` that Trunk's `copy-dir` propagates). It's gitignored. The
cron will overwrite it on the server, so the bootstrap copy is fine.

---

## 6. Deploy: prep server target + first rsync

On the server (one-time):
```bash
sudo mkdir -p /var/www/bikestat
sudo chown $USER:$USER /var/www/bikestat
```

From the Mac:
```bash
cd ~/Desktop/BikeStat
rsync -av --delete dist/ rhoge@<server-ip-or-hostname>:/var/www/bikestat/
```

Verify on the server:
```bash
ls /var/www/bikestat/
# bikestat-*.wasm  bikestat-*.js  data/  favicon-*.svg  index.html  style-*.css
ls /var/www/bikestat/data/
# cdn-ndg/  cyclistes.csv  telraam/
```

(`status.txt` won't be there yet — the cron writes it on first run.)

---

## 7. Lighttpd configuration

### 7a. Module enables

```bash
sudo lighty-enable-mod deflate
# `lighty-enable-mod redirect` says "unknown module" — workaround in 7c
# (mod_redirect is built-in but Ubuntu doesn't ship a conf-available stub).
```

If your existing lighttpd has another vhost using `setenv.add-response-header`,
also `sudo lighty-enable-mod setenv` to silence the "unknown config-key" warning
(harmless either way).

### 7b. Extend global deflate.mimetypes

`/etc/lighttpd/conf-available/20-deflate.conf` ships with a 4-entry mimetype
list (`text/html`, `text/css`, etc.) — **not** `text/csv` or
`application/wasm`. Extend it.

```bash
sudo nano /etc/lighttpd/conf-available/20-deflate.conf
```

Replace the `deflate.mimetypes = ( ... )` line with:

```
deflate.mimetypes = ( "application/javascript", "text/css", "text/html", "text/plain", "text/csv", "application/json", "application/wasm", "image/svg+xml" )
```

(Per-vhost overrides of `deflate.mimetypes` inside a `$HTTP["host"]` block
do not appear to take effect on lighttpd 1.4.74 — we tried, gzip stayed off.
Setting the list globally works.)

### 7c. The vhost

`mod_redirect` and `mod_openssl` need to be loaded explicitly via
`server.modules += ( ... )` — Ubuntu's `lighty-enable-mod` doesn't have
stubs for them.

```bash
sudo nano /etc/lighttpd/conf-available/30-bikestat.conf
```

Paste exactly:

```
server.modules += ( "mod_redirect" )
server.modules += ( "mod_openssl" )

# HTTP listener: serve ACME challenges (so renewals work over plain HTTP),
# redirect everything else on bikestat.org/www to HTTPS.
$HTTP["scheme"] == "http" {
    $HTTP["host"] =~ "^(www\.)?bikestat\.org$" {
        server.document-root = "/var/www/bikestat"
        $HTTP["url"] !~ "^/\.well-known/acme-challenge/" {
            url.redirect = ( "^/(.*)" => "https://bikestat.org/$1" )
        }
    }
}

# HTTPS listener
$SERVER["socket"] == ":443" {
    ssl.engine  = "enable"
    ssl.pemfile = "/etc/lighttpd/ssl/bikestat.pem"

    # Redirect www → apex over HTTPS
    $HTTP["host"] == "www.bikestat.org" {
        url.redirect = ( "^/(.*)" => "https://bikestat.org/$1" )
    }

    # Apex over HTTPS
    $HTTP["host"] == "bikestat.org" {
        server.document-root = "/var/www/bikestat"
        # Override the global enable from 98-downloads.conf — we don't want
        # the world browsing /data/ via lighttpd's directory listing.
        dir-listing.activate = "disable"
    }
}
```

Save (Ctrl-O, Enter, Ctrl-X).

> **Heredoc warning:** do **not** try to write this file with a `cat <<'EOF'`
> heredoc over SSH. Terminal-paste auto-indent silently puts whitespace before
> the closing `EOF`, which then never matches and the heredoc keeps slurping
> following commands. Use `nano`.

### 7d. Enable, validate, reload

The vhost file is in `conf-available/`. **You must symlink it into
`conf-enabled/` for lighttpd to load it.** (We forgot this once and spent a
while debugging "why is the apex serving the wrong content".)

```bash
sudo ln -sf ../conf-available/30-bikestat.conf /etc/lighttpd/conf-enabled/30-bikestat.conf

sudo lighttpd -tt -f /etc/lighttpd/lighttpd.conf
# expect: only harmless WARNINGs (e.g. about mod_setenv from another vhost)
```

The HTTPS block above references `/etc/lighttpd/ssl/bikestat.pem`, which
doesn't exist yet — that's fine, lighttpd's syntax check passes. Don't
`systemctl reload` until after §8 finishes; the reload would fail.

### 7e. Initial HTTP-only smoke test (optional, before HTTPS)

If you want to confirm HTTP works before issuing the cert, comment out the
`$SERVER["socket"] == ":443"` block temporarily and reload. Quick check:

```bash
curl -s http://bikestat.org/ | grep -i title
# expect: <title>BikeStat — Traffic Count Aggregator</title>
```

Then put the HTTPS block back before continuing.

---

## 8. HTTPS via Let's Encrypt

```bash
sudo certbot certonly --webroot -w /var/www/bikestat \
    -d bikestat.org -d www.bikestat.org \
    --agree-tos -m you@example.com -n
```

(`-n` for non-interactive; substitute your email.)

Build the combined PEM lighttpd wants (privkey + fullchain):

```bash
sudo mkdir -p /etc/lighttpd/ssl
sudo bash -c 'cat /etc/letsencrypt/live/bikestat.org/privkey.pem \
                  /etc/letsencrypt/live/bikestat.org/fullchain.pem \
              > /etc/lighttpd/ssl/bikestat.pem'
sudo chmod 600 /etc/lighttpd/ssl/bikestat.pem
```

Reload lighttpd:
```bash
sudo lighttpd -tt -f /etc/lighttpd/lighttpd.conf
sudo systemctl reload lighttpd
```

Smoke tests:
```bash
curl -sI https://bikestat.org/ | head -3
# expect: HTTP/2 200

curl -sI http://bikestat.org/ | head -3
# expect: HTTP/1.1 301, Location: https://bikestat.org/

curl -sI https://www.bikestat.org/ | head -3
# expect: HTTP/2 301, location: https://bikestat.org/

curl -sI http://bikestat.org/.well-known/acme-challenge/test | head -3
# expect: HTTP/1.1 404 (NOT 301 — proves the ACME exception works for renewals)
```

### 8a. Renewal deploy hook

Without this hook, certbot will renew the cert in `/etc/letsencrypt/live/`
but won't rebuild your combined PEM, and lighttpd will eventually serve an
expired cert.

```bash
sudo nano /etc/letsencrypt/renewal-hooks/deploy/lighttpd-bikestat.sh
```

Paste:
```
#!/usr/bin/env bash
set -e
cat /etc/letsencrypt/live/bikestat.org/privkey.pem \
    /etc/letsencrypt/live/bikestat.org/fullchain.pem \
  > /etc/lighttpd/ssl/bikestat.pem
chmod 600 /etc/lighttpd/ssl/bikestat.pem
systemctl reload lighttpd
```

Save, mark executable, and dry-run to verify:
```bash
sudo chmod +x /etc/letsencrypt/renewal-hooks/deploy/lighttpd-bikestat.sh
sudo certbot renew --dry-run
# expect last line: "Congratulations, all simulated renewals succeeded"
```

Auto-renewal: `certbot` already installs a systemd timer. `systemctl list-timers | grep certbot` shows it.

---

## 9. Cron: hourly VdM refresh (as deploy user)

Clone the repo on the server (we need `scripts/refresh-vdm.sh`):

```bash
mkdir -p ~/GitHub
cd ~/GitHub
git clone https://github.com/OpenMobilityData/BikeStat.git
ls BikeStat/scripts/refresh-vdm.sh
```

Bootstrap-run once to populate `cyclistes.csv` and `status.txt`:

```bash
BIKESTAT_DATA_DIR=/var/www/bikestat/data \
    ~/GitHub/BikeStat/scripts/refresh-vdm.sh

ls -la /var/www/bikestat/data/cyclistes.csv /var/www/bikestat/data/status.txt
cat /var/www/bikestat/data/status.txt
```

Install the user crontab entry:
```bash
crontab -e
```

Add (substitute your home path / username):
```
7 * * * * BIKESTAT_DATA_DIR=/var/www/bikestat/data /home/rhoge/GitHub/BikeStat/scripts/refresh-vdm.sh >> /home/rhoge/bikestat-refresh.log 2>&1
```

Verify:
```bash
crontab -l
```

---

## 10. Smoke-test checklist

Open `https://bikestat.org/`:

- [ ] Browser shows a valid lock icon (no cert warning).
- [ ] Dark theme renders, sidebar populates with all sources.
- [ ] Telraam + CDN-NDG sources appear within ~2s, VdM (Bourret/Girouard)
      within ~5s.
- [ ] Header top-right shows `VdM data: YYYY-MM-DD HH:MM TZ`.
- [ ] Selecting a source draws a chart; preset buttons work.
- [ ] Year-on-Year mode produces a 12-month axis with year-bucketed series.

In dev tools → Network on a fresh reload:
- [ ] `cyclistes.csv` transferred ≈ 320 KB with `content-encoding: gzip`.
- [ ] `*.wasm` transferred (with `content-encoding: gzip`) at ~1–2 MB.
- [ ] No 404s in console.

After an hour (or on a manual cron run):
- [ ] `curl -sI https://bikestat.org/data/cyclistes.csv | grep -i last-modified`
      shows a fresh timestamp.
- [ ] `status.txt` shows the new time and the page reflects it after reload.

---

## 11. Ongoing maintenance

### Updating the app
On the **Mac**:
```bash
cd ~/Desktop/BikeStat
git pull
trunk build --release
rsync -av --delete dist/ rhoge@bikestat.org:/var/www/bikestat/
```

The fingerprinted asset filenames mean clients pick up the new bundle
automatically as soon as `index.html` references them. No lighttpd reload.

If `--delete` wipes the cron-managed `cyclistes.csv` and `status.txt`, the
next cron tick (within an hour) restores them. If you want them back
immediately, run the bootstrap one-shot from §9 again. Or `--exclude` them
in the rsync.

For Telraam / CDN-NDG xlsx — those live under `static/data/` and ship in
`dist/` via Trunk's `copy-dir`. Commit, push, pull, rebuild, rsync.

### Adding a new VdM location
Update `MONTREAL_LOCATION_FILTER` in `src/data/sources.rs` *and* add the
corresponding street name(s) to `FILTER_RE` at the top of
`scripts/refresh-vdm.sh`. The shell filter must remain a superset of the
parser filter; if it's narrower, the new location loads empty. Then
`git pull` on the server so the cron picks up the new filter.

### Logs
- Lighttpd: `sudo journalctl -u lighttpd -n 100`, plus
  `/var/log/lighttpd/error.log` if `mod_accesslog` is configured.
- Cron: `~/bikestat-refresh.log` (per the user crontab entry).

### Cert renewal
Automatic. The deploy hook in §8a regenerates the combined PEM and reloads
lighttpd. Audit by `systemctl list-timers | grep certbot` and
`sudo certbot renew --dry-run` whenever you want.

---

## 12. Loose ends

- Telraam compass-direction strings are still placeholder `A→B`/`B→A`.
  Look up segments 9794 and 10045 on the Telraam segment map and update
  `TELRAAM_ANNOTATIONS` in `src/data/sources.rs`.
- No analytics. lighttpd `mod_accesslog` (already enabled by Ubuntu
  default) writes basic access logs — fine for now. A small JS pixel later
  if you want anything fancier.
- No structured backend. If usage grows or VdM/Telraam APIs become
  necessary, the workspace-split + ingest-binary architecture is sketched
  in chat history (search "workspace split"). Not needed at current scale.

---

## 13. If something breaks

1. `sudo lighttpd -tt -f /etc/lighttpd/lighttpd.conf` — config validation.
2. `sudo journalctl -u lighttpd -n 100` — recent server logs.
3. `tail ~/bikestat-refresh.log` — cron output.
4. Browser dev tools → Network — status codes and content types.
5. Verify cron-written files exist:
   ```bash
   ls -la /var/www/bikestat/data/cyclistes.csv /var/www/bikestat/data/status.txt
   ```
6. If the site shows raw HTML in the header where the freshness string
   should be, an SPA fallback is intercepting the missing `status.txt`.
   Confirm there's no `url.rewrite-if-not-file → /index.html` rule in any
   conf-enabled file.
7. If the apex serves the wrong site (placeholder instead of BikeStat),
   the symlink in `conf-enabled/30-bikestat.conf` is missing. Recreate it
   per §7d.
8. If gzip isn't kicking in for the CSV, the global `20-deflate.conf`
   `deflate.mimetypes` list doesn't include `text/csv`. Per §7b, extend it
   globally — per-vhost overrides do not work on lighttpd 1.4.74.

---

## 14. Lessons learned (for next time)

- **Don't compile Rust on a small VPS.** Cargo's parallelism + rustc's
  per-process RAM use will swap-thrash a 1–2 GB box. Build elsewhere, ship
  artifacts.
- **Always create the `conf-enabled/` symlink** after writing a vhost.
  `lighty-enable-mod` doesn't help for custom vhosts — it's only for
  packaged module configs.
- **Test lighttpd config at multiple levels.** `lighttpd -tt` only catches
  syntax errors; `lighttpd -p` shows the *parsed* config and reveals which
  conditionals/directives actually take effect.
- **Heredocs over SSH are fragile.** Terminal paste can introduce leading
  whitespace that breaks the EOF marker. `nano` with paste-then-save is
  more reliable.
- **lighttpd's `deflate.mimetypes` doesn't merge across conditionals** in
  the version we used. Set it globally.
- **`Content-Encoding` headers don't show up on HEAD responses** in
  lighttpd's mod_deflate. Always test compression with a real GET.
- **Namecheap "Redirect Domain" + "Parking Page" silently override DNS.**
  Disable both before adding A records.
