import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, it, vi } from "vitest";
import SecuritySection from "./SecuritySection";
import { api } from "../auth/api";
import { initialUsername, rememberUsername } from "../lib/rememberUsername";

vi.mock("../auth/api", () => ({ api: { securitySettings: vi.fn(), saveSecuritySettings: vi.fn() } }));
afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear(); });

it("saves timeout and local bypass with a reverse-proxy warning", async () => {
  vi.mocked(api.securitySettings).mockResolvedValue({ idle_timeout_minutes: null, local_network_bypass: false });
  vi.mocked(api.saveSecuritySettings).mockResolvedValue({ idle_timeout_minutes: 10, local_network_bypass: true });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}><SecuritySection /></QueryClientProvider>);
  fireEvent.click(await screen.findByLabelText("Automatically sign out when inactive"));
  fireEvent.change(screen.getByLabelText("Minutes of inactivity"), { target: { value: "10" } });
  fireEvent.click(screen.getByLabelText("Skip password authentication on local networks"));
  expect(screen.getByText(/A reverse proxy or Docker networking/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Save security settings" }));
  await waitFor(() => expect(api.saveSecuritySettings).toHaveBeenCalledWith({ idle_timeout_minutes: 10, local_network_bypass: true }));
  expect(await screen.findByText("Security settings saved.")).toBeTruthy();
  rememberUsername("curator");
  expect(initialUsername()).toBe("curator");
  fireEvent.click(screen.getByLabelText("Remember username on this browser"));
  expect(initialUsername()).toBe("");
  expect(localStorage.getItem("mkvo.savedUsername")).toBeNull();
  expect(client.getQueryData(["security-settings"])).toEqual({ idle_timeout_minutes: 10, local_network_bypass: true });
  client.clear();
});
