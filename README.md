
# Automatic File Organizer

A beginner-friendly Bash script that automatically organizes files into folders based on their file extensions.

## Features

- Organizes PDFs, images, documents, audio, videos, and scripts.
- Creates destination folders automatically.
- Skips directories and non-files.
- Avoids overwriting files when the destination filename already exists.
- Reports file movements in the terminal.

## Requirements

- macOS or a Unix-like operating system
- Bash
- Git (optional, for version control)

## Project Structure

```text
Shellsort/
├── organizer.sh
├── test_files/
├── README.md
└── LICENSE
```

## Usage

Clone the repository:

```bash
git clone https://github.com/a1nbuselvan/Shellsort.git
cd Shellsort
```

Run the organizer on a test directory:

```bash
bash organizer.sh test_files
```

## Supported Categories

| Extensions | Folder |
|---|---|
| pdf | PDFs |
| jpg, jpeg, png, gif | Images |
| txt, doc, docx | Documents |
| mp3, wav | Audio |
| mp4, mov | Videos |
| sh | Scripts |
| Other extensions | Others |

## Testing

Create sample files inside `test_files/` and run the script. Inspect the results with:

```bash
find test_files -print
```

**Warning:** The script moves files. Test with sample files before using it on important data.

## Future Improvements

- Add a dry-run mode.
- Improve extension matching.
- Handle duplicate filenames more flexibly.
- Add additional file categories.

## Author

Anbuselvan
