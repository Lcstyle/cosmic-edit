# Restoring Notepad++ Unsaved Files to Cosmic Edit

This document describes how to restore unsaved files from Notepad++ (Windows) into Cosmic Edit's hot-exit session system, allowing them to appear as unsaved tabs when Cosmic Edit launches.

## Background

Both Notepad++ and Cosmic Edit have "hot-exit" functionality that preserves unsaved work:

- **Notepad++**: Saves unsaved files to `%AppData%\Roaming\Notepad++\backup\`
- **Cosmic Edit**: Saves unsaved files to `~/.cache/cosmic-edit/backups/` with session metadata in `~/.cache/cosmic-edit/sessions/`

When migrating from Windows or recovering lost sessions, you can convert Notepad++ backup files into Cosmic Edit sessions.

## Notepad++ Backup Location

### Standard Installation
```
C:\Users\<USERNAME>\AppData\Roaming\Notepad++\backup\
```

### Portable Installation
```
<Notepad++ Install Directory>\backup\
```

### File Naming Format
Files are named: `<original_name>@YYYY-MM-DD_HHMMSS`

Examples:
- `new 1@2025-12-12_222358` - Unsaved "new 1" tab from Dec 12, 2025
- `script.py@2026-01-15_101901` - Unsaved changes to script.py

### Accessing from Linux

If you have a Windows partition mounted (e.g., at `/home/windows`), the path would be:
```
/home/windows/Users/<USERNAME>/AppData/Roaming/Notepad++/backup/
```

## Cosmic Edit Session Structure

### Directories
- **Backups**: `~/.cache/cosmic-edit/backups/` - Contains document backups with metadata
- **Sessions**: `~/.cache/cosmic-edit/sessions/` - JSON files describing open tabs

### Backup File Format (IMPORTANT)
Backup files have a specific format that **must** be followed:
```
{JSON metadata on first line}
{actual content starting from second line}
```

The metadata JSON contains:
```json
{"id":"abc123...","session_id":12345,"path":null,"cursor_line":0,"cursor_index":0,"zoom_adj":0}
```

- `id`: Unique backup identifier (must match filename without extension)
- `session_id`: 64-bit integer session ID
- `path`: File path or `null` for unsaved documents
- `cursor_line`, `cursor_index`: Cursor position
- `zoom_adj`: Zoom level adjustment

**File naming**: `{backup_id}.backup` (e.g., `d98bcd6daa7de426.backup`)

### Session File Naming (IMPORTANT)
Session files **must** be named with a 16-character hexadecimal ID:
```
{16-char-hex-id}.json
```
Examples:
- `595c72b8c2281eae.json` - Valid
- `notepadpp_restored.json` - **INVALID** (won't be detected)

Cosmic Edit scans for `.json` files and parses the filename as a 64-bit hex session ID. Files with non-hex names are ignored.

### Session JSON Format
```json
{
  "tabs": [
    {
      "path": null,
      "has_unsaved_changes": true,
      "backup_id": "abc123def456...",
      "is_pinned": false
    }
  ],
  "projects": [],
  "active_tab": 0,
  "active_project_path": null
}
```

For unsaved files:
- `path`: `null` (no file path yet)
- `has_unsaved_changes`: `true`
- `backup_id`: References a file in the backups directory

## Manual Conversion Process

### Step 1: Locate Notepad++ Backups
```bash
ls -la "/home/windows/Users/<USERNAME>/AppData/Roaming/Notepad++/backup/"
```

### Step 2: Preview Files
```bash
for f in "/path/to/notepad++/backup/"*; do
    size=$(stat -c%s "$f" 2>/dev/null)
    if [ "$size" -gt 0 ]; then
        preview=$(head -c 80 "$f" | tr '\n' ' ')
        echo "$(basename "$f"): $preview..."
    fi
done
```

### Step 3: Run Conversion Script
See the `restore-notepadpp-session` skill or use the Python script below.

## Conversion Script

```python
#!/usr/bin/env python3
"""
Convert Notepad++ backup files to Cosmic Edit sessions.

Usage:
    python3 convert_notepadpp_to_cosmic.py /path/to/notepad++/backup/
"""

import json
import os
import hashlib
import random
import sys

