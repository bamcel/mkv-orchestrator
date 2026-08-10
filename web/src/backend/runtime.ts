import type { BackendClient, BackendTransport, Unsubscribe } from "./client";
import { HttpBackendClient } from "./httpClient";
import { TauriBackendClient } from "./tauriClient";

export type BackendSelection = BackendTransport | "auto";

let activeClient: BackendClient | undefined;

export function isTauriRuntime(): boolean {
  if (typeof window === "undefined") {
    return false;
  }

  const runtimeWindow = window as typeof window & {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
  };
  return runtimeWindow.__TAURI_INTERNALS__ !== undefined || runtimeWindow.__TAURI__ !== undefined;
}

function configuredSelection(): BackendSelection {
  const configured = import.meta.env.VITE_MKVO_TRANSPORT?.trim().toLowerCase();
  if (configured === "http" || configured === "tauri" || configured === "mock") {
    return configured;
  }
  return "auto";
}

export function createBackendClient(selection: BackendSelection = configuredSelection()): BackendClient {
  const resolved = selection === "auto" ? (isTauriRuntime() ? "tauri" : "http") : selection;
  if (resolved === "tauri") {
    return new TauriBackendClient();
  }
  if (resolved === "mock") {
    throw new Error("The mock backend must be supplied with setBackendClient().");
  }
  return new HttpBackendClient({ baseUrl: import.meta.env.VITE_MKVO_API_BASE_URL });
}

export function getBackendClient(): BackendClient {
  activeClient ??= createBackendClient();
  return activeClient;
}

/**
 * Replaces the active client and returns a cleanup function that restores the
 * previous one. This makes component and integration tests independent of
 * global fetch and of the Tauri runtime.
 */
export function setBackendClient(client: BackendClient): Unsubscribe {
  const previous = activeClient;
  activeClient = client;
  return () => {
    if (activeClient === client) {
      activeClient = previous;
    }
  };
}

export function resetBackendClient(): void {
  activeClient = undefined;
}

export function getBackendTransport(): BackendTransport {
  return getBackendClient().transport;
}
