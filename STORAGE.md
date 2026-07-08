# Where FoxCull Stores Things

FoxCull does not import, copy, or modify originals during browsing/culling. It
reads media where it already lives and stores app-generated data separately.

## Per-Drive Library

On writable drives, FoxCull creates one library folder at the drive root:

```text
<drive root>/
  _FoxCull/
    catalog.sqlite
    thumbs/
    recycle/
```

- `catalog.sqlite` stores ratings, labels, flags, tags, trims, capture dates,
  and related-file metadata.
- `thumbs/` stores generated thumbnails, Focus previews, video posters, and
  Live Scrub assets.
- `recycle/` stores files moved to the in-app Trash.

The app hides `_FoxCull` from its folder tree and media grids. Current builds do
not adopt old pre-stable library folders; after one-time migration, `_FoxCull`
is the single source of truth.

## Read-Only Drives

If a drive root is not writable, FoxCull stores the library under app data:

- Windows: `%APPDATA%\com.foxcull.app\libraries\<drive-id>\`
- macOS: `~/Library/Application Support/com.foxcull.app/libraries/<drive-id>/`
- Linux: `~/.config/com.foxcull.app/libraries/<drive-id>/`

Ratings and culling still work there. Delete sweeps are disabled when the media
drive itself cannot be written.

## Portable Mode

The Windows portable build keeps app settings and default data beside the EXE
when a folder named `foxcull-data` sits next to `foxcull.exe`.

Older portable data folder names are not adopted by current builds.

## Prepare And Cache Behavior

FoxCull uses cache in a few layers:

- Folder open automatically warms grid thumbnails and video poster previews.
- Focus view prefetch keeps nearby full previews warm around the active item.
- The Prepare button explicitly builds full-size Focus previews/posters for the
  current folder/filter set.
- Live Scrub generates low-resolution scrub strips only when enabled/on demand.

The cache is safe to delete; FoxCull will regenerate it. Deleting cache only
costs time on the next browse/prepare pass.

Do not delete `catalog.sqlite` unless you intentionally want to lose that drive's
ratings, flags, tags, trims, and related-file state.

## Trash

With **In-app Trash**, rejected files are moved into:

```text
<drive>/_FoxCull/recycle/
```

Restore from the in-app Trash panel. Emptying/purging Trash permanently deletes
those files.
