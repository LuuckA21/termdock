#!/usr/bin/env bash
# Starts throw-away OpenSSH servers for the end-to-end tests and prints the
# environment variables expected by crates/termdock-core/tests/ssh_e2e.rs.
#
#   eval "$(sudo scripts/e2e-sshd.sh)"
#   cargo test -p termdock-core --test ssh_e2e
#
# Linux only, needs root (creates the user `tdtest`) and openssh-server.
set -euo pipefail

USER_NAME=tdtest
PASSWORD="e2e-$(head -c 12 /dev/urandom | base64 | tr -dc 'A-Za-z0-9')"
KEY_PASS="kp-$(head -c 9 /dev/urandom | base64 | tr -dc 'A-Za-z0-9')"
DIR=$(mktemp -d /tmp/termdock-e2e.XXXXXX)
chmod 755 "$DIR"

id "$USER_NAME" >/dev/null 2>&1 || useradd -m -s /bin/bash "$USER_NAME"
echo "$USER_NAME:$PASSWORD" | chpasswd
HOME_DIR=$(getent passwd "$USER_NAME" | cut -d: -f6)

ssh-keygen -q -t ed25519 -N '' -f "$DIR/host_ed25519"
ssh-keygen -q -t ed25519 -N "$KEY_PASS" -C "$USER_NAME" -f "$DIR/user_key"
install -d -m 700 -o "$USER_NAME" "$HOME_DIR/.ssh"
install -m 600 -o "$USER_NAME" "$DIR/user_key.pub" "$HOME_DIR/.ssh/authorized_keys"
# The tests run as the invoking user and must be able to read the client key.
chown "${SUDO_UID:-0}" "$DIR/user_key"

mkdir -p /run/sshd
common() {
  cat <<CFG
ListenAddress 127.0.0.1
HostKey $DIR/host_ed25519
PubkeyAuthentication yes
UsePAM yes
Subsystem sftp internal-sftp
LogLevel VERBOSE
CFG
}
{ common; echo "Port 2222"; echo "PidFile $DIR/sshd.pid"; echo "PasswordAuthentication yes"; echo "KbdInteractiveAuthentication no"; } >"$DIR/sshd_config"
{ common; echo "Port 2223"; echo "PidFile $DIR/sshd-kbd.pid"; echo "PasswordAuthentication no"; echo "KbdInteractiveAuthentication yes"; } >"$DIR/sshd_kbd_config"

/usr/sbin/sshd -f "$DIR/sshd_config" -E "$DIR/sshd.log"
/usr/sbin/sshd -f "$DIR/sshd_kbd_config" -E "$DIR/sshd-kbd.log"

cat <<OUT
export TERMDOCK_E2E_SSH=127.0.0.1:2222
export TERMDOCK_E2E_SSH_KBD=127.0.0.1:2223
export TERMDOCK_E2E_USER=$USER_NAME
export TERMDOCK_E2E_PASSWORD=$PASSWORD
export TERMDOCK_E2E_KEY=$DIR/user_key
export TERMDOCK_E2E_KEY_PASS=$KEY_PASS
OUT
