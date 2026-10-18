#!/bin/bash
# Double-click in Finder to uninstall OpenBark. Your encounters and models are kept unless you choose to delete them.
cd "$(dirname "$0")" && ./uninstall.sh "$@"
read -r -p "Press Enter to close. "
