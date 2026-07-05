#!/usr/bin/env bash
# One-time setup so `arcglyph` can run as your normal user (no sudo).
# - adds you to the `input` group (needed to read /dev/input/event*)
# - installs a udev rule that gives /dev/uinput group=input,mode=0660
set -euo pipefail

RULE=/etc/udev/rules.d/60-arcglyph-uinput.rules
CONTENT='KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"'

echo "==> installing udev rule at $RULE"
echo "$CONTENT" | sudo tee "$RULE" > /dev/null
sudo udevadm control --reload
sudo udevadm trigger

echo "==> adding $USER to the input group"
sudo usermod -aG input "$USER"

echo
echo "Done. Log out and log back in so the group change takes effect."
echo "Then run:  $(dirname "$0")/target/release/arcglyph"
