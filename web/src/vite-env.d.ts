/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_MKVO_TRANSPORT?: "auto" | "http" | "tauri";
  readonly VITE_MKVO_API_BASE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