def convert_notepadpp_backups(npp_backup_dir, cosmic_cache_dir=None):
    """Convert Notepad++ backups to Cosmic Edit session format."""

    if cosmic_cache_dir is None:
        cosmic_cache_dir = os.path.expanduser("~/.cache/cosmic-edit")

    cosmic_backup_dir = os.path.join(cosmic_cache_dir, "backups")
    cosmic_session_dir = os.path.join(cosmic_cache_dir, "sessions")

    # Ensure directories exist
    os.makedirs(cosmic_backup_dir, exist_ok=True)
    os.makedirs(cosmic_session_dir, exist_ok=True)

    # Generate a session ID for this conversion
    session_id = random.getrandbits(64)
    hex_session_id = f"{session_id:016x}"

    tabs = []

    for filename in sorted(os.listdir(npp_backup_dir)):
        filepath = os.path.join(npp_backup_dir, filename)

        if not os.path.isfile(filepath):
            continue

        size = os.path.getsize(filepath)
        if size == 0:
            print(f"Skipping empty file: {filename}")
            continue

        # Read content as text
        try:
            with open(filepath, 'r', encoding='utf-8', errors='replace') as f:
                content = f.read()
        except Exception as e:
            print(f"Error reading {filename}: {e}")
            continue

        # Generate unique backup ID
        hash_input = f"{filename}:{hashlib.md5(content.encode()).hexdigest()}"
        backup_id = hashlib.sha256(hash_input.encode()).hexdigest()[:16]

        # Create metadata for backup file (first line)
        metadata = {
            "id": backup_id,
            "session_id": session_id,
            "path": None,  # Unsaved file
            "cursor_line": 0,
            "cursor_index": 0,
            "zoom_adj": 0
        }

        # Write backup file: metadata JSON on first line, then content
        backup_path = os.path.join(cosmic_backup_dir, f"{backup_id}.backup")
        with open(backup_path, 'w', encoding='utf-8') as f:
            f.write(json.dumps(metadata) + '\n')
            f.write(content)

        # Create tab entry for session
        tabs.append({
            "path": None,
            "has_unsaved_changes": True,
            "backup_id": backup_id,
            "is_pinned": False
        })

        print(f"Converted: {filename} -> {backup_id}.backup ({size} bytes)")

    # Create session file
    # IMPORTANT: Session filename must be a 16-char hex ID for cosmic-edit to recognize it
    session = {
        "tabs": tabs,
        "projects": [],
        "active_tab": 0,
        "active_project_path": None
    }

    session_file = os.path.join(cosmic_session_dir, f"{hex_session_id}.json")
    with open(session_file, 'w') as f:
        json.dump(session, f, indent=2)

    print(f"\nCreated session with {len(tabs)} tabs")
    print(f"Session file: {session_file}")
    print(f"Session ID: {hex_session_id}")

    return len(tabs)

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 convert_notepadpp_to_cosmic.py /path/to/notepad++/backup/")
        sys.exit(1)

    convert_notepadpp_backups(sys.argv[1])
```

## Troubleshooting

### Files Not Appearing After Restore
1. Check ownership: `chown -R $USER:$USER ~/.cache/cosmic-edit/`
2. Verify backup files exist: `ls ~/.cache/cosmic-edit/backups/`
3. Check session JSON is valid: `python3 -m json.tool ~/.cache/cosmic-edit/sessions/*.json`

### Session Restore Mode
Ensure Cosmic Edit's session restore is enabled:
- Settings > App Settings > "Reopen projects and tabs on start" should be enabled

### Duplicate Tabs Bug
If sessions grow exponentially with duplicate tabs, this indicates a bug in session merging. The fix (as of Jan 2026) ensures merged sessions are discarded after restore in single-window mode.

## Related Files

- `/home/lcstyle/Documents/RustroverProjects/cosmic-edit/src/hotexit.rs` - Hot-exit implementation
- `/home/lcstyle/Documents/RustroverProjects/cosmic-edit/src/main.rs` - Session restore logic

## References

- [Notepad++ Community - Backup Location](https://community.notepad-plus-plus.org/topic/26741/where-are-new-files-still-unsaved-by-the-user-stored)
- [MiniTool - Notepad++ Backup Location](https://www.minitool.com/news/notepad-plus-plus-backup-location.html)
