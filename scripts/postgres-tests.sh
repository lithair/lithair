#!/bin/sh
# Run the lithair-postgres suite against a real PostgreSQL 17 with TLS, inside
# the cidx Rust container (Debian). Used by the `postgres-test` container of the
# cidx `test` phase; also runnable alone: `cidx run postgres-test`.
#
# The container starts as root only to install PostgreSQL. The server, cargo
# and the tests run as the owner of the workspace, so target/ never gets
# root-owned files and the tests can restart the server themselves.
#
# Provides to the tests:
#   LITHAIR_TEST_POSTGRES_URL            TLS URL of a database owned by a plain role
#   LITHAIR_TEST_POSTGRES_ISOLATED_URL   a second database, for tests that alter
#                                        the lithair schema itself
#   LITHAIR_TEST_POSTGRES_CA             CA that signs the server certificate
#   LITHAIR_TEST_POSTGRES_OTHER_CA       an unrelated CA (must be refused)
#   LITHAIR_TEST_POSTGRES_RESTART        shell command restarting the server
#   LITHAIR_REQUIRE_POSTGRES=1           missing settings fail instead of skipping
set -eu

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends postgresql openssl ca-certificates >/dev/null
PG=$(ls -d /usr/lib/postgresql/*/bin | sort -V | tail -1)
WORK="${WORKSPACE:-/work}"
U=$(stat -c %u "$WORK")
G=$(stat -c %g "$WORK")
if [ "$U" -eq 0 ]; then
    # PostgreSQL refuses to run as root.
    U=$(id -u postgres)
    G=$(id -g postgres)
fi
D=$(mktemp -d)
# initdb needs a passwd entry for the uid it runs as.
getent group "$G" >/dev/null || groupadd -o -g "$G" lithair-owner
getent passwd "$U" >/dev/null || useradd -o -u "$U" -g "$G" -M -d "$D" lithair-owner
as_owner() { setpriv --reuid="$U" --regid="$G" --clear-groups "$@"; }

# Test-only PKI: a CA, a server certificate for localhost, and a foreign CA.
cd "$D"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=lithair test CA" \
    -keyout ca.key -out ca.crt 2>/dev/null
openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost" \
    -keyout server.key -out server.csr 2>/dev/null
printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\n' > san.ext
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 1 \
    -extfile san.ext -out server.crt 2>/dev/null
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=foreign CA" \
    -keyout other.key -out other.crt 2>/dev/null
chmod 600 server.key

ADMIN_PASSWORD=$(openssl rand -hex 16)
PASSWORD=$(openssl rand -hex 16)
printf '%s\n' "$ADMIN_PASSWORD" > pw
chown -R "$U:$G" "$D"
as_owner "$PG/initdb" -D "$D/data" -U postgres -A scram-sha-256 --pwfile="$D/pw" >/dev/null
rm pw
cat >> data/postgresql.conf <<EOF
listen_addresses = '127.0.0.1'
unix_socket_directories = '$D'
ssl = on
ssl_cert_file = '$D/server.crt'
ssl_key_file = '$D/server.key'
EOF
# TLS for everyone; plain TCP only so the insecure-loopback test mode can run.
cat > data/pg_hba.conf <<EOF
hostssl all all 127.0.0.1/32 scram-sha-256
host    all all 127.0.0.1/32 scram-sha-256
EOF
as_owner "$PG/pg_ctl" -D "$D/data" -l "$D/server.log" -w start >/dev/null

PGPASSWORD=$ADMIN_PASSWORD "$PG/psql" -q -h 127.0.0.1 -U postgres -d postgres -v ON_ERROR_STOP=1 <<EOF
CREATE ROLE lithair LOGIN PASSWORD '$PASSWORD';
CREATE DATABASE lithair OWNER lithair;
CREATE DATABASE lithair_isolated OWNER lithair;
EOF

export LITHAIR_TEST_POSTGRES_URL="postgres://lithair:$PASSWORD@localhost:5432/lithair"
export LITHAIR_TEST_POSTGRES_ISOLATED_URL="postgres://lithair:$PASSWORD@localhost:5432/lithair_isolated"
export LITHAIR_TEST_POSTGRES_CA="$D/ca.crt"
export LITHAIR_TEST_POSTGRES_OTHER_CA="$D/other.crt"
export LITHAIR_TEST_POSTGRES_RESTART="$PG/pg_ctl -D $D/data -l $D/server.log -w -m fast restart >/dev/null"
export LITHAIR_REQUIRE_POSTGRES=1

cd "$WORK"
status=0
as_owner env HOME="$D" cargo test -p lithair-postgres --tests || status=$?
if [ "$status" -ne 0 ]; then
    tail -n 50 "$D/server.log" || true
fi
as_owner "$PG/pg_ctl" -D "$D/data" -w stop >/dev/null || true
exit "$status"
