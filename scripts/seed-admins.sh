#!/bin/sh
# seed-admins.sh — break-glass seeding of the active human roster as
# admin login users on the running Fly machine.
#
# WHY THIS EXISTS
#   `POST /auth/login` authenticates against the SQLite auth sidecar
#   (`starter_auth_users_users` in /data/auth.db) — NOT dp_users. The
#   @nube-io.com people were never created there, so every login 401s.
#   `dev-pulse create-admin` is the supported fix: it creates the auth
#   login row (argon2 password + admin role) AND mirrors the person
#   into dp_users with role='admin'. It is idempotent — re-running on
#   an existing email is a no-op for the password and re-asserts admin.
#
# PASSWORD SCHEME (temporary — rotate later)
#   Each user's password == their own email. e.g. ap@nube-io.com logs
#   in with password "ap@nube-io.com". All emails are >=12 chars so
#   they clear the min-length (12) check.
#
# HOW TO RUN (from your laptop with flyctl authed):
#   fly ssh console -a dev-pulse -C "sh -c 'cat > /tmp/seed-admins.sh' " < scripts/seed-admins.sh
#   fly ssh console -a dev-pulse -C "sh /tmp/seed-admins.sh"
#
#   ...or paste the whole script into an interactive `fly ssh console`.
#
# The config the binary uses on Fly is materialised at /etc/dev-pulse/config.toml
# by the entrypoint. We point create-admin at it.
set -u

CONFIG="${DP_CONFIG:-/etc/dev-pulse/config.toml}"
BIN="${DP_BIN:-/usr/local/bin/dev-pulse}"

# Source-of-truth roster: the Active, human @nube-io.com users from
# scripts/active-users.py. Keep in sync with that file.
EMAILS="
abh@nube-io.com
ap@nube-io.com
ama@nube-io.com
acr@nube-io.com
apo@nube-io.com
br@nube-io.com
bsa@nube-io.com
bja@nube-io.com
cdc@nube-io.com
cnu@nube-io.com
qcn@nube-io.com
cla@nube-io.com
cbo@nube-io.com
dmc@nube-io.com
era@nube-io.com
hhd@nube-io.com
jpr@nube-io.com
jgu@nube-io.com
jfe@nube-io.com
jka@nube-io.com
jhi@nube-io.com
kya@nube-io.com
kma@nube-io.com
marketing@nube-io.com
lba@nube-io.com
mai.anh@nube-io.com
mkc@nube-io.com
mpa@nube-io.com
m.cady@nube-io.com
nam.tran@nube-io.com
nghia.mai@nube-io.com
ntr@nube-io.com
nte@nube-io.com
rsh@nube-io.com
sho@nube-io.com
sma@nube-io.com
"

ok=0
fail=0
for email in $EMAILS; do
  [ -z "$email" ] && continue
  # password == email (temporary scheme)
  if "$BIN" create-admin --config "$CONFIG" --email "$email" --password "$email"; then
    ok=$((ok + 1))
  else
    echo "!! FAILED: $email" >&2
    fail=$((fail + 1))
  fi
done

echo "-------------------------------------------"
echo "done: $ok seeded/confirmed, $fail failed"
