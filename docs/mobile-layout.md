# Mobile layout

Phones below 768px, and touch devices below 1024px, use a compact header with a labeled Menu button. Desktop navigation and panel sizing remain unchanged.

Operation pages stack options above results. Options can collapse without losing their values, while Preview Summary and Apply Changes remain available. File tables become labeled rows with wrapping filenames, sortable headers, selection controls, and visible file actions. Library and Settings use narrower layouts. Dialogs fit the viewport, manage keyboard focus, and support Escape; the file browser includes single-tap folder and selection controls.

## Validation

- Production build passed.
- Full web suite passed: 150 tests. Subsequent targeted suite passed: 66 tests, including the added navigation test.
- Chromium checks covered Dashboard, Rename, Remove Tracks, Edit Tracks, Subtitles, Settings, Library, and Logs at 320, 375, 390, 768, and 1280 CSS pixels without page-level horizontal overflow.
- Touch interaction checks covered navigation, preserved extraction settings when collapsing options, template selection through file actions, all eight Settings sections, preview dialogs, and the file browser in a shortened viewport.
- Phone landscape checked at 844 × 390. Fixtures were used; no real media was changed.

Physical iOS/Android browsers and their actual software keyboards still need device testing. The shortened viewport check approximates keyboard space rather than opening a real mobile keyboard.
