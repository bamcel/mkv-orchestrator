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

function openDatabase(): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(databaseName, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(storeName, { keyPath: "id" });
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function loadImportedFonts(): Promise<SubtitleFont[]> {
  const database = await openDatabase();
  return new Promise<SubtitleFont[]>((resolve, reject) => {
    const request = database.transaction(storeName, "readonly").objectStore(storeName).getAll();
    request.onsuccess = () => resolve(request.result as SubtitleFont[]);
    request.onerror = () => reject(request.error);
  }).finally(() => database.close());
}

export async function saveImportedFont(font: SubtitleFont): Promise<void> {
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
}

export async function deleteImportedFont(id: string): Promise<void> {
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
}

export async function resolveFontData(font: SubtitleFont): Promise<SubtitleFont> {
  if (font.dataBase64) return font;
  if (!font.assetUrl) throw new Error("The selected font has no font data.");
  const response = await fetch(font.assetUrl);
  if (!response.ok) throw new Error(`Could not load bundled font ${font.family}.`);
  return { ...font, dataBase64: arrayBufferToBase64(await response.arrayBuffer()) };
}

export function fileToSubtitleFont(file: File): Promise<SubtitleFont> {
  return file.arrayBuffer().then((buffer) => ({
    id: `imported-${crypto.randomUUID()}`,
    family: file.name.replace(/\.(?:ttf|otf)$/i, "").replace(/[-_]+/g, " ").trim(),
    fileName: file.name,
    dataBase64: arrayBufferToBase64(buffer),
    builtIn: false
  }));
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
