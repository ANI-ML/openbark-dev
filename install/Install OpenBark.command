#!/bin/bash
# Double-click in Finder to install OpenBark for this Mac user.
cd "$(dirname "$0")" && ./install.sh "$@"
read -r -p "Press Enter to close. "
