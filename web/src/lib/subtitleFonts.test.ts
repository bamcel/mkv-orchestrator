import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { deleteImportedFont, fileToSubtitleFont, loadImportedFonts, saveImportedFont, type SubtitleFont } from "./subtitleFonts";

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

it("imports a font when crypto.randomUUID is unavailable", async () => {
  vi.stubGlobal("crypto", {
    getRandomValues: (bytes: Uint8Array) => {
      bytes.fill(7);
      return bytes;
    }
  });
  const file = new File([new Uint8Array([0x4f, 0x54, 0x54, 0x4f])], "DrawingBrutalDemo.otf", { type: "font/otf" });

  await expect(fileToSubtitleFont(file)).resolves.toEqual({
    id: `imported-${"07".repeat(16)}`,
    family: "DrawingBrutalDemo",
    fileName: "DrawingBrutalDemo.otf",
    dataBase64: "T1RUTw==",
    builtIn: false
  });
});
