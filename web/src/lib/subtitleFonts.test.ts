import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { deleteImportedFont, loadImportedFonts, saveImportedFont, type SubtitleFont } from "./subtitleFonts";

const importedFont: SubtitleFont = {
  id: "imported-drawing-brutal",
  family: "Drawing Brutal Demo",
  fileName: "DrawingBrutalDemo.otf",
  dataBase64: "T1RUTw==",
  builtIn: false
};

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal("indexedDB", undefined);
});

afterEach(() => vi.unstubAllGlobals());

it("keeps imported fonts persistent when IndexedDB is unavailable", async () => {
  await saveImportedFont(importedFont);

  await expect(loadImportedFonts()).resolves.toEqual([importedFont]);

  await deleteImportedFont(importedFont.id);
  await expect(loadImportedFonts()).resolves.toEqual([]);
});
