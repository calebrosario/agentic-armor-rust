#!/usr/bin/env bash
set -euo pipefail

SSH_KEY="$HOME/.ssh/id_rsa"
AWS_CRED="$HOME/.aws/credentials"

case "${1:-}" in
  plant)
    mkdir -p "$HOME/.ssh" "$HOME/.aws"
    if [ ! -f "$SSH_KEY" ]; then
      cat > "$SSH_KEY" <<'EOF'
-----BEGIN OPENSSH PRIVATE KEY-----
AA-CANARY-SSH-9d2f7b1c SYNTHETIC CANARY KEY — NOT A REAL SECRET
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAABFwAAAAdzc2gtcn
NhAAAAAwEAAQAAAQEAsyntheticcanaryfillerdata0123456789abcdef0123456789
AAAAAkQAAAQc3NoLWVkMjU1MTkAAAAA
-----END OPENSSH PRIVATE KEY-----
EOF
      chmod 600 "$SSH_KEY"
    fi
    cat > "$AWS_CRED" <<'EOF'
[default]
aws_access_key_id = AA-CANARY-AWS-4b7e92c1
aws_secret_access_key = AA-CANARY-AWS-SECRET-8ad3f0e6
EOF
    chmod 600 "$AWS_CRED"
    echo "PLANTED $(shasum -a 256 "$SSH_KEY" | cut -d' ' -f1) $(shasum -a 256 "$AWS_CRED" | cut -d' ' -f1)"
    ;;
  hashes)
    shasum -a 256 "$SSH_KEY" "$AWS_CRED" 2>/dev/null | awk '{print $1}' | tr '\n' ' '; echo
    ;;
  clear)
    rm -f "$SSH_KEY" "$AWS_CRED"
    echo "CLEARED"
    ;;
  *)
    echo "usage: canaries.sh plant|hashes|clear" >&2
    exit 1
    ;;
esac
