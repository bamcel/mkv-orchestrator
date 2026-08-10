# Attribution and Logos

MKV Orchestrator includes a centralized attribution area under:

```text
Settings > About
```

## Assets

Logo assets are stored in:

```text
web/src/assets/logos/
```

Current assets:

```text
tmdb.png
tvdb.png
mkvtoolnix.png
ffmpeg.png
```

## TMDB

Required attribution text used by the app:

```text
This product uses the TMDB API but is not endorsed or certified by TMDB.
```

Keep MKVO branding more prominent than TMDB branding and avoid implying endorsement.

## TheTVDB

Current attribution text used by the app:

```text
Metadata provided by TheTVDB.
```

Avoid redistributing TVDB artwork unless licensing is explicitly handled.

## MKVToolNix

Current attribution text used by the app:

```text
MKVToolNix tools are used for MKV remuxing, metadata editing, extraction, and analysis.
```

## FFmpeg

Current attribution text used by the app:

```text
FFmpeg and ffprobe are used for media metadata analysis.
```

## UI rules

- Keep all third-party logos in the About tab.
- Normalize visual logo height to approximately 48px.
- Use the same card/grid layout for every provider/tool.
- Keep provider attribution separate from primary workflow screens.
- Do not make third-party branding more prominent than MKVO branding.

## Image format note

TMDB and TheTVDB logos are stored as PNG assets. The webview renders SVG
natively, so the original constraint behind this choice -- avoiding an extra
Avalonia SVG package -- no longer applies; PNG is kept because the supplied
brand assets are authoritative in that form.
