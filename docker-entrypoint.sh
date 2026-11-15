#!/bin/sh
# Fly mounts the /data volume owned by root: fix ownership, then drop privileges.
set -e
if [ "$(id -u)" = "0" ]; then
    mkdir -p /data
    chown -R anpot:anpot /data
    exec setpriv --reuid=anpot --regid=anpot --init-groups /usr/local/bin/anpot "$@"
fi
exec /usr/local/bin/anpot "$@"
