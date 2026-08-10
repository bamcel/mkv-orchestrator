import { afterEach, describe, expect, it, vi } from "vitest";

import { createBackendClient, isTauriRuntime, resetBackendClient } from "./runtime";
import { createMockBackendClient } from "./mockClient";
import { setBackendClient, getBackendClient } from "./runtime";

/**
 * Transport selection is the seam the whole migration rests on: the same React
 * UI has to reach Rust over Tauri IPC on the desktop and over HTTP in the
 * container. Picking the wrong one fails at runtime, not at compile time.
 */
describe("backend transport selection", () => {
  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    resetBackendClient();
  });

  it("uses HTTP when no Tauri runtime is present", () => {
    expect(isTauriRuntime()).toBe(false);
    expect(createBackendClient("auto").transport).toBe("http");
  });

  it("uses Tauri IPC when the runtime injected its globals", () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    expect(isTauriRuntime()).toBe(true);
    expect(createBackendClient("auto").transport).toBe("tauri");
  });

  it("honours an explicit transport over detection", () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    expect(createBackendClient("http").transport).toBe("http");
  });

  it("refuses to invent a mock backend without one being supplied", () => {
    // A mock has no real implementation, so silently constructing one would
    // make a misconfigured build look like it works.
    expect(() => createBackendClient("mock")).toThrow(/must be supplied/i);
  });
});

describe("injected backend clients", () => {
  afterEach(() => resetBackendClient());

  it("restores the previous client when the injection is undone", () => {
    const first = createMockBackendClient({ transport: "mock" });
    const restore = setBackendClient(first);
    expect(getBackendClient()).toBe(first);

    restore();
    expect(getBackendClient()).not.toBe(first);
  });

  it("fails loudly when a test calls an unconfigured method", async () => {
    const client = createMockBackendClient({});
    await expect(client.getStatus()).rejects.toThrow(/not configured/i);
  });
});
