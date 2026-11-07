#!/bin/sh
# After the package is removed (D71 §6): the key holder's secrets go when the
# .deb is purged ("purge") or the .rpm is erased (no copy left: "0").
case "$1" in
  purge | 0) rm -rf /var/lib/pwm ;;
esac
exit 0
