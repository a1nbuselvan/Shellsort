#!/bin/bash

# Check whether a folder was provided
if [ -z "$1" ]; then
    echo "Usage: bash organizer.sh <folder_path>"
    exit 1
fi

TARGET="$1"

# Check whether the folder exists
if [ ! -d "$TARGET" ]; then
    echo "Error: Folder does not exist."
    exit 1
fi

# Process every item in the target folder
for file in "$TARGET"/*; do

    # Skip directories and other non-files
    [ -f "$file" ] || continue

    filename="${file##*/}"
    extension="${filename##*.}"

    # Skip files without extensions
    if [ "$filename" = "$extension" ]; then
        folder="Others"
    else
        case "$extension" in
            pdf|PDF)
                folder="PDFs"
                ;;
            jpg|JPG|jpeg|JPEG|png|PNG|gif|GIF|webp|WEBP)
                folder="Images"
                ;;
            txt|TXT|doc|DOC|docx|DOCX|rtf|RTF|odt|ODT)
                folder="Documents"
                ;;
            mp3|MP3|wav|WAV|m4a|M4A)
                folder="Audio"
                ;;
            mp4|MP4|mov|MOV|mkv|MKV)
                folder="Videos"
                ;;
            sh|SH|bash|BASH)
                folder="Scripts"
                ;;
            *)
                folder="Others"
                ;;
        esac
    fi

    # Create the destination folder
    mkdir -p "$TARGET/$folder"

    # Avoid overwriting an existing file
    if [ -e "$TARGET/$folder/$filename" ]; then
        echo "Skipped: $filename (destination already exists)"
        continue
    fi

    # Move the file
    mv "$file" "$TARGET/$folder/"

    echo "Organized: $filename -> $folder/"

done

echo "File organization complete!"