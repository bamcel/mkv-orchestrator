# API Provider Key Handling

MKV Orchestrator uses a user-supplied API key model for metadata providers.

## Policy

MKVO should not ship shared production API keys.

Each user is expected to configure their own provider credentials under:

```text
Settings > API Providers
```

## Current Providers

```text
TVDB
TMDB
```

TVDB and TMDB can be used for TV and movie metadata lookup. The selected provider must be configured before rename searches can run.

## Stored Settings

Credentials are stored locally in MKVO settings:

```text
TvdbApiKey
TvdbPin
TmdbApiKey
```

Credentials are not intended to be committed to source control, bundled with releases, written to logs, or included in support files.

## UX Rules

- Detect missing provider API keys before starting metadata lookup.
- Show a clear status message when the selected provider is not configured.
- Do not let provider exceptions be the first indication that a key is missing.
- Mask API key fields in the UI.
- Do not log API keys.
- Do not include API keys in support/export bundles.
- Keep provider setup links visible in Settings.

## Missing-Key Behavior

If selected provider is TVDB and `TvdbApiKey` is blank:

```text
TVDB API key is required. Add your own TVDB API key in Settings > API Providers.
```

If selected provider is TMDB and `TmdbApiKey` is blank:

```text
TMDB API key is required. Add your own TMDB API key in Settings > API Providers.
```

## Test Connection

Settings includes a **Test Selected Provider** button. It performs a lightweight provider search using the selected provider and reports success/failure without exposing credentials.
