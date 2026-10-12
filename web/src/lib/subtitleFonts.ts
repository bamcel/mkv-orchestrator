import andikaBasicUrl from "../assets/fonts/AndikaBasic-Regular.ttf?url";
import liberationSansUrl from "../assets/fonts/LiberationSans-Regular.ttf?url";

export type SubtitleFont = {
  id: string;
  family: string;
  fileName: string;
  dataBase64?: string;
  assetUrl?: string;
  builtIn: boolean;
};

export const builtInSubtitleFonts: SubtitleFont[] = [
  { id: "builtin-andika-basic", family: "Andika Basic", fileName: "AndikaBasic-Regular.ttf", assetUrl: andikaBasicUrl, builtIn: true },
  { id: "builtin-liberation-sans", family: "Liberation Sans", fileName: "LiberationSans-Regular.ttf", assetUrl: liberationSansUrl, builtIn: true }
];

const databaseName = "mkvo-subtitle-fonts";
const storeName = "fonts";
const databaseVersion = 2;
const fallbackStorageKey = "mkvo-subtitle-fonts-fallback";

function openDatabase(): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(databaseName, databaseVersion);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(storeName)) request.result.createObjectStore(storeName, { keyPath: "id" });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function loadImportedFonts(): Promise<SubtitleFont[]> {
  const fallbackFonts = loadFallbackFonts();
  try {
    const database = await openDatabase();
    const indexedFonts = await new Promise<SubtitleFont[]>((resolve, reject) => {
      const request = database.transaction(storeName, "readonly").objectStore(storeName).getAll();
      request.onsuccess = () => resolve(request.result as SubtitleFont[]);
      request.onerror = () => reject(request.error);
    }).finally(() => database.close());
    return uniqueFonts([...fallbackFonts, ...indexedFonts]);
  } catch {
    return fallbackFonts;
  }
}

export async function saveImportedFont(font: SubtitleFont): Promise<void> {
  try {
    const database = await openDatabase();
    try {
      await new Promise<void>((resolve, reject) => {
        const transaction = database.transaction(storeName, "readwrite");
        transaction.objectStore(storeName).put(font);
        transaction.oncomplete = () => resolve();
        transaction.onerror = () => reject(transaction.error);
        transaction.onabort = () => reject(transaction.error ?? new Error("The font import was cancelled by browser storage."));
      });
    } finally {
      database.close();
    }
  } catch {
    saveFallbackFonts(uniqueFonts([...loadFallbackFonts(), font]));
  }
}

export async function deleteImportedFont(id: string): Promise<void> {
  try {
    const database = await openDatabase();
    try {
      await new Promise<void>((resolve, reject) => {
        const transaction = database.transaction(storeName, "readwrite");
        transaction.objectStore(storeName).delete(id);
        transaction.oncomplete = () => resolve();
        transaction.onerror = () => reject(transaction.error);
        transaction.onabort = () => reject(transaction.error ?? new Error("The saved font could not be removed."));
      });
    } finally {
      database.close();
    }
  } catch {
    // The fallback below still removes the font when IndexedDB is unavailable.
  }
  saveFallbackFonts(loadFallbackFonts().filter((font) => font.id !== id));
}

export async function resolveFontData(font: SubtitleFont): Promise<SubtitleFont> {
  if (font.dataBase64) return font;
  if (!font.assetUrl) throw new Error("The selected font has no font data.");
  const response = await fetch(font.assetUrl);
  if (!response.ok) throw new Error(`Could not load bundled font ${font.family}.`);
  return { ...font, dataBase64: arrayBufferToBase64(await response.arrayBuffer()) };
}

export function fileToSubtitleFont(file: File): Promise<SubtitleFont> {
  return readFileBuffer(file).then((buffer) => ({
    id: createImportedFontId(),
    family: file.name.replace(/\.(?:ttf|otf)$/i, "").replace(/[-_]+/g, " ").trim(),
    fileName: file.name,
    dataBase64: arrayBufferToBase64(buffer),
    builtIn: false
  }));
}

function createImportedFontId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return `imported-${crypto.randomUUID()}`;
  }

  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    const bytes = crypto.getRandomValues(new Uint8Array(16));
    return `imported-${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  }

  return `imported-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function readFileBuffer(file: File): Promise<ArrayBuffer> {
  if (typeof file.arrayBuffer === "function") return file.arrayBuffer();
  return new Promise<ArrayBuffer>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => reader.result instanceof ArrayBuffer ? resolve(reader.result) : reject(new Error("The selected font could not be read."));
    reader.onerror = () => reject(reader.error ?? new Error("The selected font could not be read."));
    reader.readAsArrayBuffer(file);
  });
}

export async function registerPreviewFont(font: SubtitleFont): Promise<void> {
  const resolved = await resolveFontData(font);
  const bytes = Uint8Array.from(atob(resolved.dataBase64 ?? ""), (character) => character.charCodeAt(0));
  const face = new FontFace(font.family, bytes.buffer);
  await face.load();
  document.fonts.add(face);
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

function loadFallbackFonts(): SubtitleFont[] {
  try {
    const value = localStorage.getItem(fallbackStorageKey);
    if (!value) return [];
    const fonts = JSON.parse(value) as SubtitleFont[];
    return Array.isArray(fonts) ? fonts.filter((font) => font && typeof font.id === "string" && typeof font.family === "string" && font.builtIn === false) : [];
  } catch {
    return [];
  }
}

function saveFallbackFonts(fonts: SubtitleFont[]): void {
  localStorage.setItem(fallbackStorageKey, JSON.stringify(fonts));
}

function uniqueFonts(fonts: SubtitleFont[]): SubtitleFont[] {
  return [...new Map(fonts.map((font) => [font.id, font])).values()];
}
