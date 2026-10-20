#!/bin/sh

# Compatibility wrapper for installations that prefer the original script name.
# The installed CLI owns preview, confirmation, classification, and file safety.
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage: organizer.sh <folder_path> [--yes]" >&2
    exit 2
fi

if ! command -v shellsort >/dev/null 2>&1; then
    echo "Error: shellsort was not found on PATH. Install Shellsort and try again." >&2
    exit 127
fi

if [ "$#" -eq 2 ]; then
    if [ "$2" != "--yes" ]; then
        echo "Usage: organizer.sh <folder_path> [--yes]" >&2
        exit 2
    fi
    exec shellsort organize "$1" --yes
fi

exec shellsort organize "$1"
