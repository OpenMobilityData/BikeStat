# Server outage, October 27–29, 2026: storage volume detached, boot stopped in emergency mode

**Symptom.** bikestat.org down and `ssh blob` refused from about October 27, 2026. The server answered ping, but every TCP port (22, 80, 443) refused at once.

**Cause.** BuyVM restarted the VPS twice on October 27 (Stallion log: « Started Virtual Server », no user, provider addresses), most likely node maintenance. During it the 512 GB block storage slab « Data and Media » (VOLUME-28436) came off the server (Stallion showed it attached to nothing). `/etc/fstab` mounted it at `/mnt/Data` as `/dev/sda1 … defaults 0 2`, so the boot timed out waiting for `/dev/sda1` and stopped in emergency mode: the network came up (hence the ping), ssh and lighttpd never started (hence the refusals). Billing was not the cause: the VPS and the slab were both active; an invoice for November 4 – December 3 was paid on October 29.

**Fix (October 29, 2026).**
1. Stallion → Storage Volumes → « Attach to a Virtual Server » → BLOB.
2. Stallion → Power State → Forceful Reset (a Graceful Restart is ignored in emergency mode). Up in about 75 s; `/mnt/Data` mounted; lighttpd, ssh and cron active; the hourly refresh ran at 22:07 UTC.
3. `/etc/fstab` line 10 now reads `/dev/sda1  /mnt/Data  ext4  defaults,nofail,x-systemd.device-timeout=30s  0  2` (backup `/etc/fstab.bak`; `findmnt --verify` clean), so a missing volume no longer blocks the boot.

**Diagnosis, for next time.** If the site is down: ping the IP; if it answers but ports refuse instantly, open Stallion → the server → Console (web console): emergency mode means a mount failed. Check Stallion → Storage Volumes for the slab's attachment, and the FranTech client area for each service's status.

**Still open.** `postfix@-.service` failed at boot (mail is not used by BikeStat; disable it if nothing on the server sends mail). Consider mounting by `UUID=` instead of `/dev/sda1`.
