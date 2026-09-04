import { afterEach, expect, it, vi } from "vitest";
import { api } from "./api";
import { HttpBackendClient } from "../backend/httpClient";

afterEach(() => vi.unstubAllGlobals());

it("uses same-origin cookies and announces unauthorized responses", async () => {
  const expired = vi.fn();
  window.addEventListener("mkvo:unauthorized", expired);
  const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ detail: "Incorrect username or password." }), { status: 401 }));
  vi.stubGlobal("fetch", fetchMock);
  await expect(api.authLogin("admin", "wrong")).rejects.toThrow("Incorrect username or password.");
  expect(fetchMock).toHaveBeenCalledWith("/api/auth/login", expect.objectContaining({
    credentials: "same-origin", cache: "no-store", method: "POST",
    body: JSON.stringify({ username: "admin", password: "wrong" })
  }));
  expect(expired).toHaveBeenCalledOnce();
  window.removeEventListener("mkvo:unauthorized", expired);
});

it("also announces 401 responses from protected application requests", async () => {
  const expired = vi.fn();
  window.addEventListener("mkvo:unauthorized", expired);
  const client = new HttpBackendClient({ fetch: vi.fn().mockResolvedValue(new Response('{"detail":"Authentication required."}', { status: 401 })) });
  await expect(client.getStatus()).rejects.toThrow("Authentication required.");
  expect(expired).toHaveBeenCalledOnce();
  window.removeEventListener("mkvo:unauthorized", expired);
});

it("accepts a 204 activity response without parsing JSON", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
  await expect(api.authActivity()).resolves.toBeUndefined();
});
