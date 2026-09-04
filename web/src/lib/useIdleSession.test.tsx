import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { api } from "../auth/api";
import { useIdleSession } from "./useIdleSession";

vi.mock("../auth/api", () => ({ api: { authActivity: vi.fn(), authLogout: vi.fn() } }));
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-09-03T12:00:00Z"));
  vi.mocked(api.authActivity).mockResolvedValue(undefined);
  vi.mocked(api.authLogout).mockResolvedValue({ authenticated: false });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.clearAllMocks(); localStorage.clear(); });

it("signs out after inactivity without sending idle keepalives", async () => {
  const expired = vi.fn();
  renderHook(() => useIdleSession(1, expired));
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(expired).toHaveBeenCalledTimes(1);
  expect(api.authLogout).toHaveBeenCalledTimes(1);
  expect(api.authActivity).toHaveBeenCalledTimes(1);
});

it("counts input and activity from another tab, but not time passing", async () => {
  const expired = vi.fn();
  renderHook(() => useIdleSession(1, expired));
  await act(async () => { await vi.advanceTimersByTimeAsync(40_000); });
  act(() => window.dispatchEvent(new Event("keydown")));
  await act(async () => { await vi.advanceTimersByTimeAsync(40_000); });
  expect(expired).not.toHaveBeenCalled();
  act(() => window.dispatchEvent(new StorageEvent("storage", { key: "mkvo.lastActivity", newValue: String(Date.now()) })));
  await act(async () => { await vi.advanceTimersByTimeAsync(59_000); });
  expect(expired).not.toHaveBeenCalled();
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(expired).toHaveBeenCalledTimes(1);
});

it("does nothing when disabled and cleans up when unmounted", async () => {
  const expired = vi.fn();
  const { unmount } = renderHook(() => useIdleSession(null, expired));
  await act(async () => { await vi.advanceTimersByTimeAsync(120_000); });
  expect(api.authActivity).not.toHaveBeenCalled();
  expect(expired).not.toHaveBeenCalled();
  unmount();
  const active = renderHook(() => useIdleSession(1, expired));
  active.unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(120_000); });
  expect(expired).not.toHaveBeenCalled();
});

it("expires a suspended tab before accepting fresh input", async () => {
  const expired = vi.fn();
  renderHook(() => useIdleSession(1, expired));
  await act(async () => { await Promise.resolve(); });
  vi.setSystemTime(new Date("2026-09-03T12:02:00Z"));
  act(() => window.dispatchEvent(new Event("pointerdown")));
  expect(expired).toHaveBeenCalledOnce();
  expect(api.authActivity).toHaveBeenCalledOnce();
});
